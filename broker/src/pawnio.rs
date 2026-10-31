//! Temperatura de los Ryzen con PawnIO, un driver firmado que solo ejecuta módulos firmados. A diferencia de WinRing0, que
//! Windows Defender bloquea por vulnerable, no da acceso libre al hardware. Aquí solo se lee un registro de temperatura:
//! nada escribe en el hardware.

use std::ffi::c_void;
use std::mem::transmute;
use std::path::PathBuf;
use std::ptr::null_mut;

use pulse_shared::broker::{CPU_NEEDS_DRIVER, CPU_OK, CPU_UNSUPPORTED};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW};
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath};

/// Módulo firmado por el autor de PawnIO (LGPL-2.1), un archivo aparte junto al exe.
const MODULE_FILE: &str = "RyzenSMU.bin";
/// Registro del sensor en el controlador de gestión del Ryzen.
const TEMPERATURE_REGISTER: u64 = 0x0005_9800;
/// Con este bit activo, la escala empieza 49 grados más abajo.
const RANGE_BIT: u32 = 0x8_0000;

type Open = unsafe extern "system" fn(*mut *mut c_void) -> i32;
type Load = unsafe extern "system" fn(*mut c_void, *const u8, usize) -> i32;
type Execute = unsafe extern "system" fn(*mut c_void, *const u8, *const u64, usize, *mut u64, usize, *mut usize) -> i32;

pub struct CpuTemp {
    session: Option<(*mut c_void, Execute)>,
    status: u8,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn is_amd() -> bool {
    #[allow(unused_unsafe)]
    let id = unsafe { core::arch::x86_64::__cpuid(0) };
    [id.ebx, id.edx, id.ecx] == [0x6874_7541, 0x6974_6e65, 0x444d_4163]
}

/// Carpeta de Program Files preguntada a Windows, no a una variable de entorno que otro proceso pudiera cambiar.
pub fn program_files() -> Option<PathBuf> {
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

unsafe fn symbol(lib: HMODULE, name: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe { GetProcAddress(lib, name.as_ptr()) }
}

/// Grados a partir del valor del registro.
fn decode(raw: u32) -> f32 {
    let degrees = ((raw >> 21) & 0x7ff) as f32 * 0.125;
    if raw & RANGE_BIT != 0 { degrees - 49.0 } else { degrees }
}

impl CpuTemp {
    pub fn open() -> Self {
        if !is_amd() {
            return Self { session: None, status: CPU_UNSUPPORTED };
        }
        match Self::connect() {
            Ok(session) => Self { session: Some(session), status: CPU_OK },
            Err(status) => Self { session: None, status },
        }
    }

    fn connect() -> Result<(*mut c_void, Execute), u8> {
        let dll = program_files().ok_or(CPU_NEEDS_DRIVER)?.join("PawnIO").join("PawnIOLib.dll");
        if !dll.is_file() {
            return Err(CPU_NEEDS_DRIVER);
        }
        let module = std::env::current_exe()
            .ok()
            .and_then(|e| Some(e.parent()?.join(MODULE_FILE)))
            .and_then(|p| std::fs::read(p).ok())
            .ok_or(CPU_UNSUPPORTED)?;
        unsafe {
            let path = wide(&dll.to_string_lossy());
            let lib = LoadLibraryExW(path.as_ptr(), null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH);
            if lib.is_null() {
                return Err(CPU_NEEDS_DRIVER);
            }
            let open: Open = transmute(symbol(lib, b"pawnio_open\0").ok_or(CPU_NEEDS_DRIVER)?);
            let load: Load = transmute(symbol(lib, b"pawnio_load\0").ok_or(CPU_NEEDS_DRIVER)?);
            let execute: Execute = transmute(symbol(lib, b"pawnio_execute\0").ok_or(CPU_NEEDS_DRIVER)?);
            let mut handle = null_mut();
            if open(&mut handle) < 0 || handle.is_null() {
                return Err(CPU_NEEDS_DRIVER);
            }
            // cargar el módulo comprueba también que este CPU es de una familia que conoce
            if load(handle, module.as_ptr(), module.len()) < 0 {
                return Err(CPU_UNSUPPORTED);
            }
            Ok((handle, execute))
        }
    }

    pub fn status(&self) -> u8 {
        self.status
    }

    /// Grados, o NaN si no hay lectura.
    pub fn read(&self) -> f32 {
        let Some((handle, execute)) = self.session else { return f32::NAN };
        let input = [TEMPERATURE_REGISTER];
        let mut output = [0u64];
        let mut written = 0usize;
        let result = unsafe {
            execute(
                handle,
                c"ioctl_read_smu_register".as_ptr().cast(),
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut written,
            )
        };
        if result < 0 || written < 1 {
            return f32::NAN;
        }
        let degrees = decode(output[0] as u32);
        // un valor absurdo es un registro aún sin leer, no una temperatura
        if (1.0..=125.0).contains(&degrees) { degrees } else { f32::NAN }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_register_is_decoded_in_eighths_of_a_degree() {
        assert_eq!(decode(416 << 21), 52.0);
    }

    #[test]
    fn the_range_bit_lowers_the_scale() {
        assert_eq!(decode((808 << 21) | RANGE_BIT), 52.0);
    }
}
