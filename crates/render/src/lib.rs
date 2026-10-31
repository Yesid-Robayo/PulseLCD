//! Del dato a la imagen. Una sola función, `Scene::paint`, sirve al panel, a la vista previa y a la galería, así que lo
//! que se ve en la ventana es exactamente lo que se ve en el panel. No guarda estado de qué cambió: comparar imágenes
//! es trabajo del guardián.

mod alerts;
mod builtin;
mod canvas;
mod history;
pub mod patterns;
mod scene;
mod text;

pub use alerts::{AlertView, Device};
pub use builtin::{BUILTIN, builtin_image, builtin_theme};
pub use canvas::Canvas;
pub use history::History;
pub use scene::Scene;
