//! Todo lo que toca el panel. `protocol` son solo bytes; `serial` y `discover` hablan con Windows; `diff` decide qué
//! enviar y `guardian` mantiene la conexión sana. El guardián solo conoce las interfaces `Bus` y `Wire`, así que sus
//! reglas se prueban con un panel simulado que falla a voluntad.

pub mod diff;
pub mod discover;
pub mod guardian;
pub mod protocol;
pub mod serial;
#[cfg(test)]
mod simulator;

pub use guardian::{Bus, Clock, Guardian, LinkError, SystemBus, SystemClock, Wire};
pub use protocol::{Frame, Rect, Rotation};
