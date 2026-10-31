//! Arranque con Windows. El agente va en la clave `Run` del usuario, sin permisos.
//!
//! El ayudante de sensores necesita administrador. Se instala una sola vez, copiándolo a `Program Files`, y una tarea
//! programada lo arranca sin avisos de Windows. Esa copia es imprescindible: la tarea corre con privilegios, y si apuntara
//! a una carpeta del usuario, cualquier programa podría cambiar el archivo y ganar esos privilegios sin que nadie lo note.
//!
//! `install_helper` y `remove_helper` corren elevados: no usan el log ni la carpeta de datos, que son del usuario, y llaman
//! a las herramientas de Windows por su ruta completa, porque un nombre suelto se busca antes junto al exe.

use std::mem::{size_of, zeroed};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pulse_shared::broker::{self, FpsTool, INCLUDED_FPS_TOOL};
use pulse_shared::log;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, ERROR_SUCCESS, GetLastError};
use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, GetExitCodeProcess, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_ProgramFiles, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SHGetKnownFolderPath, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "PulseLCD";
const HELPER_TASK: &str = "PulseLCD-Sensors";
const HELPER_EXE: &str = "PulseLCD-Sensors.exe";
/// Lo que el ayudante necesita a su lado. Solo el exe es obligatorio: sin los demás funciona con menos datos.
const HELPER_FILES: [&str; 4] = [HELPER_EXE, INCLUDED_FPS_TOOL, "RyzenSMU.bin", "RyzenSMU.LICENSE.txt"];
const TASK_FILE: &str = "task.xml";
/// Instalar y quitar limpian la caché: esto solo cubre un cambio hecho fuera de la app.
const HELPER_CHECK_EVERY: Duration = Duration::from_secs(60);
/// Tiempo para que Windows pregunte y la instalación termine.
const ELEVATED_WAIT_MS: u32 = 120_000;
/// Margen para que Windows suelte el exe tras cerrarlo.
const RELEASE_WAIT: Duration = Duration::from_millis(400);
const FREE_ATTEMPTS: u32 = 8;

static HELPER_CACHE: Mutex<Option<(Instant, HelperState)>> = Mutex::new(None);

/// Cómo terminó una operación que necesita administrador.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevated {
    Done,
    /// Se rechazó el aviso de Windows: no se llegó a tocar nada.
    Denied,
    /// Se autorizó con otra cuenta: no se llegó a tocar nada.
    OtherAccount,
    Failed,
}

/// Por qué falló. El proceso elevado no puede usar el log, así que lo cuenta con su código de salida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallError {
    MissingFiles = 2,
    Copy = 3,
    Task = 4,
    OtherAccount = 5,
    Remove = 6,
}

impl InstallError {
    const ALL: [Self; 5] = [Self::MissingFiles, Self::Copy, Self::Task, Self::OtherAccount, Self::Remove];

    fn reason(self) -> &'static str {
        match self {
            Self::MissingFiles => "faltan los archivos del ayudante",
            Self::Copy => "no se pudieron copiar los archivos",
            Self::Task => "no se pudo registrar la tarea",
            Self::OtherAccount => "se autorizó con otra cuenta de Windows",
            Self::Remove => "quedó algo sin quitar",
        }
    }

    fn from_exit_code(code: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|error| *error as u32 == code)
    }
}

/// Cómo está instalado el ayudante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelperState {
    /// Existe su tarea programada, y por tanto se puede quitar.
    pub installed: bool,
    /// Está en `Program Files` y es la misma versión que esta app. Si hay tarea sin esto, hay que reinstalarlo.
    pub in_place: bool,
}

pub fn system_tool(name: &str) -> PathBuf {
    let mut buffer = [0u16; 260];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    PathBuf::from(String::from_utf16_lossy(&buffer[..len.min(buffer.len())])).join(name)
}

fn run_tool(name: &str, args: &[&str]) -> bool {
    Command::new(system_tool(name))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn schtasks(args: &[&str]) -> bool {
    run_tool("schtasks.exe", args)
}

/// `Program Files`, preguntado a Windows: una variable de entorno se podría cambiar.
fn program_files() -> Option<PathBuf> {
    unsafe {
        let mut path = null_mut();
        if SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, null_mut(), &mut path) != 0 || path.is_null() {
            return None;
        }
        let len = (0..).take_while(|i| *path.add(*i) != 0).count();
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
        CoTaskMemFree(path as *const _);
        Some(PathBuf::from(text))
    }
}

