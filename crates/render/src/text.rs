//! Texto: carga fuentes de la carpeta del tema o de Windows y guarda cada glifo ya dibujado. Las cifras ocupan siempre el
//! mismo ancho aunque la fuente sea proporcional, para que un número que cambia no haga bailar a los demás.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont, point};
use pulse_shared::theme::{Align, Bounds, Color, TextStyle, is_plain_file_name};

use crate::canvas::Canvas;

/// Una fuente mayor no es una fuente normal.
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
/// Al llenarse se vacía: un tema normal usa unos pocos cientos.
const MAX_GLYPHS: usize = 2048;
/// Fuentes que existen en cualquier Windows, por si falta la pedida.
const FALLBACKS: [&str; 2] = ["segoeui.ttf", "arial.ttf"];
/// Límites para una fuente manipulada, que puede declarar métricas absurdas. Ninguna letra real se acerca a esto.
const MAX_SCALE: f32 = 512.0;
const MAX_GLYPH_PIXELS: usize = 1 << 20;

fn windows_fonts() -> PathBuf {
    let root = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    root.join("Fonts")
}

fn read_font(path: &Path) -> Option<FontVec> {
    if std::fs::metadata(path).ok()?.len() > MAX_FONT_BYTES {
        return None;
    }
    FontVec::try_from_vec(std::fs::read(path).ok()?).ok()
}

/// Un glifo ya dibujado.
struct Glyph {
    left: i32,
    top: i32,
    width: usize,
    height: usize,
    alpha: Vec<u8>,
}

#[derive(Default)]
pub struct Fonts {
    folder: Option<PathBuf>,
    loaded: HashMap<String, Option<Arc<FontVec>>>,
    glyphs: HashMap<(usize, GlyphId, u32), Arc<Glyph>>,
}

/// Escala para que `size` sea el tamaño de letra en píxeles, como en cualquier editor.
fn scale_for(font: &FontVec, size: f32) -> PxScale {
    let em = font.units_per_em().unwrap_or(1000.0);
    PxScale::from((size * font.height_unscaled() / em).clamp(1.0, MAX_SCALE))
}

fn rasterize(font: &FontVec, id: GlyphId, scale: PxScale) -> Glyph {
    let empty = Glyph { left: 0, top: 0, width: 0, height: 0, alpha: Vec::new() };
    let Some(outline) = font.outline_glyph(id.with_scale_and_position(scale, point(0.0, 0.0))) else { return empty };
    let bounds = outline.px_bounds();
    let (width, height) = (bounds.width() as usize, bounds.height() as usize);
    if width.saturating_mul(height) > MAX_GLYPH_PIXELS {
        return empty;
    }
    let mut alpha = vec![0u8; width * height];
    outline.draw(|x, y, coverage| {
        if let Some(cell) = alpha.get_mut(y as usize * width + x as usize) {
            *cell = (coverage * 255.0) as u8;
        }
    });
    Glyph { left: bounds.min.x as i32, top: bounds.min.y as i32, width, height, alpha }
}

impl Fonts {
    /// `folder`: carpeta del tema, donde puede traer sus propias fuentes.
    pub fn new(folder: Option<PathBuf>) -> Self {
        Self { folder, ..Self::default() }
    }

    fn open(&self, name: &str) -> Option<FontVec> {
        if !is_plain_file_name(name) {
            return None;
        }
        let own = self.folder.as_ref().map(|f| f.join(name));
        own.and_then(|p| read_font(&p)).or_else(|| read_font(&windows_fonts().join(name)))
    }

    fn font(&mut self, name: &str) -> Option<Arc<FontVec>> {
        if let Some(found) = self.loaded.get(name) {
            return found.clone();
        }
        let found = self.open(name).or_else(|| FALLBACKS.iter().find_map(|f| self.open(f))).map(Arc::new);
        self.loaded.insert(name.to_string(), found.clone());
        found
    }

    fn glyph(&mut self, font: &Arc<FontVec>, id: GlyphId, scale: PxScale) -> Arc<Glyph> {
        if self.glyphs.len() >= MAX_GLYPHS {
            self.glyphs.clear();
        }
        let key = (Arc::as_ptr(font) as usize, id, (scale.x * 8.0) as u32);
        self.glyphs.entry(key).or_insert_with(|| Arc::new(rasterize(font, id, scale))).clone()
    }

