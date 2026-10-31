//! Conversación entre la ventana y el agente, por una tubería con nombre. Cada mensaje lleva 4 bytes de longitud, 1 de
//! tipo y su contenido. Una sola vía de ida y vuelta: sin memoria compartida ni archivos de estado, y nada se escribe al
//! disco si la ventana está cerrada.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::metrics::Readings;

pub const PIPE_NAME: &str = r"\\.\pipe\PulseLCD.Agent";
/// Mayor que cualquier mensaje real: una configuración, o una pantalla completa en RGB565.
pub const MAX_MESSAGE: u32 = 1024 * 1024;

const KIND_JSON: u8 = 0;
const KIND_FRAME: u8 = 1;

/// Lo que la ventana le pide al agente. Solo acciones concretas: nunca una ruta a ejecutar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "request", content = "value")]
pub enum Request {
    /// Cambia la configuración completa.
    SetConfig(Box<Config>),
    /// El tema en uso cambió en disco: hay que volver a cargarlo.
    ReloadTheme,
    SetScreen(bool),
    /// Resincroniza y repinta, sin reiniciar el panel.
    Repair,
    /// Reinicia el panel con su comando RESET. Es el último recurso y el usuario lo pide a sabiendas.
    HardReset,
    Deghost,
    /// Muestra las alertas unos segundos.
    TestAlert,
    Quit,
}

/// En qué punto está la conexión con el panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PanelState {
    /// Buscando el puerto.
    #[default]
    Searching,
    /// El puerto volvió y se le da margen al firmware.
    Waking,
    /// Resincronizando y pintando.
    Syncing,
    Healthy,
    /// El usuario la apagó, o el PC está suspendido.
    Off,
    /// Limpiando la pantalla.
    Maintenance,
}

/// Disponibilidad de un grupo de sensores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SensorState {
    #[default]
    Unknown,
    Ok,
    /// Hace falta instalar PawnIO.
    NeedsDriver,
    /// Hace falta el ayudante de sensores.
    NeedsHelper,
    /// Este equipo no lo tiene o no está soportado.
    Unavailable,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub state: PanelState,
    pub port: Option<String>,
    pub readings: Readings,
    pub cpu_temp: SensorState,
    pub gpu: SensorState,
    pub fps: SensorState,
    /// Programa en primer plano, para explicar por qué no hay FPS.
    pub foreground: String,
    /// Cuántas veces volvió a conectarse desde que arrancó.
    pub reconnects: u32,
    /// Veces que el guardián detectó un panel desincronizado y lo arregló.
    pub healed: u32,
    /// Duración del último cuadro completo, en milisegundos.
    pub frame_ms: u32,
    /// Segundos sin encontrar el panel; a partir de un minuto la ventana sugiere revisar el cable.
    pub missing_secs: u32,
}

/// Lo que el agente le envía a la ventana.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Status(Box<Status>),
    Config(Box<Config>),
    /// Lo que se ve en el panel ahora mismo: RGB565, ancho y alto incluidos.
    Frame {
        width: u16,
        height: u16,
        pixels: Vec<u8>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event", content = "value")]
enum JsonEvent {
    Status(Box<Status>),
    Config(Box<Config>),
}

fn write_message(w: &mut impl Write, kind: u8, body: &[u8]) -> io::Result<()> {
    if body.len() as u64 > MAX_MESSAGE as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mensaje demasiado grande"));
    }
    let mut head = [0u8; 5];
    head[..4].copy_from_slice(&(body.len() as u32).to_le_bytes());
    head[4] = kind;
    w.write_all(&head)?;
    w.write_all(body)?;
    w.flush()
}

fn read_message(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mensaje demasiado grande"));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    Ok((head[4], body))
}

pub fn write_request(w: &mut impl Write, request: &Request) -> io::Result<()> {
    write_message(w, KIND_JSON, serde_json::to_string(request)?.as_bytes())
}

pub fn read_request(r: &mut impl Read) -> io::Result<Request> {
    let (_, body) = read_message(r)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_event(w: &mut impl Write, event: &Event) -> io::Result<()> {
    match event {
        Event::Status(s) => {
            write_message(w, KIND_JSON, serde_json::to_string(&JsonEvent::Status(s.clone()))?.as_bytes())
        }
        Event::Config(c) => {
            write_message(w, KIND_JSON, serde_json::to_string(&JsonEvent::Config(c.clone()))?.as_bytes())
        }
        Event::Frame { width, height, pixels } => {
            let mut body = Vec::with_capacity(pixels.len() + 4);
            body.extend(width.to_le_bytes());
            body.extend(height.to_le_bytes());
            body.extend(pixels);
            write_message(w, KIND_FRAME, &body)
        }
    }
}

pub fn read_event(r: &mut impl Read) -> io::Result<Event> {
    let (kind, body) = read_message(r)?;
    if kind == KIND_FRAME {
        if body.len() < 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "cuadro incompleto"));
        }
        let width = u16::from_le_bytes([body[0], body[1]]);
        let height = u16::from_le_bytes([body[2], body[3]]);
        return Ok(Event::Frame { width, height, pixels: body[4..].to_vec() });
    }
    match serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))? {
        JsonEvent::Status(s) => Ok(Event::Status(s)),
        JsonEvent::Config(c) => Ok(Event::Config(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_arrive_as_they_were_sent() {
        let mut wire = Vec::new();
        let frame = Event::Frame { width: 2, height: 1, pixels: vec![1, 2, 3, 4] };
        let status = Event::Status(Box::new(Status { port: Some("COM5".into()), ..Status::default() }));
        write_event(&mut wire, &frame).unwrap();
        write_event(&mut wire, &status).unwrap();
        let mut r = wire.as_slice();
        assert_eq!(read_event(&mut r).unwrap(), frame);
        assert_eq!(read_event(&mut r).unwrap(), status);
    }

    #[test]
    fn a_giant_length_is_refused_before_allocating() {
        let wire = [0xff, 0xff, 0xff, 0x7f, 0];
        assert!(read_event(&mut wire.as_slice()).is_err());
    }

    #[test]
    fn requests_arrive_as_they_were_sent() {
        let mut wire = Vec::new();
        write_request(&mut wire, &Request::SetScreen(false)).unwrap();
        assert_eq!(read_request(&mut wire.as_slice()).unwrap(), Request::SetScreen(false));
    }
}
