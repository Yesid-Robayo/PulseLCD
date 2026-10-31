//! Una escena es un tema cargado: su fondo ya dibujado y sus fuentes. `paint` no toca ningún dispositivo.

use std::io::{BufRead, Cursor, Seek};
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use pulse_panel::Frame;
use pulse_shared::metrics::{Metric, Readings};
use pulse_shared::theme::{Background, Bounds, Color, Kind, MAX_HISTORY, MAX_IMAGE_ZOOM, Orientation, Theme, Widget};

use crate::alerts::{self, AlertView};
use crate::builtin::builtin_image;
use crate::canvas::Canvas;
use crate::history::History;
use crate::text::Fonts;

/// Una imagen mayor no aporta nada en un panel de 480×320 y solo gasta memoria al decodificarla.
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 8192;

/// Qué parte de una imagen se ve cuando no cabe entera.
#[derive(Debug, Clone, Copy)]
struct Framing {
    x: u8,
    y: u8,
    zoom: u16,
}

pub struct Scene {
    theme: Theme,
    folder: Option<PathBuf>,
    base: Canvas,
    fonts: Fonts,
}

/// Dibuja el fondo del tema.
fn draw_background(theme: &Theme, folder: Option<&Path>) -> Result<Canvas, String> {
    let (w, h) = theme.orientation.size();
    match &theme.background {
        Background::Color { color } => Ok(Canvas::new(w, h, *color)),
        Background::Gradient { from, to, angle } => {
            let mut base = Canvas::new(w, h, Color::BLACK);
            base.fill_gradient(*from, *to, *angle);
            Ok(base)
        }
        // un tema incluido editado no copia su fondo: mientras no se cambie, sale del exe
        Background::Image { file, x, y, zoom } => {
            let framing = Framing { x: *x, y: *y, zoom: *zoom };
            match folder.map(|f| f.join(file)).filter(|path| path.is_file()) {
                Some(path) => cover(&path, w, h, framing),
                None => {
                    let bytes = builtin_image(file).ok_or("no se encuentra la imagen del tema")?;
                    let reader = image::ImageReader::new(Cursor::new(bytes));
                    fit(reader.with_guessed_format().map_err(|e| e.to_string())?, w, h, framing)
                }
            }
        }
    }
}

impl Scene {
    /// Carga `theme.json` de una carpeta.
    pub fn open(folder: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(folder.join("theme.json")).map_err(|e| e.to_string())?;
        let theme: Theme = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        Self::new(theme, Some(folder.to_path_buf()))
    }

    /// `folder`: de dónde salen su imagen y sus fuentes. Los temas incluidos no tienen.
    pub fn new(theme: Theme, folder: Option<PathBuf>) -> Result<Self, String> {
        theme.validate()?;
        let base = draw_background(&theme, folder.as_deref())?;
        Ok(Self { theme, base, fonts: Fonts::new(folder.clone()), folder })
    }

    /// Cambia el tema conservando lo ya cargado: el editor llama a esto en cada movimiento del ratón.
    pub fn update(&mut self, theme: Theme) -> Result<(), String> {
        theme.validate()?;
        if theme.background != self.theme.background || theme.orientation != self.theme.orientation {
            self.base = draw_background(&theme, self.folder.as_deref())?;
        }
        self.theme = theme;
        Ok(())
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn orientation(&self) -> Orientation {
        self.theme.orientation
    }

    /// Las métricas que necesitan guardar historia.
    pub fn graphed_metrics(&self) -> Vec<Metric> {
        let mut metrics: Vec<Metric> = self
            .theme
            .widgets
            .iter()
            .filter_map(|w| match w.kind {
                Kind::Graph { metric, .. } => Some(metric),
                _ => None,
            })
            .collect();
        // dos gráficas del mismo dato comparten historia: sin esto correría al doble de velocidad
        metrics.sort();
        metrics.dedup();
        metrics
    }

    /// Las métricas que el tema muestra, para no leer ni consultar lo que nadie ve.
    pub fn used_metrics(&self) -> Vec<Metric> {
        let mut used: Vec<Metric> = self.theme.widgets.iter().filter_map(Widget::metric).collect();
        used.sort();
        used.dedup();
        used
    }

    /// Dibuja un cuadro.
    pub fn paint(&mut self, readings: &Readings, history: &History, alert: &AlertView) -> Canvas {
        let mut canvas = self.base.clone();
        for widget in &self.theme.widgets {
            draw_widget(&mut canvas, &mut self.fonts, widget, readings, history);
        }
        alerts::paint(&mut canvas, alert);
        canvas
    }

    pub fn frame(&mut self, readings: &Readings, history: &History, alert: &AlertView) -> Frame {
        self.paint(readings, history, alert).to_frame()
    }
}

/// Escala una imagen para cubrir la pantalla sin deformarla, recortando lo que sobra.
fn cover(path: &Path, w: u16, h: u16, framing: Framing) -> Result<Canvas, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_IMAGE_BYTES {
        return Err("la imagen es demasiado grande".into());
    }
    fit(
        image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?,
        w,
        h,
        framing,
    )
}

