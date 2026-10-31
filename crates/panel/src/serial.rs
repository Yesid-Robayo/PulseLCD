//! Puerto serie con la API directa de Windows. Se evita cualquier capa intermedia a propósito: `WriteFile` dice cuántos
//! bytes salieron de verdad, y una escritura parcial es justo el fallo que deja al panel desincronizado sin dar error.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, PURGE_RXCLEAR, PURGE_TXCLEAR, PurgeComm, SetCommState,
    SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING, WriteFile};

use crate::guardian::{LinkError, Wire};

const BAUD_RATE: u32 = 115_200;
/// Un bloque tarda 30 ms; si en este tiempo no salió, el panel ya no está recibiendo.
const WRITE_TIMEOUT_MS: u32 = 3000;

// Campos de bits de DCB: fBinary (0), fOutxCtsFlow (2), fRtsControl (12-13) = RTS_CONTROL_HANDSHAKE (2). El DTR queda
// desactivado: al abrir el puerto no debe pasarle nada al panel.
const DCB_FLAGS: u32 = 1 | (1 << 2) | (2 << 12);

pub struct Serial {
    handle: HANDLE,
}

// El identificador solo se usa desde el hilo del guardián, que es su único dueño.
unsafe impl Send for Serial {}

impl Serial {
    pub fn open(port: &str) -> Result<Self, LinkError> {
        // el prefijo hace falta a partir de COM10, y no estorba antes
        let path: Vec<u16> = format!(r"\\.\{port}").encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let handle = CreateFileW(path.as_ptr(), GENERIC_WRITE, 0, null(), OPEN_EXISTING, 0, null_mut());
            if handle == INVALID_HANDLE_VALUE {
                return Err(LinkError::Open(GetLastError()));
            }
            let serial = Self { handle };
            let mut dcb: DCB = std::mem::zeroed();
            dcb.DCBlength = size_of::<DCB>() as u32;
            if GetCommState(handle, &mut dcb) == 0 {
                return Err(LinkError::Open(GetLastError()));
            }
            dcb.BaudRate = BAUD_RATE;
            dcb.ByteSize = 8;
            dcb.Parity = NOPARITY;
            dcb.StopBits = ONESTOPBIT;
            dcb._bitfield = DCB_FLAGS;
            if SetCommState(handle, &dcb) == 0 {
                return Err(LinkError::Open(GetLastError()));
            }
            let timeouts = COMMTIMEOUTS {
                ReadIntervalTimeout: 0,
                ReadTotalTimeoutMultiplier: 0,
                ReadTotalTimeoutConstant: 0,
                WriteTotalTimeoutMultiplier: 0,
                WriteTotalTimeoutConstant: WRITE_TIMEOUT_MS,
            };
            if SetCommTimeouts(handle, &timeouts) == 0 {
                return Err(LinkError::Open(GetLastError()));
            }
            // lo que quedara de una sesión mal cerrada no debe mezclarse con lo nuevo
            PurgeComm(handle, PURGE_TXCLEAR | PURGE_RXCLEAR);
            Ok(serial)
        }
    }
}

impl Wire for Serial {
    fn write(&mut self, data: &[u8]) -> Result<(), LinkError> {
        let mut written = 0u32;
        let ok = unsafe { WriteFile(self.handle, data.as_ptr(), data.len() as u32, &mut written, null_mut()) };
        if ok == 0 {
            return Err(LinkError::Write(unsafe { GetLastError() }));
        }
        if written as usize != data.len() {
            return Err(LinkError::Partial { written: written as usize, wanted: data.len() });
        }
        Ok(())
    }
}

impl Drop for Serial {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}
