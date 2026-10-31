//! Hilo que conversa con el agente: recibe el estado y la imagen del panel, y envía las órdenes. Si al abrirse la ventana
//! el agente no está, lo arranca una vez. Después ya no: si el agente se fue es porque alguien lo cerró.

use std::fs::{File, OpenOptions};
use std::os::windows::io::AsRawHandle;
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pulse_shared::config::Config;
use pulse_shared::ipc::{self, Event, PIPE_NAME, Request, Status};
use pulse_shared::log;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Pipes::PeekNamedPipe;

const RETRY: Duration = Duration::from_millis(300);
const IDLE: Duration = Duration::from_millis(20);

/// Última imagen del panel, en RGB565.
#[derive(Clone, Default)]
pub struct Mirror {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
    /// Cambia con cada imagen: la página lo usa para saber si debe pedirla.
    pub serial: u64,
}

#[derive(Default)]
pub struct Live {
    pub connected: bool,
    pub status: Option<Status>,
    pub config: Option<Config>,
    pub mirror: Mirror,
}

pub struct Bridge {
    pub live: Arc<Mutex<Live>>,
    requests: Mutex<Sender<Request>>,
}

fn waiting(file: &File) -> Option<u32> {
    let mut available = 0u32;
    let handle = file.as_raw_handle() as HANDLE;
    let ok = unsafe { PeekNamedPipe(handle, null_mut(), 0, null_mut(), &mut available, null_mut()) };
    (ok != 0).then_some(available)
}

fn apply(live: &Mutex<Live>, event: Event) {
    let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
    match event {
        Event::Status(status) => live.status = Some(*status),
        Event::Config(config) => live.config = Some(*config),
        Event::Frame { width, height, pixels } => {
            let serial = live.mirror.serial + 1;
            live.mirror = Mirror { width, height, pixels, serial };
        }
    }
}

/// Atiende la conexión hasta que se corta.
fn serve(mut pipe: File, live: &Mutex<Live>, requests: &Receiver<Request>) {
    loop {
        let mut busy = false;
        match waiting(&pipe) {
            None => return,
            Some(0) => {}
            Some(_) => {
                busy = true;
                match ipc::read_event(&mut pipe) {
                    Ok(event) => apply(live, event),
                    Err(e) => {
                        log::warn(&format!("mensaje no válido del agente: {e}"));
                        return;
                    }
                }
            }
        }
        while let Ok(request) = requests.try_recv() {
            busy = true;
            if ipc::write_request(&mut pipe, &request).is_err() {
                return;
            }
        }
        if !busy {
            std::thread::sleep(IDLE);
        }
    }
}

fn start_agent() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--agent").spawn();
    }
}

impl Bridge {
    pub fn start() -> Self {
        let live = Arc::new(Mutex::new(Live::default()));
        let (sender, requests) = mpsc::channel();
        let shared = live.clone();
        std::thread::spawn(move || {
            let mut asked = false;
            loop {
                match OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
                    Ok(pipe) => {
                        asked = true;
                        shared.lock().unwrap_or_else(|e| e.into_inner()).connected = true;
                        serve(pipe, &shared, &requests);
                        shared.lock().unwrap_or_else(|e| e.into_inner()).connected = false;
                    }
                    Err(_) if !asked => {
                        asked = true;
                        start_agent();
                    }
                    Err(_) => {}
                }
                std::thread::sleep(RETRY);
            }
        });
        Self { live, requests: Mutex::new(sender) }
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.lock().unwrap_or_else(|e| e.into_inner()).send(request);
    }
}
