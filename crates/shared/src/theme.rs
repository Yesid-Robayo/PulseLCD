//! Formato de los temas (`theme.json`). Un tema es un fondo y una lista plana de widgets: sin jerarquías ni casos
//! especiales, lo que se ve en la lista del editor es exactamente lo que hay en el archivo.

use serde::{Deserialize, Serialize};

use crate::metrics::Metric;

pub const MAX_WIDGETS: usize = 96;
pub const MAX_TEXT: usize = 64;
pub const MAX_HISTORY: u16 = 240;
pub const MAX_IMAGE_ZOOM: u16 = 400;

/// Color `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub const WHITE: Color = Color(255, 255, 255);
    pub const BLACK: Color = Color(0, 0, 0);

    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Color(byte(0)?, byte(2)?, byte(4)?))
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Color::parse(&text).ok_or_else(|| serde::de::Error::custom("color no válido"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

impl Orientation {
    pub fn size(self) -> (u16, u16) {
        match self {
            Orientation::Landscape => (480, 320),
            Orientation::Portrait => (320, 480),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Background {
    Color {
        color: Color,
    },
    /// `angle` en grados: 0 va de izquierda a derecha y 90 de arriba abajo.
    Gradient {
        from: Color,
        to: Color,
        angle: f32,
    },
    /// Imagen dentro de la carpeta del tema. Cubre la pantalla, y lo que no cabe se recorta: `x` e `y` eligen qué parte se
    /// ve, de 0 a 100, y `zoom` acerca la imagen, en porcentaje.
    Image {
        file: String,
        #[serde(default = "centered")]
        x: u8,
        #[serde(default = "centered")]
        y: u8,
        #[serde(default = "no_zoom")]
        zoom: u16,
    },
}

impl Background {
    pub fn image(file: impl Into<String>) -> Self {
        Self::Image { file: file.into(), x: centered(), y: centered(), zoom: no_zoom() }
    }
}

fn centered() -> u8 {
    50
}

fn no_zoom() -> u16 {
    100
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Rectángulo que ocupa un widget, en píxeles del panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Bounds {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    /// Archivo de fuente: de la carpeta del tema o de `C:\Windows\Fonts`.
    pub font: String,
    pub size: f32,
    pub color: Color,
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { font: "segoeui.ttf".into(), size: 18.0, color: Color::WHITE, align: Align::Left }
    }
}

/// Qué dibuja un widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Kind {
    /// Texto fijo.
    Label {
        text: String,
        #[serde(flatten)]
        style: TextStyle,
    },
    /// Valor de una métrica.
    Value {
        metric: Metric,
        #[serde(default = "yes")]
        unit: bool,
        #[serde(flatten)]
        style: TextStyle,
    },
    Bar {
        metric: Metric,
        color: Color,
        track: Color,
        #[serde(default)]
        radius: f32,
    },
    /// Anillo; `start` y `sweep` en grados, 0 arriba y en sentido horario.
    Ring {
        metric: Metric,
        color: Color,
        track: Color,
        thickness: f32,
        #[serde(default)]
        start: f32,
        #[serde(default = "full_turn")]
        sweep: f32,
    },
    Graph {
        metric: Metric,
        color: Color,
        #[serde(default = "default_history")]
        history: u16,
        #[serde(default = "yes")]
        fill: bool,
    },
    /// Caja de color, para agrupar datos.
    Panel {
        color: Color,
        /// 0..=100.
        opacity: u8,
        #[serde(default)]
        radius: f32,
    },
}

fn yes() -> bool {
    true
}

fn full_turn() -> f32 {
    360.0
}

fn default_history() -> u16 {
    60
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    #[serde(flatten)]
    pub bounds: Bounds,
    #[serde(flatten)]
    pub kind: Kind,
}

impl Widget {
    /// La métrica que usa, si usa alguna.
    pub fn metric(&self) -> Option<Metric> {
        match &self.kind {
            Kind::Value { metric, .. }
            | Kind::Bar { metric, .. }
            | Kind::Ring { metric, .. }
            | Kind::Graph { metric, .. } => Some(*metric),
            Kind::Label { .. } | Kind::Panel { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    #[serde(default)]
    pub orientation: Orientation,
    pub background: Background,
    pub widgets: Vec<Widget>,
}

impl Theme {
    /// Muestra datos del clima, que solo llegan si el usuario eligió su ciudad.
    pub fn shows_weather(&self) -> bool {
        self.widgets.iter().any(|widget| widget.metric().is_some_and(Metric::is_weather))
    }

    /// Rechaza lo que no se puede dibujar. Lo que venga de un archivo pasa por aquí antes de usarse.
    pub fn validate(&self) -> Result<(), String> {
        if self.widgets.len() > MAX_WIDGETS {
            return Err(format!("demasiados widgets: {}", self.widgets.len()));
        }
        let (w, h) = self.orientation.size();
        for widget in &self.widgets {
            let b = widget.bounds;
            let inside = b.x > -(w as i16) && b.y > -(h as i16) && (b.x as i32) < w as i32 && (b.y as i32) < h as i32;
            if !inside || b.w == 0 || b.h == 0 || b.w > w * 2 || b.h > h * 2 {
                return Err(format!("widget fuera de la pantalla: {b:?}"));
            }
            match &widget.kind {
                Kind::Label { text, .. } if text.chars().count() > MAX_TEXT => {
                    return Err("texto demasiado largo".into());
                }
                Kind::Label { style, .. } | Kind::Value { style, .. } => {
                    if !(4.0..=160.0).contains(&style.size) || !is_plain_file_name(&style.font) {
                        return Err(format!("fuente no válida: {}", style.font));
                    }
                }
                Kind::Graph { history, .. } if !(2..=MAX_HISTORY).contains(history) => {
                    return Err("historial no válido".into());
                }
                Kind::Ring { thickness, sweep, .. } if !(1.0..=80.0).contains(thickness) || sweep.abs() > 360.0 => {
                    return Err("anillo no válido".into());
                }
                _ => {}
            }
        }
        if let Background::Image { file, .. } = &self.background
            && !is_plain_file_name(file)
        {
            return Err(format!("imagen no válida: {file}"));
        }
        Ok(())
    }
}

/// Nombres que Windows reserva: abrirlos, con o sin extensión, puede tocar un dispositivo en vez de un archivo.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end().to_ascii_uppercase();
    let numbered = stem.strip_prefix("COM").or(stem.strip_prefix("LPT"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || numbered.is_some_and(|rest| {
            rest.chars().count() == 1 && rest.chars().all(|c| c.is_ascii_digit() || "¹²³".contains(c))
        })
}

/// Un nombre de archivo sin carpetas: un tema descargado no puede pedir archivos fuera de su sitio.
pub fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && !name.contains(['/', '\\', ':'])
        && !name.starts_with('.')
        && name.chars().all(|c| !c.is_control())
        && !is_device_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_saved_before_framing_existed_stays_centered() {
        let text = r#"{ "kind": "image", "file": "fondo.png" }"#;
        assert_eq!(serde_json::from_str::<Background>(text).unwrap(), Background::image("fondo.png"));
    }

    #[test]
    fn a_theme_cannot_name_a_device_as_a_file() {
        for name in ["COM3", "com3.ttf", "NUL", "aux.png", "LPT1", "CON", "con .png", "COM¹"] {
            assert!(!is_plain_file_name(name), "{name}");
        }
    }

    #[test]
    fn names_that_only_look_like_a_device_are_fine() {
        for name in ["comic.ttf", "console.png", "com.png", "nullable.ttf", "COM10.png"] {
            assert!(is_plain_file_name(name), "{name}");
        }
    }

    fn theme(widgets: Vec<Widget>) -> Theme {
        Theme {
            name: "Prueba".into(),
            orientation: Orientation::Landscape,
            background: Background::Color { color: Color::BLACK },
            widgets,
        }
    }

    fn label(font: &str) -> Widget {
        Widget {
            bounds: Bounds { x: 10, y: 10, w: 100, h: 30 },
            kind: Kind::Label { text: "CPU".into(), style: TextStyle { font: font.into(), ..TextStyle::default() } },
        }
    }

    #[test]
    fn a_theme_survives_a_round_trip_through_json() {
        let original = theme(vec![
            label("segoeui.ttf"),
            Widget {
                bounds: Bounds { x: 0, y: 0, w: 80, h: 80 },
                kind: Kind::Ring {
                    metric: Metric::CpuUsage,
                    color: Color(1, 2, 3),
                    track: Color(4, 5, 6),
                    thickness: 8.0,
                    start: 0.0,
                    sweep: 360.0,
                },
            },
        ]);
        let text = serde_json::to_string(&original).unwrap();
        let back: Theme = serde_json::from_str(&text).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn a_theme_cannot_reach_outside_its_folder() {
        assert!(theme(vec![label("..\\..\\secret.ttf")]).validate().is_err());
        let mut t = theme(vec![]);
        t.background = Background::image("C:\\Windows\\win.ini");
        assert!(t.validate().is_err());
    }

    #[test]
    fn a_widget_outside_the_panel_is_rejected() {
        let mut w = label("segoeui.ttf");
        w.bounds.x = 900;
        assert!(theme(vec![w]).validate().is_err());
    }

    #[test]
    fn colors_must_be_six_hex_digits() {
        assert_eq!(Color::parse("#0a1B2c"), Some(Color(10, 27, 44)));
        assert_eq!(Color::parse("0a1b2c"), None);
        assert_eq!(Color::parse("#12345"), None);
        assert_eq!(Color::parse("#ñ12345"), None);
    }
}
