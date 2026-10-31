//! Inicio rápido de Windows. Con él, apagar no apaga del todo: el USB sigue con corriente y el panel puede quedarse en negro
//! al encender. PulseLCD solo lo detecta y explica cómo quitarlo; el cambio lo hace el usuario en Windows.

use std::mem::zeroed;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::null_mut;

use pulse_shared::log;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Power::GetPwrCapabilities;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::{autostart, wide};

const POWER_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Power";
const FAST_STARTUP: &str = "HiberbootEnabled";

/// Necesita la hibernación: tras `powercfg /h off` el valor sigue en 1, pero ya no hace nada.
fn is_active(enabled: Option<u32>, hibernation_file: bool) -> bool {
    enabled == Some(1) && hibernation_file
}

fn fast_startup_value() -> Option<u32> {
    let (key, name) = (wide(POWER_KEY), wide(FAST_STARTUP));
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let data = (&raw mut value).cast();
    let read = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, null_mut(), data, &mut size)
    };
    (read == ERROR_SUCCESS).then_some(value)
}

pub fn fast_startup_on() -> bool {
    let hibernation_file = unsafe {
        let mut capabilities = zeroed();
        GetPwrCapabilities(&mut capabilities) && capabilities.HiberFilePresent
    };
    is_active(fast_startup_value(), hibernation_file)
}

/// La página de Windows con la casilla, que es donde se quita.
pub fn open_power_options() {
    let opened = Command::new(autostart::system_tool("control.exe"))
        .args(["/name", "Microsoft.PowerOptions", "/page", "pageGlobalSettings"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    if opened.is_err() {
        log::warn("no se pudieron abrir las opciones de energía");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_startup_counts_only_when_it_can_really_work() {
        assert!(is_active(Some(1), true));
        assert!(!is_active(Some(0), true));
        assert!(!is_active(Some(1), false), "sin hibernación no hace nada");
        assert!(!is_active(None, true));
    }

    #[test]
    fn the_registry_value_is_read_if_this_windows_has_it() {
        assert!(fast_startup_value().is_none_or(|value| value <= 1));
    }
}
