//! El agente: corre en la bandeja, sin permisos de administrador, y es el único dueño del panel.
//!
//! El hilo principal solo atiende a Windows (bandeja, suspender, apagar). El trabajo va en `pipeline`, un hilo que recibe
//! órdenes por una cola; así nada más puede escribir al puerto. `server` conversa con la ventana cuando está abierta.

mod helper;
mod pipeline;
mod server;

use std::cell::{OnceCell, RefCell};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use pulse_shared::config::Config;
use pulse_shared::i18n;
use pulse_shared::ipc::{PanelState, Request};
use pulse_shared::log;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::NIN_SELECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
    PBT_APMSUSPEND, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, TranslateMessage, WM_APP,
    WM_CONTEXTMENU, WM_DESTROY, WM_ENDSESSION, WM_LBUTTONUP, WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_RBUTTONUP,
    WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use crate::tray::{self, CMD_OPEN, CMD_QUIT, CMD_REPAIR, CMD_SCREEN, MSG_TRAY, Tray};
use crate::wide;

pub use pipeline::Order;

const CLASS: &str = "PulseLCDAgent";
/// El hilo de trabajo avisa de un estado nuevo.
const MSG_STATE: u32 = WM_APP + 2;
/// El hilo de trabajo terminó.
const MSG_DONE: u32 = WM_APP + 3;
/// Selección del icono con el teclado (NIN_SELECT | NINF_KEY).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
/// Windows da unos 5 s antes de cerrar a la fuerza. Un cuadro completo son 2,4 s.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(4000);
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

struct Agent {
    tray: Tray,
    taskbar_created: u32,
}

thread_local! {
    static AGENT: RefCell<Option<Agent>> = const { RefCell::new(None) };
    /// Fuera de `AGENT`: Windows puede entrar aquí mientras la bandeja lo tiene prestado, y una orden no puede perderse.
    static ORDERS: OnceCell<Sender<Order>> = const { OnceCell::new() };
}

static WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Avisa al hilo principal del estado del panel. Lo llama el hilo de trabajo.
pub(crate) fn notify_state(state: PanelState) {
    let hwnd = WINDOW.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, MSG_STATE, state as usize, 0) };
    }
}

fn state_from(value: usize) -> PanelState {
    [
        PanelState::Searching,
        PanelState::Waking,
        PanelState::Syncing,
        PanelState::Healthy,
        PanelState::Off,
        PanelState::Maintenance,
    ]
    .get(value)
    .copied()
    .unwrap_or_default()
}

fn single_instance() -> bool {
    unsafe {
        let mutex = CreateMutexW(std::ptr::null(), 0, wide(r"Local\PulseLCD.Agent").as_ptr());
        !mutex.is_null() && matches!(WaitForSingleObject(mutex, 0), WAIT_OBJECT_0 | WAIT_ABANDONED)
    }
}

fn light_taskbar() -> bool {
    user_dword(PERSONALIZE, "SystemUsesLightTheme") == Some(1)
}

/// El tema de las apps en Windows.
pub fn apps_use_light_theme() -> bool {
    user_dword(PERSONALIZE, "AppsUseLightTheme") != Some(0)
}

/// Lee un número del registro del usuario.
fn user_dword(key: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut data = 0u32;
    let mut size = 4u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(value).as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    (read == 0).then_some(data)
}

pub fn run(cold_start: bool) {
    log::init("agente");
    if !single_instance() {
        log::info("ya hay un agente en marcha");
        return;
    }
    let config = Config::load();
    i18n::set(config.language);
    crate::autostart::apply(config.start_with_windows);
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class_name = wide(CLASS);
        let mut class: WNDCLASSEXW = std::mem::zeroed();
        class.cbSize = size_of::<WNDCLASSEXW>() as u32;
        class.lpfnWndProc = Some(window_proc);
        class.hInstance = module;
        class.lpszClassName = class_name.as_ptr();
        RegisterClassExW(&class);
        // una ventana normal y oculta: las de solo mensajes no reciben los avisos de energía
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            wide("PulseLCD Agent").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            log::error("no se pudo crear la ventana del agente");
            return;
        }
        WINDOW.store(hwnd as isize, Ordering::Relaxed);
        let (orders, inbox) = mpsc::channel();
        let (events, outbox) = server::outbox();
        let worker_orders = orders.clone();
        std::thread::spawn(move || server::run(worker_orders, outbox));
        std::thread::spawn(move || {
            pipeline::run(config, inbox, events, cold_start);
            PostMessageW(WINDOW.load(Ordering::Relaxed) as HWND, MSG_DONE, 0, 0);
        });
        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        let tray = Tray::new(hwnd, light_taskbar());
        ORDERS.with(|o| {
            let _ = o.set(orders);
        });
        AGENT.with(|a| *a.borrow_mut() = Some(Agent { tray, taskbar_created }));
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        AGENT.with(|a| a.borrow_mut().take());
    }
    log::info("agente cerrado");
}

fn order(make: impl FnOnce() -> Order) {
    ORDERS.with(|o| {
        if let Some(orders) = o.get() {
            let _ = orders.send(make());
        }
    });
}

/// Usa la bandeja si no está ocupada: `Shell_NotifyIconW` puede hacer que Windows vuelva a entrar en esta ventana.
fn with_tray(use_it: impl FnOnce(&mut Tray)) {
    AGENT.with(|a| {
        if let Ok(mut agent) = a.try_borrow_mut()
            && let Some(agent) = agent.as_mut()
        {
            use_it(&mut agent.tray);
        }
    });
}

/// Deja el panel limpio antes de que Windows siga adelante.
fn release_and_wait(make: impl FnOnce(Sender<()>) -> Order) {
    let (done, wait) = mpsc::channel();
    order(|| make(done));
    if wait.recv_timeout(SHUTDOWN_WAIT).is_err() {
        log::warn("el panel no terminó a tiempo");
    }
}

fn open_window() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
}

fn menu(hwnd: HWND) {
    let mut off = false;
    with_tray(|tray| off = tray.is_off());
    match tray::show_menu(hwnd, off) {
        CMD_OPEN => open_window(),
        CMD_REPAIR => order(|| Order::Request(Request::Repair)),
        CMD_SCREEN => order(|| Order::Request(Request::SetScreen(off))),
        CMD_QUIT => order(|| Order::Request(Request::Quit)),
        _ => {}
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar = AGENT.with(|a| a.try_borrow().ok().and_then(|x| x.as_ref().map(|x| x.taskbar_created)));
    if taskbar == Some(msg) && msg != 0 {
        with_tray(Tray::add);
        return 0;
    }
    match msg {
        MSG_TRAY => {
            match (lparam & 0xffff) as u32 {
                WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => open_window(),
                WM_RBUTTONUP | WM_CONTEXTMENU => menu(hwnd),
                _ => {}
            }
            0
        }
        MSG_STATE => {
            with_tray(|tray| tray.update(state_from(wparam)));
            0
        }
        MSG_DONE => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_POWERBROADCAST => {
            match wparam as u32 {
                PBT_APMSUSPEND => release_and_wait(Order::Suspend),
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => order(|| Order::Resume),
                _ => {}
            }
            1
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wparam != 0 {
                release_and_wait(Order::Suspend);
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
