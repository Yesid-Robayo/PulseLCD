//! PulseLCD: tu PC en una pantalla pequeña.
//!
//! Un exe con varios papeles:
//! - sin argumentos: la ventana, que solo existe mientras está abierta.
//! - `--agent`: el agente de la bandeja, único dueño del panel. `--cold` indica que arrancó con Windows.
//! - `--install-helper <cuenta>` y `--remove-helper`: instalan o quitan el ayudante de sensores. Son lo único que pide
//!   permisos de administrador, una sola vez. La cuenta es la de quien lo pidió.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod autostart;
mod icon;
mod power;
mod programs;
mod themes;
mod tray;
mod updates;
mod window;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    // estos dos corren como administrador: antes de tocar el log, que vive en una carpeta del usuario
    if has("--install-helper") {
        let account = args.iter().skip_while(|a| *a != "--install-helper").nth(1);
        std::process::exit(autostart::install_helper(account.map(String::as_str)));
    }
    if has("--remove-helper") {
        std::process::exit(autostart::remove_helper());
    }
    // un pánico cierra el proceso: que al menos quede en el log para diagnosticarlo
    std::panic::set_hook(Box::new(|info| pulse_shared::log::error(&format!("fallo inesperado: {info}"))));
    if has("--agent") {
        agent::run(has("--cold"));
    } else {
        window::run();
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