fn fit<R: BufRead + Seek>(
    mut reader: image::ImageReader<R>,
    w: u16,
    h: u16,
    framing: Framing,
) -> Result<Canvas, String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| e.to_string())?.into_rgb8();
    let (iw, ih) = picture.dimensions();
    let (tw, th) = (w as u32, h as u32);
    if (iw, ih) == (tw, th) && framing.zoom <= 100 {
        return Ok(Canvas::from_rgb(w, h, picture.as_raw()));
    }
    // primero se recorta a la proporción de la pantalla y luego se escala: al revés, una imagen de 1×8192 pediría
    // varios GB de memoria aunque el archivo pese unos bytes
    let (cw, ch) = if iw as u64 * th as u64 > ih as u64 * tw as u64 {
        ((ih as u64 * tw as u64 / th as u64).max(1) as u32, ih)
    } else {
        (iw, (iw as u64 * th as u64 / tw as u64).max(1) as u32)
    };
    let zoom = framing.zoom.clamp(100, MAX_IMAGE_ZOOM) as u64;
    let (cw, ch) = ((cw as u64 * 100 / zoom).max(1) as u32, (ch as u64 * 100 / zoom).max(1) as u32);
    let shift = |room: u32, percent: u8| (room as u64 * percent.min(100) as u64 / 100) as u32;
    let cropped =
        image::imageops::crop_imm(&picture, shift(iw - cw, framing.x), shift(ih - ch, framing.y), cw, ch).to_image();
    let scaled = image::imageops::resize(&cropped, tw, th, FilterType::Lanczos3);
    Ok(Canvas::from_rgb(w, h, scaled.as_raw()))
}

/// Posición de un valor dentro de su rango, de 0 a 1.
fn level(metric: Metric, value: Option<f64>) -> f32 {
    let Some(value) = value else { return 0.0 };
    let (low, high) = metric.range().unwrap_or((0.0, value.abs().max(1.0)));
    ((value - low) / (high - low)).clamp(0.0, 1.0) as f32
}

fn draw_widget(c: &mut Canvas, fonts: &mut Fonts, widget: &Widget, readings: &Readings, history: &History) {
    let b = widget.bounds;
    let (x, y, w, h) = (b.x as f32, b.y as f32, b.w as f32, b.h as f32);
    match &widget.kind {
        Kind::Label { text, style } => fonts.draw(c, text, b, style),
        Kind::Value { metric, unit, style } => {
            fonts.draw(c, &metric.format(readings.get(*metric), *unit), b, style);
        }
        Kind::Panel { color, opacity, radius } => {
            c.rounded_rect([x, y, w, h], *radius, *color, *opacity as f32 / 100.0);
        }
        Kind::Bar { metric, color, track, radius } => {
            c.rounded_rect([x, y, w, h], *radius, *track, 1.0);
            let fill = level(*metric, readings.get(*metric).number());
            if fill > 0.0 {
                c.rounded_rect([x, y, (w * fill).max(h.min(w * 0.02).max(1.0)), h], *radius, *color, 1.0);
            }
        }
        Kind::Ring { metric, color, track, thickness, start, sweep } => {
            let radius = w.min(h) / 2.0 - thickness / 2.0;
            let center = (x + w / 2.0, y + h / 2.0);
            c.arc(center, radius, *thickness, *start, *sweep, *track);
            let fill = level(*metric, readings.get(*metric).number());
            if fill > 0.0 {
                c.arc(center, radius, *thickness, *start, sweep * fill, *color);
            }
        }
        Kind::Graph { metric, color, history: count, fill } => {
            graph(c, b, *metric, *color, (*count).min(MAX_HISTORY) as usize, *fill, history);
        }
    }
}

