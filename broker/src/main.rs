//! Ayudante de sensores: el único proceso de PulseLCD que corre con permisos de administrador.
//!
//! Hace dos cosas y solo dos: leer la temperatura del CPU y medir los FPS. A propósito no abre temas, imágenes ni fuentes,
//! no toca la red y no escribe ningún archivo, ni siquiera un log: un proceso elevado que interpreta datos del usuario o
//! escribe en sus carpetas es la vía clásica para escalar privilegios. Solo sirve lecturas por una tubería, y lo único
//! que recibe es un byte de orden.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fps;
mod pawnio;

use std::fs::File;
use std::io::Read;
use std::os::windows::io::FromRawHandle;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use pulse_shared::broker::{PIPE_NAME, Sample, WANT_FPS, WANT_FPS_REAL_ONLY, WANT_NOTHING};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Solo el sistema, los administradores y el usuario sentado al PC pueden usar la tubería.
const PIPE_ACCESS_RULES: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";
const TICK: Duration = Duration::from_secs(1);
/// La medición de FPS se detiene tras este tiempo sin que nadie la pida.
const FPS_IDLE: Duration = Duration::from_secs(60);
const ERROR_PIPE_CONNECTED: u32 = 535;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_pipe() -> Option<HANDLE> {
    unsafe {
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let rules = wide(PIPE_ACCESS_RULES);
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            rules.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return None;
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // primera instancia: si otro programa ya creó una tubería con este nombre, esto falla y el ayudante se va
        let pipe = CreateNamedPipeW(
            wide(PIPE_NAME).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            256,
            64,
            0,
            &attributes,
        );
        LocalFree(descriptor);
        (pipe != INVALID_HANDLE_VALUE).then_some(pipe)
    }
}

/// Proceso de la ventana en primer plano.
fn foreground_pid() -> u32 {
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return 0;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        pid
    }
}

/// Última orden que llegó, sin esperar si no hay ninguna.
fn pending_order(pipe: HANDLE, reader: &mut File) -> Result<Option<u8>, ()> {
    let mut available = 0u32;
    if unsafe { PeekNamedPipe(pipe, null_mut(), 0, null_mut(), &mut available, null_mut()) } == 0 {
        return Err(());
    }
    let mut order = None;
    let mut buffer = [0u8; 16];
    while available > 0 {
        let size = (available as usize).min(buffer.len());
        let read = reader.read(&mut buffer[..size]).map_err(|_| ())?;
        if read == 0 {
            return Err(());
        }
        order = Some(buffer[read - 1]);
        available -= read as u32;
    }
    Ok(order)
}

fn serve(pipe: HANDLE, file: &mut File, temp: &pawnio::CpuTemp, meter: &mut fps::FpsMeter) {
    let mut want = WANT_NOTHING;
    let mut last_wanted = Instant::now();
    loop {
        match pending_order(pipe, file) {
            Ok(Some(order)) => want = order,
            Ok(None) => {}
            Err(()) => return,
        }
        if want == WANT_NOTHING {
            if last_wanted.elapsed() >= FPS_IDLE {
                meter.stop();
            }
        } else {
            last_wanted = Instant::now();
            meter.start(want != WANT_FPS_REAL_ONLY);
        }
        let pid = foreground_pid();
        let sample = Sample {
            cpu_temp: temp.read(),
            cpu_status: temp.status(),
            fps_status: meter.status(),
            fps_pid: pid,
            fps: if matches!(want, WANT_FPS | WANT_FPS_REAL_ONLY) { meter.fps_of(pid) } else { 0.0 },
        };
        if sample.write(file).is_err() {
            return;
        }
        std::thread::sleep(TICK);
    }
}

fn main() {
    let temp = pawnio::CpuTemp::open();
    let mut meter = fps::FpsMeter::default();
    // ya hay otro ayudante, o alguien ocupó el nombre: no hay nada que hacer aquí
    let Some(pipe) = create_pipe() else { return };
    // si el ayudante anterior murió de golpe, su sesión sigue abierta
    fps::close_session();
    // un solo identificador: así el nombre nunca queda libre entre dos conexiones
    let mut file = unsafe { File::from_raw_handle(pipe as _) };
    loop {
        let connected = unsafe { ConnectNamedPipe(pipe, null_mut()) != 0 || GetLastError() == ERROR_PIPE_CONNECTED };
        if connected {
            serve(pipe, &mut file, &temp, &mut meter);
        } else {
            std::thread::sleep(TICK);
        }
        meter.stop();
        unsafe { DisconnectNamedPipe(pipe) };
    }
}
