//! Selector de archivos de Windows.

use std::path::PathBuf;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OPENFILENAMEW,
};

use crate::wide;

/// `filter` en el formato de Windows, con `|` como separador: `"Imágenes|*.png;*.jpg"`.
pub fn pick_file(owner: isize, title: &str, filter: &str) -> Option<PathBuf> {
    let mut path = [0u16; 1024];
    let filter: Vec<u16> = filter.encode_utf16().map(|c| if c == b'|' as u16 { 0 } else { c }).chain([0, 0]).collect();
    let title = wide(title);
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = owner as HWND;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = path.as_mut_ptr();
    dialog.nMaxFile = path.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    dialog.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let len = path.iter().position(|c| *c == 0).unwrap_or(path.len());
    Some(PathBuf::from(String::from_utf16_lossy(&path[..len])))
}
