//! Conversación con el ayudante de sensores (`PulseLCD-Sensors.exe`), el único proceso que corre elevado.
//!
//! El ayudante no lee archivos del usuario, ni temas, ni red: solo la temperatura del CPU y los FPS. Por eso el protocolo
//! es mínimo y de tamaño fijo, sin texto que interpretar con privilegios: el agente envía un byte de orden y recibe
//! `Sample`.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

pub const PIPE_NAME: &str = r"\\.\pipe\PulseLCD.Sensors";

/// Órdenes del agente al ayudante.
pub const WANT_NOTHING: u8 = 0;
/// Medir FPS, contando los cuadros generados.
pub const WANT_FPS: u8 = 1;
/// Medir FPS solo de los cuadros reales del juego.
pub const WANT_FPS_REAL_ONLY: u8 = 2;

/// Lo que falta para leer la temperatura.
pub const CPU_OK: u8 = 0;
pub const CPU_NEEDS_DRIVER: u8 = 1;
pub const CPU_UNSUPPORTED: u8 = 2;

/// Lo que falta para medir FPS.
pub const FPS_OK: u8 = 0;
pub const FPS_NO_TOOL: u8 = 1;

const SAMPLE_BYTES: usize = 14;

/// Copia de PresentMon que acompaña a PulseLCD.
/// Sesión de seguimiento de Windows que abre la herramienta de FPS.
pub const FPS_SESSION: &str = "PulseLCD";
pub const INCLUDED_FPS_TOOL: &str = "PresentMon-x64.exe";

/// Con qué se miden los FPS. La ventana lo muestra y el ayudante lo usa, y por eso lo decide una sola función.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FpsTool {
    AmdDriver,
    NvidiaDriver,
    Included,
}

/// Herramientas que traen los drivers, dentro de Program Files: solo un administrador puede tocarlas.
const DRIVER_TOOLS: [(FpsTool, &str); 2] = [
    (FpsTool::AmdDriver, r"AMD\CNext\CNext\PresentMon-x64.exe"),
    (FpsTool::NvidiaDriver, r"NVIDIA Corporation\FrameViewSDK\bin\PresentMon_x64.exe"),
];

/// La herramienta de un driver, si hay alguna. Se prefiere porque coincide con ese driver y es la probada con Frame
/// Generation.
pub fn driver_fps_tool(program_files: &Path) -> Option<(FpsTool, PathBuf)> {
    DRIVER_TOOLS.iter().map(|(tool, path)| (*tool, program_files.join(path))).find(|(_, path)| path.is_file())
}

/// La herramienta a usar: la de un driver o, si no hay, la incluida en alguna de las carpetas dadas.
pub fn find_fps_tool(program_files: &Path, folders: &[&Path]) -> Option<(FpsTool, PathBuf)> {
    driver_fps_tool(program_files).or_else(|| {
        let own = folders.iter().map(|f| f.join(INCLUDED_FPS_TOOL)).find(|p| p.is_file())?;
        Some((FpsTool::Included, own))
    })
}

/// Una lectura del ayudante.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sample {
    /// Grados; NaN si no hay lectura.
    pub cpu_temp: f32,
    pub cpu_status: u8,
    pub fps_status: u8,
    /// Identificador del proceso en primer plano al que corresponden los FPS; 0 si no hay.
    pub fps_pid: u32,
    /// Cuadros por segundo de ese proceso; 0 si no hay.
    pub fps: f32,
}

impl Sample {
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let mut b = [0u8; SAMPLE_BYTES];
        b[0..4].copy_from_slice(&self.cpu_temp.to_le_bytes());
        b[4] = self.cpu_status;
        b[5] = self.fps_status;
        b[6..10].copy_from_slice(&self.fps_pid.to_le_bytes());
        b[10..14].copy_from_slice(&self.fps.to_le_bytes());
        w.write_all(&b)?;
        w.flush()
    }

    pub fn read(r: &mut impl Read) -> io::Result<Self> {
        let mut b = [0u8; SAMPLE_BYTES];
        r.read_exact(&mut b)?;
        let float = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(Self {
            cpu_temp: float(0),
            cpu_status: b[4],
            fps_status: b[5],
            fps_pid: u32::from_le_bytes([b[6], b[7], b[8], b[9]]),
            fps: float(10),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_with(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join("pulselcd-fps-tool").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_driver_tool_wins_over_the_included_one() {
        let system = folder_with("amd", &[r"AMD\CNext\CNext\PresentMon-x64.exe"]);
        let own = folder_with("own-1", &[INCLUDED_FPS_TOOL]);
        assert_eq!(find_fps_tool(&system, &[&own]).unwrap().0, FpsTool::AmdDriver);
    }

    #[test]
    fn the_nvidia_tool_is_found_too() {
        let system = folder_with("nvidia", &[r"NVIDIA Corporation\FrameViewSDK\bin\PresentMon_x64.exe"]);
        assert_eq!(find_fps_tool(&system, &[]).unwrap().0, FpsTool::NvidiaDriver);
    }

    #[test]
    fn without_a_driver_tool_the_included_one_is_used() {
        let (system, own) = (folder_with("empty", &[]), folder_with("own-2", &[INCLUDED_FPS_TOOL]));
        assert_eq!(find_fps_tool(&system, &[&own]).unwrap().0, FpsTool::Included);
        assert_eq!(find_fps_tool(&system, &[&system]), None);
    }

    #[test]
    fn a_sample_keeps_its_values_on_the_wire() {
        let sample = Sample { cpu_temp: 52.5, cpu_status: CPU_OK, fps_status: FPS_NO_TOOL, fps_pid: 4242, fps: 144.0 };
        let mut wire = Vec::new();
        sample.write(&mut wire).unwrap();
        assert_eq!(wire.len(), SAMPLE_BYTES);
        assert_eq!(Sample::read(&mut wire.as_slice()).unwrap(), sample);
    }
}