fn helper_dir() -> Option<PathBuf> {
    Some(program_files()?.join("PulseLCD"))
}

/// Con qué se miden los FPS en este PC, para contárselo al usuario. Si la copia incluida solo está junto a la app, el
/// ayudante no la ve, y `helper_is_current` pide reinstalarlo.
pub fn fps_tool() -> Option<FpsTool> {
    let (system, installed) = (program_files()?, helper_dir()?);
    let exe = std::env::current_exe().ok()?;
    Some(broker::find_fps_tool(&system, &[&installed, exe.parent()?])?.0)
}

/// Añade o quita el agente del inicio de Windows.
pub fn apply(enabled: bool) {
    let Ok(exe) = std::env::current_exe() else { return };
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
            return;
        }
        if enabled {
            let line = wide(&format!("\"{}\" --agent --cold", exe.display()));
            RegSetValueExW(key, wide(VALUE).as_ptr(), 0, REG_SZ, line.as_ptr() as *const u8, (line.len() * 2) as u32);
        } else {
            RegDeleteValueW(key, wide(VALUE).as_ptr());
        }
        RegCloseKey(key);
    }
}

/// Estado del ayudante. La consulta lanza un proceso, y la ventana pregunta cada segundo.
pub fn helper_state_cached() -> HelperState {
    let mut cache = HELPER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, value)) = *cache
        && at.elapsed() < HELPER_CHECK_EVERY
    {
        return value;
    }
    let value = HelperState { installed: schtasks(&["/Query", "/TN", HELPER_TASK]), in_place: helper_is_current() };
    *cache = Some((Instant::now(), value));
    value
}

/// `bundled` es el ayudante que acompaña a esta versión de la app, si lo hay.
fn is_current(installed: &[u8], bundled: Option<&[u8]>, can_restore_tool: bool) -> bool {
    !can_restore_tool && bundled.is_none_or(|exe| exe == installed)
}

/// Compara la copia de `Program Files` con la que acompaña a esta versión de la app.
fn helper_is_current() -> bool {
    let Some(dir) = helper_dir() else { return false };
    let Ok(installed) = std::fs::read(dir.join(HELPER_EXE)) else { return false };
    let beside = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.to_path_buf()));
    let bundled = beside.as_ref().and_then(|folder| std::fs::read(folder.join(HELPER_EXE)).ok());
    // al instalar se borra la copia incluida si está la del driver: si esa desaparece después, hay que volver a ponerla
    let tool_lost = program_files().is_some_and(|system| broker::find_fps_tool(&system, &[&dir]).is_none());
    let can_restore_tool = tool_lost && beside.is_some_and(|folder| folder.join(INCLUDED_FPS_TOOL).is_file());
    is_current(&installed, bundled.as_deref(), can_restore_tool)
}

/// Cuenta de este proceso, como `EQUIPO\usuario`. Se pregunta a Windows: una variable de entorno se podría cambiar.
fn account() -> Option<String> {
    let mut buffer = [0u16; 512];
    let mut len = buffer.len() as u32;
    let found = unsafe { GetUserNameExW(NameSamCompatible, buffer.as_mut_ptr(), &mut len) };
    found.then(|| String::from_utf16_lossy(&buffer[..len as usize]))
}