    /// Dibuja `text` dentro de `bounds`, centrado en vertical. Lo que no cabe se recorta: un texto largo no invade otro
    /// widget ni sale de su caja.
    pub fn draw(&mut self, canvas: &mut Canvas, text: &str, bounds: Bounds, style: &TextStyle) {
        let Some(font) = self.font(&style.font) else { return };
        let scale = scale_for(&font, style.size);
        let scaled = font.as_scaled(scale);
        let digit_width = ('0'..='9').map(|d| scaled.h_advance(font.glyph_id(d))).fold(0.0, f32::max);
        let cell = |ch: char, id: GlyphId| if ch.is_ascii_digit() { digit_width } else { scaled.h_advance(id) };
        let glyphs: Vec<(char, GlyphId)> = text.chars().map(|c| (c, font.glyph_id(c))).collect();
        let kern = |i: usize| glyphs.get(i + 1).map_or(0.0, |next| scaled.kern(glyphs[i].1, next.1));
        let width: f32 = glyphs.iter().enumerate().map(|(i, (c, id))| cell(*c, *id) + kern(i)).sum();
        let (bw, bh) = (bounds.w as f32, bounds.h as f32);
        let mut pen = bounds.x as f32
            + match style.align {
                Align::Left => 0.0,
                Align::Center => (bw - width) / 2.0,
                Align::Right => bw - width,
            };
        let baseline = (bounds.y as f32 + (bh + scaled.ascent() + scaled.descent()) / 2.0).round() as i32;
        let clip =
            (bounds.x as i32, bounds.y as i32, bounds.x as i32 + bounds.w as i32, bounds.y as i32 + bounds.h as i32);
        for (i, (ch, id)) in glyphs.iter().enumerate() {
            let glyph = self.glyph(&font, *id, scale);
            let room = cell(*ch, *id);
            let centered = pen + (room - scaled.h_advance(*id)) / 2.0;
            blit(canvas, &glyph, (centered.round() as i32 + glyph.left, baseline + glyph.top), clip, style.color);
            pen += room + kern(i);
        }
    }
}

fn blit(canvas: &mut Canvas, glyph: &Glyph, at: (i32, i32), clip: (i32, i32, i32, i32), color: Color) {
    for row in 0..glyph.height {
        for col in 0..glyph.width {
            let (x, y) = (at.0 + col as i32, at.1 + row as i32);
            if x >= clip.0 && x < clip.2 && y >= clip.1 && y < clip.3 {
                canvas.blend(x, y, color, glyph.alpha[row * glyph.width + col] as f32 / 255.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(canvas: &Canvas) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                if canvas.pixel(x, y) != [0, 0, 0] {
                    out.push((x, y));
                }
            }
        }
        out
    }

    fn draw(text: &str, bounds: Bounds, align: Align) -> Vec<(u16, u16)> {
        let mut canvas = Canvas::new(200, 60, Color::BLACK);
        let style = TextStyle { size: 24.0, align, ..TextStyle::default() };
        Fonts::new(None).draw(&mut canvas, text, bounds, &style);
        ink(&canvas)
    }

    #[test]
    fn text_is_drawn_and_stays_in_its_box() {
        let bounds = Bounds { x: 20, y: 10, w: 60, h: 30 };
        let points = draw("Temperatura muy larga", bounds, Align::Left);
        assert!(!points.is_empty());
        assert!(points.iter().all(|(x, y)| (20..80).contains(x) && (10..40).contains(y)));
    }

    #[test]
    fn right_alignment_ends_at_the_right_edge() {
        let bounds = Bounds { x: 0, y: 0, w: 200, h: 40 };
        let points = draw("42", bounds, Align::Right);
        let right = points.iter().map(|p| p.0).max().unwrap();
        assert!(right >= 188, "{right}");
    }

    #[test]
    fn digits_keep_the_same_width_so_numbers_do_not_dance() {
        let bounds = Bounds { x: 0, y: 0, w: 200, h: 40 };
        let left_edge = |t: &str| draw(t, bounds, Align::Right).iter().map(|p| p.0).min().unwrap();
        assert!((left_edge("111") as i32 - left_edge("888") as i32).abs() <= 4);
    }

    #[test]
    fn a_missing_font_falls_back_instead_of_going_blank() {
        let mut canvas = Canvas::new(120, 40, Color::BLACK);
        let style = TextStyle { font: "no-existe.ttf".into(), size: 20.0, ..TextStyle::default() };
        Fonts::new(None).draw(&mut canvas, "Hola", Bounds { x: 0, y: 0, w: 120, h: 40 }, &style);
        assert!(!ink(&canvas).is_empty());
    }

    #[test]
    fn a_path_in_a_font_name_is_not_followed() {
        assert!(Fonts::new(None).open("..\\..\\Windows\\Fonts\\arial.ttf").is_none());
    }
}
