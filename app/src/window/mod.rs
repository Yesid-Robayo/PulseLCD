//! La ventana: una vista web de Windows (WebView2) con la interfaz, alojada aquí. Es un proceso aparte que solo existe
//! mientras está abierta; al cerrarla no queda nada en memoria y la pantalla sigue funcionando.

mod api;
mod bridge;
mod dialogs;

use std::sync::Arc;

use pulse_shared::config::{Appearance, Config};
use pulse_shared::{i18n, log};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Icon, WindowBuilder};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow};
use wry::{WebContext, WebViewBuilder};

use crate::wide;

const TITLE: &str = "PulseLCD";
/// Clase propia: por el título también se encontraría una carpeta del Explorador que se llame igual.
const WINDOW_CLASS: &str = "PulseLCDWindow";
/// Con la barra final: sin ella también pasaría `http://pulse.localhost.ejemplo.com`.
const ORIGIN: &str = "http://pulse.localhost/";
const DARK_BACKGROUND: (u8, u8, u8) = (0x0b, 0x10, 0x14);
const LIGHT_BACKGROUND: (u8, u8, u8) = (0xf3, 0xf6, 0xf6);

/// Si ya hay una ventana, la trae al frente.
fn another_window_open() -> bool {
    unsafe {
        CreateMutexW(std::ptr::null(), 1, wide(r"Local\PulseLCD.Window").as_ptr());
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return false;
        }
        let window = FindWindowW(wide(WINDOW_CLASS).as_ptr(), std::ptr::null());
        if !window.is_null() {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        true
    }
}

fn uses_light(config: &Config) -> bool {
    match config.appearance {
        Appearance::Light => true,
        Appearance::Dark => false,
        Appearance::System => crate::agent::apps_use_light_theme(),
    }
}

/// Barra de título del mismo color que la app.
fn paint_title_bar(hwnd: isize, light: bool) {
    let (r, g, b) = if light { LIGHT_BACKGROUND } else { DARK_BACKGROUND };
    let caption = r as u32 | (g as u32) << 8 | (b as u32) << 16;
    let dark: i32 = (!light) as i32;
    unsafe {
        let window = hwnd as *mut core::ffi::c_void;
        DwmSetWindowAttribute(window, 20, &dark as *const i32 as *const _, 4); // DWMWA_USE_IMMERSIVE_DARK_MODE
        DwmSetWindowAttribute(window, 35, &caption as *const u32 as *const _, 4); // DWMWA_CAPTION_COLOR
    }
}

/// Carpeta de datos de la vista web, fuera de las carpetas que se sincronizan entre equipos.
fn web_data_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(pulse_shared::paths::data_dir)
        .join("PulseLCD")
        .join("WebView2")
}

fn window_icon() -> Option<Icon> {
    let icon = crate::icon::app_icon(64);
    Icon::from_rgba(icon.rgba, icon.size, icon.size).ok()
}

pub fn run() {
    if another_window_open() {
        return;
    }
    log::init("ventana");
    let config = Config::load();
    i18n::set(config.language);
    let light = uses_light(&config);
    let api = Arc::new(api::Api::new(bridge::Bridge::start()));
    let event_loop = EventLoopBuilder::new().build();
    let window = match WindowBuilder::new()
        .with_title(TITLE)
        .with_window_classname(WINDOW_CLASS)
        .with_inner_size(LogicalSize::new(1100.0, 720.0))
        .with_min_inner_size(LogicalSize::new(960.0, 640.0))
        .with_window_icon(window_icon())
        .with_visible(false)
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(e) => {
            log::error(&format!("no se pudo crear la ventana: {e}"));
            return;
        }
    };
    let hwnd = window.hwnd() as isize;
    api.set_owner(hwnd);
    paint_title_bar(hwnd, light);
    let mut context = WebContext::new(Some(web_data_dir()));
    let handler = api.clone();
    let (r, g, b) = if light { LIGHT_BACKGROUND } else { DARK_BACKGROUND };
    let view = WebViewBuilder::new_with_web_context(&mut context)
        .with_background_color((r, g, b, 255))
        .with_devtools(cfg!(debug_assertions))
        .with_asynchronous_custom_protocol("pulse".into(), move |_id, request, responder| {
            // en su propio hilo: una búsqueda de clima no debe congelar la ventana
            let handler = handler.clone();
            std::thread::spawn(move || responder.respond(handler.handle(request)));
        })
        .with_navigation_handler(|url| url.starts_with(ORIGIN))
        .with_new_window_req_handler(|_url, _features| wry::NewWindowResponse::Deny)
        .with_url("pulse://localhost/index.html")
        .build(&window);
    let view = match view {
        Ok(view) => view,
        Err(e) => {
            log::error(&format!("WebView2 no está disponible: {e}"));
            return;
        }
    };
    window.set_visible(true);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // se conservan aquí: al soltarlos se cerraría la vista web
        let _ = (&view, &context);
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}