/// Vuelve a lanzar este exe como administrador y espera a que termine. `extra` va detrás de la orden y no se registra.
fn elevate(flag: &str, extra: Option<&str>) -> Elevated {
    let Ok(exe) = std::env::current_exe() else {
        log::warn(&format!("{flag}: no se encontró el exe"));
        return Elevated::Failed;
    };
    let parameters = extra.map_or(flag.to_string(), |text| format!("{flag} \"{text}\""));
    let (verb, path, args) = (wide("runas"), wide(&exe.to_string_lossy()), wide(&parameters));
    let code = unsafe {
        let mut info: SHELLEXECUTEINFOW = zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = path.as_ptr();
        info.lpParameters = args.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            let error = GetLastError();
            if error == ERROR_CANCELLED {
                return Elevated::Denied;
            }
            log::warn(&format!("{flag}: Windows no pudo lanzarlo (error {error})"));
            return Elevated::Failed;
        }
        WaitForSingleObject(info.hProcess, ELEVATED_WAIT_MS);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        code
    };
    *HELPER_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if code == 0 {
        return Elevated::Done;
    }
    let error = InstallError::from_exit_code(code);
    log::warn(&format!("{flag}: {} (código {code})", error.map_or("error inesperado", InstallError::reason)));
    if error == Some(InstallError::OtherAccount) { Elevated::OtherAccount } else { Elevated::Failed }
}

/// Pide permiso a Windows para instalar el ayudante.
pub fn request_helper_install() -> Elevated {
    elevate("--install-helper", account().as_deref())
}

pub fn request_helper_removal() -> Elevated {
    elevate("--remove-helper", None)
}

fn stop_helper() {
    schtasks(&["/End", "/TN", HELPER_TASK]);
    // también una copia abierta a mano: no debe quedar nada corriendo con privilegios
    run_tool("taskkill.exe", &["/F", "/IM", HELPER_EXE]);
    std::thread::sleep(RELEASE_WAIT);
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Definición de la tarea. Va en XML porque `schtasks /Create` por parámetros la detiene a las 72 horas y no la deja
/// arrancar con batería: el ayudante dejaría de funcionar sin avisar en un PC que no se apaga.
fn task_definition(user: &str, command: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>PulseLCD: lee la temperatura del CPU y los FPS.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
    </Exec>
  </Actions>
</Task>
"#,
        user = escape_xml(user),
        command = escape_xml(command)
    )
}

/// `--install-helper`: ya corre como administrador. Devuelve el código de salida. Si se autorizó con otra cuenta, la tarea
/// sería de esa cuenta y el ayudante nunca arrancaría en la sesión de quien lo pidió: se para antes de tocar nada.
pub fn install_helper(requested_by: Option<&str>) -> i32 {
    if requested_by.is_some_and(|wanted| account().as_deref() != Some(wanted)) {
        return InstallError::OtherAccount as i32;
    }
    match copy_and_register() {
        Ok(()) => 0,
        Err(error) => error as i32,
    }
}

fn copy_and_register() -> Result<(), InstallError> {
    let source = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.to_path_buf()));
    let (Some(source), Some(target)) = (source, helper_dir()) else { return Err(InstallError::MissingFiles) };
    if !source.join(HELPER_EXE).is_file() && !target.join(HELPER_EXE).is_file() {
        return Err(InstallError::MissingFiles);
    }
    stop_helper();
    let result = copy_files(&source, &target).and_then(|()| register_task(&target));
    // aunque algo haya fallado: la versión que ya estaba debe volver a funcionar
    schtasks(&["/Run", "/TN", HELPER_TASK]);
    result
}

fn copy_files(source: &Path, target: &Path) -> Result<(), InstallError> {
    std::fs::create_dir_all(target).map_err(|_| InstallError::Copy)?;
    let driver_tool = program_files().and_then(|folder| broker::driver_fps_tool(&folder)).is_some();
    for name in HELPER_FILES {
        // con la herramienta del driver la copia incluida sobra: cuanto menos haya con privilegios, mejor
        if driver_tool && name == INCLUDED_FPS_TOOL {
            let _ = std::fs::remove_file(target.join(name));
        } else if source != target
            && source.join(name).is_file()
            && !when_free(|| std::fs::copy(source.join(name), target.join(name)).is_ok())
        {
            return Err(InstallError::Copy);
        }
    }
    Ok(())
}

/// Windows tarda un momento variable en soltar un exe recién cerrado.
fn when_free(mut action: impl FnMut() -> bool) -> bool {
    (0..FREE_ATTEMPTS).any(|attempt| {
        if attempt > 0 {
            std::thread::sleep(RELEASE_WAIT);
        }
        action()
    })
}

