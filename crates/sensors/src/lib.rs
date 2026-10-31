//! Métricas que se pueden leer sin permisos de administrador. Solo se lee lo que alguien va a mostrar: `Sampler` recibe
//! la lista de métricas en uso y deja quieto lo demás, incluida la red.

pub mod battery;
pub mod gpu;
pub mod http;
mod poller;
mod sampler;
mod system;
pub mod weather;

pub use poller::Poller;
pub use sampler::{Sampler, network_interfaces};
