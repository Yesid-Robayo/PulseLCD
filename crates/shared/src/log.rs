//! Log en `%APPDATA%\PulseLCD\pulselcd.log`. Cada línea lleva hora, proceso y nivel, y se escribe al disco al momento.
//! Si pasa de `MAX_BYTES`, se rota. Nunca se registran datos personales (ni rutas con el usuario, ni nombre
//! del equipo, ni la ubicación del clima).

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 512 * 1024;

static ROLE: OnceLock<&'static str> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

/// Prepara el log para este proceso (`"agente"`, `"ventana"` o `"sensores"`).
pub fn init(role: &'static str) {
    let _ = ROLE.set(role);
    info(&format!("PulseLCD {} arrancando ({role})", crate::VERSION));
}

fn write(level: &str, msg: &str) {
    // sin `init` no se escribe: ni las pruebas ni los procesos elevados deben tocar el archivo del usuario
    let Some(role) = ROLE.get() else { return };
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let line = format!("{} [{role}] {level:5} {msg}\r\n", crate::clock::now().stamp());
    let path = crate::paths::log_file();
    // el agente puede pasar semanas sin reiniciarse: el tamaño se mira en cada línea, no al arrancar
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, path.with_file_name("pulselcd.old.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
    #[cfg(debug_assertions)]
    eprint!("{line}");
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn warn(msg: &str) {
    write("AVISO", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}