fn register_task(target: &Path) -> Result<(), InstallError> {
    let user = account().ok_or(InstallError::Task)?;
    let xml = task_definition(&user, &target.join(HELPER_EXE).to_string_lossy());
    let bytes: Vec<u8> = std::iter::once(0xfeffu16).chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect();
    // dentro de Program Files: en una carpeta del usuario se podría cambiar el archivo antes de registrarlo
    let definition = target.join(TASK_FILE);
    std::fs::write(&definition, bytes).map_err(|_| InstallError::Task)?;
    let created = schtasks(&["/Create", "/TN", HELPER_TASK, "/XML", &definition.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&definition);
    if created { Ok(()) } else { Err(InstallError::Task) }
}

/// `--remove-helper`: ya corre como administrador. Solo borra sus propios archivos, por su nombre.
pub fn remove_helper() -> i32 {
    stop_helper();
    // sobrevive a su proceso, y después de esto ya nadie la cerraría
    run_tool("logman.exe", &["stop", broker::FPS_SESSION, "-ets"]);
    schtasks(&["/Delete", "/TN", HELPER_TASK, "/F"]);
    let mut clean = !schtasks(&["/Query", "/TN", HELPER_TASK]);
    if let Some(target) = helper_dir() {
        for name in HELPER_FILES {
            let path = target.join(name);
            clean &= when_free(|| !path.exists() || std::fs::remove_file(&path).is_ok());
        }
        let _ = std::fs::remove_dir(target);
    }
    if clean { 0 } else { InstallError::Remove as i32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn the_copy_waits_until_windows_releases_the_file() {
        let dir = std::env::temp_dir().join(format!("pulselcd-copy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (source, target) = (dir.join("new"), dir.join("old"));
        std::fs::write(&source, "nuevo").unwrap();
        let busy =
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(&target).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(RELEASE_WAIT * 2);
            drop(busy);
        });
        let copied = when_free(|| std::fs::copy(&source, &target).is_ok());
        release.join().unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(copied);
        assert_eq!(text, "nuevo");
    }

    #[test]
    fn each_failure_is_recognised_from_its_exit_code() {
        for error in InstallError::ALL {
            assert_eq!(InstallError::from_exit_code(error as u32), Some(error));
        }
        assert_eq!(InstallError::from_exit_code(0), None);
        assert_eq!(InstallError::from_exit_code(1), None);
    }

    #[test]
    fn a_different_build_of_the_helper_is_outdated() {
        assert!(is_current(b"a", Some(b"a"), false));
        assert!(!is_current(b"a", Some(b"b"), false));
    }

    #[test]
    fn without_a_bundled_helper_there_is_nothing_to_compare() {
        assert!(is_current(b"a", None, false));
    }

    #[test]
    fn a_lost_fps_tool_asks_for_a_reinstall_even_with_the_same_build() {
        assert!(!is_current(b"a", Some(b"a"), true));
    }

    #[test]
    fn the_account_is_given_by_windows_with_its_domain() {
        let account = account().expect("cuenta");
        let (domain, user) = account.split_once('\\').expect("EQUIPO\\usuario");
        assert!(!domain.is_empty() && !user.is_empty());
    }

    #[test]
    fn nothing_is_touched_if_another_account_authorised_it() {
        assert_eq!(install_helper(Some("OTRO\\nadie")), InstallError::OtherAccount as i32);
    }

    #[test]
    fn the_task_never_stops_by_itself() {
        let xml = task_definition("PC\\Yo", r"C:\Program Files\PulseLCD\PulseLCD-Sensors.exe");
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
    }

    #[test]
    fn names_cannot_break_the_task_definition() {
        let xml = task_definition("A&B<C>", "D\"E");
        assert!(xml.contains("A&amp;B&lt;C&gt;") && xml.contains("D&quot;E"));
    }

    #[test]
    fn windows_tools_are_called_by_their_full_path() {
        let tool = system_tool("schtasks.exe");
        assert!(tool.is_absolute(), "{tool:?}");
        assert!(tool.is_file(), "{tool:?}");
    }

    #[test]
    fn the_helper_lives_where_only_an_administrator_can_write() {
        let dir = helper_dir().expect("carpeta");
        assert!(dir.ends_with("PulseLCD"));
        assert!(dir.to_string_lossy().to_lowercase().contains("program files"));
    }
}
