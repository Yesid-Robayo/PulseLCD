//! Icono de la bandeja y su menú.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use pulse_shared::i18n::tr;
use pulse_shared::ipc::PanelState;
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, ICONINFO,
    MF_SEPARATOR, MF_STRING, SM_CXSMICON, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_APP,
};

use crate::icon::{Icon, tray_icon};
use crate::wide;

pub const MSG_TRAY: u32 = WM_APP + 1;
pub const CMD_OPEN: usize = 1;
pub const CMD_REPAIR: usize = 2;
pub const CMD_SCREEN: usize = 3;
pub const CMD_QUIT: usize = 4;

const LIGHT_INK: [u8; 3] = [0xff, 0xff, 0xff];
const DARK_INK: [u8; 3] = [0x1c, 0x1c, 0x28];
const ALERT: [u8; 3] = [0xff, 0x5d, 0x6c];

fn hicon(icon: &Icon) -> HICON {
    unsafe {
        let s = icon.size as i32;
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: s,
            biHeight: -s,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits: *mut core::ffi::c_void = null_mut();
        let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() || bits.is_null() {
            return null_mut();
        }
        let target = std::slice::from_raw_parts_mut(bits as *mut u8, icon.rgba.len());
        for (d, p) in target.as_chunks_mut::<4>().0.iter_mut().zip(icon.rgba.as_chunks::<4>().0) {
            *d = [p[2], p[1], p[0], p[3]];
        }
        let mask = CreateBitmap(s, s, 1, 1, null());
        let parts = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let handle = CreateIconIndirect(&parts);
        DeleteObject(color);
        DeleteObject(mask);
        handle
    }
}

fn copy_into(target: &mut [u16], text: &str) {
    let units: Vec<u16> = text.encode_utf16().take(target.len() - 1).collect();
    target[..units.len()].copy_from_slice(&units);
    target[units.len()] = 0;
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    state: PanelState,
    light_taskbar: bool,
}

impl Tray {
    pub fn new(hwnd: HWND, light_taskbar: bool) -> Self {
        let mut tray = Self { hwnd, icon: null_mut(), state: PanelState::Searching, light_taskbar };
        tray.add();
        tray
    }

    fn base(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = 1;
        data
    }

    fn redraw(&mut self) {
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) }.clamp(16, 64) as u32;
        let ink = if self.light_taskbar { DARK_INK } else { LIGHT_INK };
        let alert = matches!(self.state, PanelState::Searching | PanelState::Waking).then_some(ALERT);
        let new = hicon(&tray_icon(size, ink, alert));
        if !self.icon.is_null() {
            unsafe { DestroyIcon(self.icon) };
        }
        self.icon = new;
    }

    fn tip(&self) -> &'static str {
        match self.state {
            PanelState::Healthy => tr("PulseLCD: pantalla conectada", "PulseLCD: display connected"),
            PanelState::Searching => tr("PulseLCD: buscando la pantalla", "PulseLCD: looking for the display"),
            PanelState::Waking | PanelState::Syncing => tr("PulseLCD: conectando", "PulseLCD: connecting"),
            PanelState::Off => tr("PulseLCD: pantalla apagada", "PulseLCD: display off"),
            PanelState::Maintenance => tr("PulseLCD: limpiando la pantalla", "PulseLCD: cleaning the display"),
        }
    }

    /// Añade el icono, también tras reiniciarse el Explorador.
    pub fn add(&mut self) {
        self.redraw();
        let mut data = self.base();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = MSG_TRAY;
        data.hIcon = self.icon;
        copy_into(&mut data.szTip, self.tip());
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &data);
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    pub fn update(&mut self, state: PanelState) {
        if state == self.state {
            return;
        }
        self.state = state;
        self.redraw();
        let mut data = self.base();
        data.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.hIcon = self.icon;
        copy_into(&mut data.szTip, self.tip());
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub fn is_off(&self) -> bool {
        self.state == PanelState::Off
    }
}

/// Menú contextual. Es una función libre porque `TrackPopupMenu` corre un bucle propio que vuelve a entrar en el
/// procedimiento de ventana.
pub fn show_menu(hwnd: HWND, screen_off: bool) -> usize {
    unsafe {
        let menu = CreatePopupMenu();
        let item = |id: usize, text: &str| AppendMenuW(menu, MF_STRING, id, wide(text).as_ptr());
        item(CMD_OPEN, tr("Abrir PulseLCD", "Open PulseLCD"));
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(CMD_REPAIR, tr("Reparar la pantalla", "Repair the display"));
        item(
            CMD_SCREEN,
            if screen_off {
                tr("Encender la pantalla", "Turn the display on")
            } else {
                tr("Apagar la pantalla", "Turn the display off")
            },
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(CMD_QUIT, tr("Salir", "Quit"));
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        let flags = TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD;
        let chosen = TrackPopupMenu(menu, flags, point.x, point.y, 0, hwnd, null());
        DestroyMenu(menu);
        chosen as usize
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = self.base();
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
        }
    }
}
