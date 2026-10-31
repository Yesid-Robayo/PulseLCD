//! Lo que comparten todos los procesos de PulseLCD: el agente, la ventana y el ayudante de sensores.

pub mod broker;
pub mod clock;
pub mod config;
pub mod i18n;
pub mod ipc;
pub mod log;
pub mod metrics;
pub mod paths;
pub mod theme;

pub const APP_NAME: &str = "PulseLCD";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
