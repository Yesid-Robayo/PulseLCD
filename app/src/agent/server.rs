//! Tubería con la ventana. Atiende a un cliente a la vez, y si la ventana se cuelga se pierden cuadros, no memoria.

use std::fs::File;
use std::os::windows::io::FromRawHandle;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::time::Duration;

use pulse_shared::ipc::{self, Event, PIPE_NAME, Request};
use pulse_shared::log;
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

use super::Order;
use crate::wide;

/// Solo el dueño y el sistema. Por defecto otras cuentas del PC podrían leer la tubería, con los ajustes y la ubicación.
const PIPE_ACCESS_RULES: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";
const PIPE_BUFFER: u32 = 1024 * 1024;
/// Espera tras un fallo al aceptar la conexión, para no girar en vacío.
const ACCEPT_RETRY: Duration = Duration::from_secs(1);
/// Cada cuánto se miran las órdenes de la ventana.
const POLL: Duration = Duration::from_millis(50);
const ERROR_PIPE_CONNECTED: u32 = 535;
/// Eventos que pueden esperar a la ventana antes de descartarse.
const QUEUE: usize = 4;

/// Lado del hilo de trabajo: sabe si hay ventana y le envía eventos sin bloquearse nunca.
#[derive(Clone)]
pub struct Events {
    queue: SyncSender<Event>,
    connected: Arc<AtomicBool>,
}

pub struct Outbox {
    queue: Receiver<Event>,
    connected: Arc<AtomicBool>,
}

pub fn outbox() -> (Events, Outbox) {
    let (queue, inbox) = mpsc::sync_channel(QUEUE);
    let connected = Arc::new(AtomicBool::new(false));
    (Events { queue, connected: connected.clone() }, Outbox { queue: inbox, connected })
}

impl Events {
    pub fn window_open(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn send(&self, event: Event) {
        if self.window_open()
            && let Err(TrySendError::Disconnected(_)) = self.queue.try_send(event)
        {
            log::warn("se cerró la cola de eventos");
        }
    }
}

fn create_pipe() -> Option<HANDLE> {
    unsafe {
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(PIPE_ACCESS_RULES).as_ptr(),
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
        // primera instancia: si otro programa ocupó el nombre, esto falla en vez de compartirlo con él
        let pipe = CreateNamedPipeW(
            wide(PIPE_NAME).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER,
            PIPE_BUFFER,
            0,
            &attributes,
        );
        LocalFree(descriptor);
        (pipe != INVALID_HANDLE_VALUE).then_some(pipe)
    }
}

/// `Err` si la ventana se fue.
fn waiting_bytes(pipe: HANDLE) -> Result<u32, ()> {
    let mut available = 0u32;
    if unsafe { PeekNamedPipe(pipe, null_mut(), 0, null_mut(), &mut available, null_mut()) } == 0 {
        return Err(());
    }
    Ok(available)
}

fn serve(pipe: HANDLE, file: &mut File, orders: &Sender<Order>, outbox: &Outbox) {
    // sin hilos aparte: en una tubería síncrona, una lectura en espera bloquearía las escrituras
    loop {
        match waiting_bytes(pipe) {
            Err(()) => return,
            Ok(0) => {}
            Ok(_) => match ipc::read_request(file) {
                Ok(request) => {
                    let closing = request == Request::Quit;
                    if orders.send(Order::Request(request)).is_err() || closing {
                        return;
                    }
                }
                Err(e) => {
                    log::warn(&format!("mensaje no válido de la ventana: {e}"));
                    return;
                }
            },
        }
        match outbox.queue.recv_timeout(POLL) {
            Ok(event) => {
                if ipc::write_event(file, &event).is_err() {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

pub fn run(orders: Sender<Order>, outbox: Outbox) {
    // un solo identificador para toda la vida del agente: así el nombre nunca queda libre entre dos ventanas
    let Some(pipe) = create_pipe() else {
        log::error("no se pudo crear la tubería");
        return;
    };
    let mut file = unsafe { File::from_raw_handle(pipe as _) };
    loop {
        let connected = unsafe { ConnectNamedPipe(pipe, null_mut()) != 0 || GetLastError() == ERROR_PIPE_CONNECTED };
        if connected {
            outbox.connected.store(true, Ordering::Relaxed);
            let _ = orders.send(Order::WindowConnected);
            serve(pipe, &mut file, &orders, &outbox);
            outbox.connected.store(false, Ordering::Relaxed);
            while outbox.queue.try_recv().is_ok() {}
        } else {
            std::thread::sleep(ACCEPT_RETRY);
        }
        unsafe { DisconnectNamedPipe(pipe) };
    }
}