fn graph(c: &mut Canvas, b: Bounds, metric: Metric, color: Color, count: usize, fill: bool, history: &History) {
    let values = history.last(metric, count);
    let (x, y, w, h) = (b.x as f32, b.y as f32, b.w as f32, b.h as f32);
    let (low, high) = metric.range().unwrap_or_else(|| {
        // sin rango fijo, la gráfica se adapta a lo que hay en pantalla
        let top = values.iter().flatten().fold(1.0f32, |m, v| m.max(*v));
        (0.0, top as f64 * 1.15)
    });
    let step = w / (count.max(2) - 1) as f32;
    let first = count.saturating_sub(values.len());
    let point = |i: usize, v: f32| {
        let t = ((v as f64 - low) / (high - low)).clamp(0.0, 1.0) as f32;
        (x + (first + i) as f32 * step, y + h - 1.0 - t * (h - 2.0))
    };
    let points: Vec<Option<(f32, f32)>> = values.iter().enumerate().map(|(i, v)| v.map(|v| point(i, v))).collect();
    if fill {
        for pair in points.windows(2) {
            let (Some(a), Some(b)) = (pair[0], pair[1]) else { continue };
            let (from, to) = (a.0.round() as i32, b.0.round() as i32);
            for px in from..to {
                let t = (px as f32 - a.0) / (b.0 - a.0).max(0.001);
                let top = a.1 + (b.1 - a.1) * t;
                for py in top.ceil() as i32..(y + h) as i32 {
                    let depth = (py as f32 - top) / (y + h - top).max(1.0);
                    c.blend(px, py, color, 0.32 * (1.0 - depth));
                }
            }
        }
    }
    for pair in points.windows(2) {
        if let (Some(a), Some(b)) = (pair[0], pair[1]) {
            c.line(a, b, 2.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{BUILTIN, builtin_theme};

    const CENTERED: Framing = Framing { x: 50, y: 50, zoom: 100 };
    use pulse_shared::metrics::Value;
    use pulse_shared::theme::{TextStyle, Theme};

    fn bar(metric: Metric) -> Widget {
        Widget {
            bounds: Bounds { x: 10, y: 10, w: 100, h: 10 },
            kind: Kind::Bar { metric, color: Color::WHITE, track: Color(40, 40, 40), radius: 0.0 },
        }
    }

    fn theme(widgets: Vec<Widget>) -> Theme {
        Theme {
            name: "x".into(),
            orientation: Orientation::Landscape,
            background: Background::Color { color: Color::BLACK },
            widgets,
        }
    }

    fn readings(metric: Metric, value: f64) -> Readings {
        let mut r = Readings::default();
        r.set(metric, Value::Number(value));
        r
    }

    #[test]
    fn a_bar_fills_in_proportion_to_its_value() {
        let mut scene = Scene::new(theme(vec![bar(Metric::CpuUsage)]), None).unwrap();
        let c = scene.paint(&readings(Metric::CpuUsage, 50.0), &History::default(), &AlertView::default());
        assert_eq!(c.pixel(30, 15), [255, 255, 255]);
        assert_eq!(c.pixel(90, 15), [40, 40, 40]);
    }

    #[test]
    fn a_missing_sensor_leaves_the_bar_empty() {
        let mut scene = Scene::new(theme(vec![bar(Metric::CpuUsage)]), None).unwrap();
        let c = scene.paint(&Readings::default(), &History::default(), &AlertView::default());
        assert_eq!(c.pixel(12, 15), [40, 40, 40]);
    }

    #[test]
    fn the_same_data_always_paints_the_same_frame() {
        for id in BUILTIN {
            let mut scene = Scene::new(builtin_theme(id).unwrap(), None).unwrap();
            let r = Readings::samples();
            let a = scene.frame(&r, &History::default(), &AlertView::default());
            let b = scene.frame(&r, &History::default(), &AlertView::default());
            assert_eq!(a, b, "{id}");
        }
    }

    #[test]
    fn showing_an_alert_only_sends_the_alert() {
        for id in BUILTIN {
            let mut scene = Scene::new(builtin_theme(id).unwrap(), None).unwrap();
            let readings = Readings::samples();
            let history = History::sample(&scene.graphed_metrics());
            let calm = scene.frame(&readings, &history, &AlertView::default());
            let alert = scene.frame(&readings, &history, &AlertView::demo());
            let rects = pulse_panel::diff::changed_rects(&calm, &alert);
            let bytes: usize = rects.iter().map(|r| r.area() * 2).sum();
            // el panel recibe 128 kB/s: con esto el aviso aparece en una fracción de segundo, sin barrido
            assert!(bytes < 48_000, "{id}: {bytes} bytes en {rects:?}");
        }
    }

    #[test]
    fn a_theme_only_asks_for_what_it_shows() {
        let scene = Scene::new(theme(vec![bar(Metric::RamPercent), bar(Metric::RamPercent)]), None).unwrap();
        assert_eq!(scene.used_metrics(), [Metric::RamPercent]);
    }

    #[test]
    fn a_broken_theme_is_refused_before_it_can_draw() {
        let mut t = theme(vec![]);
        t.widgets.push(Widget {
            bounds: Bounds { x: 0, y: 0, w: 10, h: 10 },
            kind: Kind::Label { text: "a".into(), style: TextStyle { size: 9000.0, ..TextStyle::default() } },
        });
        assert!(Scene::new(t, None).is_err());
    }

    #[test]
    fn an_image_theme_without_a_folder_is_refused() {
        let mut t = theme(vec![]);
        t.background = Background::image("fondo.png");
        assert!(Scene::new(t, None).is_err());
    }

    #[test]
    fn a_wide_image_covers_the_panel_without_distortion() {
        let dir = std::env::temp_dir().join("pulselcd-render-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fondo.png");
        let picture =
            image::RgbImage::from_fn(
                960,
                320,
                |x, _| {
                    if x < 480 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) }
                },
            );
        picture.save(&path).unwrap();
        let c = cover(&path, 480, 320, CENTERED).unwrap();
        assert_eq!((c.width, c.height), (480, 320));
        assert!(c.pixel(10, 100)[0] > 200, "izquierda roja");
        assert!(c.pixel(470, 100)[2] > 200, "derecha azul");
        let left = cover(&path, 480, 320, Framing { x: 0, ..CENTERED }).unwrap();
        assert!(left.pixel(470, 100)[0] > 200, "encuadrada a la izquierda solo se ve el rojo");
        let right = cover(&path, 480, 320, Framing { x: 100, ..CENTERED }).unwrap();
        assert!(right.pixel(10, 100)[2] > 200, "encuadrada a la derecha solo se ve el azul");
    }

    #[test]
    fn zooming_shows_a_smaller_part_of_the_image() {
        let dir = std::env::temp_dir().join("pulselcd-render-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cuadros.png");
        let picture =
            image::RgbImage::from_fn(480, 320, |x, _| image::Rgb(if x < 120 { [255, 0, 0] } else { [0, 255, 0] }));
        picture.save(&path).unwrap();
        assert!(cover(&path, 480, 320, CENTERED).unwrap().pixel(5, 100)[0] > 200);
        let near = cover(&path, 480, 320, Framing { zoom: 200, ..CENTERED }).unwrap();
        assert!(near.pixel(5, 100)[1] > 200, "con zoom, el borde rojo queda fuera");
    }

    #[test]
    fn framing_values_out_of_range_cannot_break_the_crop() {
        let dir = std::env::temp_dir().join("pulselcd-render-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limites.png");
        image::RgbImage::new(64, 64).save(&path).unwrap();
        for framing in [Framing { x: 255, y: 255, zoom: 65535 }, Framing { x: 0, y: 0, zoom: 0 }] {
            let c = cover(&path, 480, 320, framing).unwrap();
            assert_eq!((c.width, c.height), (480, 320));
        }
    }

    #[test]
    fn a_tiny_file_with_extreme_proportions_cannot_exhaust_memory() {
        let dir = std::env::temp_dir().join("pulselcd-render-test");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, size) in [("alta.png", (1, 8192)), ("ancha.png", (8192, 1))] {
            let path = dir.join(name);
            image::RgbImage::new(size.0, size.1).save(&path).unwrap();
            let c = cover(&path, 480, 320, CENTERED).unwrap();
            assert_eq!((c.width, c.height), (480, 320));
        }
    }

    #[test]
    fn two_graphs_of_one_metric_share_their_history() {
        let graph = |x| Widget {
            bounds: Bounds { x, y: 0, w: 100, h: 50 },
            kind: Kind::Graph { metric: Metric::CpuUsage, color: Color::WHITE, history: 10, fill: false },
        };
        let scene = Scene::new(theme(vec![graph(0), graph(120)]), None).unwrap();
        assert_eq!(scene.graphed_metrics(), [Metric::CpuUsage]);
    }

    #[test]
    fn a_graph_draws_only_where_there_is_data() {
        let widget = Widget {
            bounds: Bounds { x: 0, y: 0, w: 100, h: 50 },
            kind: Kind::Graph { metric: Metric::CpuUsage, color: Color::WHITE, history: 10, fill: false },
        };
        let mut scene = Scene::new(theme(vec![widget]), None).unwrap();
        let mut history = History::default();
        for _ in 0..4 {
            history.push(&[Metric::CpuUsage], &readings(Metric::CpuUsage, 50.0));
        }
        let c = scene.paint(&Readings::default(), &history, &AlertView::default());
        assert_eq!(c.pixel(10, 25), [0, 0, 0], "sin datos antiguos");
        assert_ne!(c.pixel(95, 25), [0, 0, 0], "lo nuevo entra por la derecha");
    }
}
