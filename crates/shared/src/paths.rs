//! Rutas de datos: todo vive en `%APPDATA%\PulseLCD`, salvo que exista `PulseLCDData` junto al exe (modo portable) o
//! se defina `PULSELCD_DATA_DIR` (útil para pruebas).

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    // las pruebas automáticas nunca deben tocar los datos reales del usuario
    #[cfg(test)]
    let dir = std::env::temp_dir().join("pulselcd-tests");
    #[cfg(not(test))]
    let dir = match std::env::var_os("PULSELCD_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => portable_dir()
            .filter(|d| d.is_dir())
            .or_else(|| Some(PathBuf::from(std::env::var_os("APPDATA")?).join("PulseLCD")))
            .unwrap_or_else(|| std::env::temp_dir().join("PulseLCD")),
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Dónde estarían los datos en modo portable.
#[cfg_attr(test, allow(dead_code))]
fn portable_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("PulseLCDData"))
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

pub fn log_file() -> PathBuf {
    data_dir().join("pulselcd.log")
}

pub fn themes_dir() -> PathBuf {
    let dir = data_dir().join("themes");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Escribe un archivo de forma atómica (temporal + renombrar): otro proceso nunca lee un JSON a medias. El temporal
/// se fuerza al disco antes de renombrar: si no, un corte de luz podría dejar el archivo nuevo vacío.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tests_never_use_the_real_data_folder() {
        assert!(super::data_dir().starts_with(std::env::temp_dir()));
    }
}
