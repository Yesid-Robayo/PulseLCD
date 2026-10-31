//! Programas abiertos. Nadie tiene por qué saber cómo se llama el `.exe` de lo que está usando, así que la lista de
//! programas ignorados se llena eligiendo de aquí. Solo salen los que tienen una ventana a la vista: son los únicos que
//! pueden estar en primer plano, y es lo único que se mide.

use std::collections::HashSet;

use serde::Serialize;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows_sys::core::BOOL;

/// Programas que presentan cuadros pero no son juegos. Se suman a los del usuario.
pub const NOT_GAMES: [&str; 29] = [
    "pulselcd.exe",
    "lockapp.exe",
    "explorer.exe",
    "dwm.exe",
    "searchhost.exe",
    "applicationframehost.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "textinputhost.exe",
    "systemsettings.exe",
    "taskmgr.exe",
    "windowsterminal.exe",
    "cmd.exe",
    "powershell.exe",
    "code.exe",
    "notepad.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "nvidia overlay.exe",
    "spotify.exe",
    "vlc.exe",
    "steam.exe",
    "steamwebhelper.exe",
];

/// Un programa abierto. El título de su ventana es lo que el usuario reconoce; se queda en la ventana de PulseLCD y nunca
/// va al log, porque puede llevar nombres de archivos o de personas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Program {
    pub exe: String,
    pub title: String,
}

/// Nombre del `.exe` de un proceso, en minúsculas.
pub fn exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit('\\').next().map(str::to_lowercase)
    }
}

unsafe extern "system" fn remember(window: HWND, list: LPARAM) -> BOOL {
    unsafe { (*(list as *mut Vec<HWND>)).push(window) };
    1
}

/// De la más reciente a la más antigua.
fn top_level_windows() -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    unsafe { EnumWindows(Some(remember), &mut list as *mut Vec<HWND> as LPARAM) };
    list
}

/// Una ventana de las que se ven en la barra de tareas. Las ocultas por Windows ("cloaked") son visibles para la API.
fn is_app_window(window: HWND) -> bool {
    unsafe {
        let mut cloaked = 0u32;
        let attribute = DWMWA_CLOAKED as u32;
        let hidden = DwmGetWindowAttribute(window, attribute, (&raw mut cloaked).cast(), size_of::<u32>() as u32) == 0
            && cloaked != 0;
        IsWindowVisible(window) != 0
            && !hidden
            && GetWindow(window, GW_OWNER).is_null()
            && GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0
    }
}

fn describe(window: HWND) -> Option<Program> {
    let mut title = [0u16; 256];
    let len = unsafe { GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32) };
    if len <= 0 {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    Some(Program { exe: exe_name(pid)?, title: String::from_utf16_lossy(&title[..len as usize]) })
}

/// Uno por programa, con su ventana más reciente, y sin los que no tiene sentido ofrecer.
fn one_of_each(found: impl IntoIterator<Item = Program>, skip: &[&str]) -> Vec<Program> {
    let mut seen: HashSet<String> = skip.iter().map(|name| name.to_string()).collect();
    let mut list: Vec<Program> = found.into_iter().filter(|p| seen.insert(p.exe.clone())).collect();
    list.sort_by_key(|p| p.title.to_lowercase());
    list
}

/// Lo que se puede elegir: lo que ya se ignora siempre no se ofrece.
pub fn open() -> Vec<Program> {
    one_of_each(top_level_windows().into_iter().filter(|w| is_app_window(*w)).filter_map(describe), &NOT_GAMES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(exe: &str, title: &str) -> Program {
        Program { exe: exe.into(), title: title.into() }
    }

    #[test]
    fn the_name_is_the_same_one_the_measuring_compares_with() {
        let name = exe_name(std::process::id()).expect("nombre");
        assert!(name.ends_with(".exe") && !name.contains('\\'), "{name}");
        assert_eq!(name, name.to_lowercase());
    }

    #[test]
    fn a_process_that_does_not_exist_has_no_name() {
        assert_eq!(exe_name(0), None);
        assert_eq!(exe_name(u32::MAX), None);
    }

    #[test]
    fn each_program_appears_once_with_its_most_recent_window() {
        let found =
            [program("obs64.exe", "OBS 31"), program("obs64.exe", "Ajustes"), program("wallpaper64.exe", "Wallpaper")];
        assert_eq!(one_of_each(found, &[]), [program("obs64.exe", "OBS 31"), program("wallpaper64.exe", "Wallpaper")]);
    }

    #[test]
    fn programs_that_are_already_ignored_are_not_offered() {
        let found = [program("code.exe", "main.rs"), program("obs64.exe", "OBS 31")];
        assert_eq!(one_of_each(found, &["code.exe"]), [program("obs64.exe", "OBS 31")]);
    }

    #[test]
    fn the_list_is_in_alphabetical_order_whatever_the_case() {
        let found = [program("b.exe", "zoom"), program("a.exe", "Audacity"), program("c.exe", "Blender")];
        let titles: Vec<String> = one_of_each(found, &[]).into_iter().map(|p| p.title).collect();
        assert_eq!(titles, ["Audacity", "Blender", "zoom"]);
    }

    #[test]
    fn listing_the_open_programs_never_offers_the_excluded_ones() {
        let list = open();
        assert!(list.iter().all(|p| !NOT_GAMES.contains(&p.exe.as_str())));
        assert!(list.iter().all(|p| !p.title.is_empty()));
    }
}
