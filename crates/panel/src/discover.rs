//! Busca el puerto del panel en la lista de dispositivos de Windows, por su identidad USB. Sin consultas WMI: esto corre
//! cada segundo mientras el panel no está.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DICS_FLAG_GLOBAL, DIGCF_PRESENT, DIREG_DEV, GUID_DEVCLASS_PORTS, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiOpenDevRegKey,
};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Registry::{KEY_READ, REG_SZ, RegCloseKey, RegQueryValueExW};

/// Un puerto serie presente en el sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub is_panel: bool,
}

/// Identidad USB de los paneles probados.
fn looks_like_panel(instance_id: &str) -> bool {
    let id = instance_id.to_ascii_uppercase();
    id.contains("VID_1A86&PID_5722") || id.contains("USB35INCHIPS")
}

pub fn ports() -> Vec<Port> {
    let mut found = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(&GUID_DEVCLASS_PORTS, null(), null_mut(), DIGCF_PRESENT);
        if set as isize == INVALID_HANDLE_VALUE as isize {
            return found;
        }
        for index in 0.. {
            let mut info: SP_DEVINFO_DATA = std::mem::zeroed();
            info.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, index, &mut info) == 0 {
                break;
            }
            let mut id = [0u16; 256];
            let mut len = 0u32;
            if SetupDiGetDeviceInstanceIdW(set, &info, id.as_mut_ptr(), id.len() as u32, &mut len) == 0 {
                continue;
            }
            let instance = String::from_utf16_lossy(&id[..(len as usize).saturating_sub(1).min(id.len())]);
            let key = SetupDiOpenDevRegKey(set, &info, DICS_FLAG_GLOBAL, 0, DIREG_DEV, KEY_READ);
            if key as isize == INVALID_HANDLE_VALUE as isize {
                continue;
            }
            let mut name = [0u16; 32];
            let mut size = size_of_val(&name) as u32;
            let mut kind = 0u32;
            let value: Vec<u16> = "PortName".encode_utf16().chain(std::iter::once(0)).collect();
            let read =
                RegQueryValueExW(key, value.as_ptr(), null(), &mut kind, name.as_mut_ptr() as *mut u8, &mut size);
            RegCloseKey(key);
            if read != 0 || kind != REG_SZ {
                continue;
            }
            let chars = (size as usize / 2).min(name.len());
            let text = String::from_utf16_lossy(&name[..chars]).trim_end_matches('\0').to_string();
            if text.starts_with("COM") {
                found.push(Port { name: text, is_panel: looks_like_panel(&instance) });
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    found.sort_by_key(|p| p.name.trim_start_matches("COM").parse::<u32>().unwrap_or(u32::MAX));
    found
}

/// El puerto a usar: el pedido si existe, o el del panel en modo automático.
pub fn pick(ports: &[Port], wanted: &str) -> Option<String> {
    if wanted.eq_ignore_ascii_case("auto") {
        ports.iter().find(|p| p.is_panel).map(|p| p.name.clone())
    } else {
        ports.iter().find(|p| p.name.eq_ignore_ascii_case(wanted)).map(|p| p.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, is_panel: bool) -> Port {
        Port { name: name.into(), is_panel }
    }

    #[test]
    fn auto_only_takes_a_real_panel() {
        assert_eq!(pick(&[port("COM3", false), port("COM5", true)], "auto").as_deref(), Some("COM5"));
        assert_eq!(pick(&[port("COM3", false)], "auto"), None);
    }

    #[test]
    fn a_chosen_port_must_exist() {
        assert_eq!(pick(&[port("COM3", false)], "com3").as_deref(), Some("COM3"));
        assert_eq!(pick(&[port("COM3", false)], "COM7"), None);
    }

    #[test]
    fn the_panel_is_recognised_by_its_usb_identity() {
        assert!(looks_like_panel(r"USB\VID_1A86&PID_5722\USB35INCHIPSV2"));
        assert!(!looks_like_panel(r"USB\VID_1A86&PID_7523\5&1234"));
    }
}
