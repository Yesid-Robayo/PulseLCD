//! Lienzo RGB con primitivas suavizadas. Todo se dibuja con campos de distancia: cada píxel sabe a qué distancia está del
//! borde de la figura, lo que da bordes limpios sin supermuestreo y sin dependencias de gráficos.

use pulse_panel::Frame;
use pulse_shared::theme::Color;

#[derive(Clone)]
pub struct Canvas {
    pub width: u16,
    pub height: u16,
    pixels: Vec<[u8; 3]>,
}

/// Cuánto cubre la figura un píxel, a partir de la distancia a su borde (negativa dentro).
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

fn mix(under: u8, over: u8, alpha: f32) -> u8 {
    (under as f32 + (over as f32 - under as f32) * alpha).round() as u8
}

/// Matriz de Bayer 4×4: el error de pasar a 16 bits se reparte en un patrón fijo, sin bandas en los degradados.
const BAYER: [[i16; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

fn quantize(value: u8, bits: u32, threshold: i16) -> u16 {
    let step = 1i16 << (8 - bits);
    let offset = threshold * step / 16;
    let v = (value as i16 + offset).min(255);
    (v as u16) >> (8 - bits)
}

impl Canvas {
    pub fn new(width: u16, height: u16, color: Color) -> Self {
        Self { width, height, pixels: vec![[color.0, color.1, color.2]; width as usize * height as usize] }
    }

    pub fn from_rgb(width: u16, height: u16, rgb: &[u8]) -> Self {
        let pixels = rgb.as_chunks::<3>().0.to_vec();
        Self { width, height, pixels }
    }

    pub fn pixel(&self, x: u16, y: u16) -> [u8; 3] {
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    /// RGBA de 8 bits, para la ventana.
    pub fn to_rgba(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    }

    pub fn fill_gradient(&mut self, from: Color, to: Color, angle: f32) {
        let (sin, cos) = angle.to_radians().sin_cos();
        let (w, h) = (self.width as f32, self.height as f32);
        let reach = (w * cos.abs() + h * sin.abs()).max(1.0);
        let start = -(w * cos.max(0.0) + h * sin.max(0.0)) / 2.0 - (w * (-cos).max(0.0) + h * (-sin).max(0.0)) / 2.0;
        for y in 0..self.height {
            for x in 0..self.width {
                let along = (x as f32 - w / 2.0) * cos + (y as f32 - h / 2.0) * sin;
                let t = ((along - start) / reach).clamp(0.0, 1.0);
                self.pixels[y as usize * self.width as usize + x as usize] =
                    [mix(from.0, to.0, t), mix(from.1, to.1, t), mix(from.2, to.2, t)];
            }
        }
    }

    pub fn blend(&mut self, x: i32, y: i32, color: Color, alpha: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || alpha <= 0.0 {
            return;
        }
        let p = &mut self.pixels[y as usize * self.width as usize + x as usize];
        *p = [mix(p[0], color.0, alpha), mix(p[1], color.1, alpha), mix(p[2], color.2, alpha)];
    }

    /// Recorre los píxeles de una zona, pasando el centro de cada uno, y pinta según la distancia al borde.
    fn shade(&mut self, area: [f32; 4], color: Color, opacity: f32, distance: impl Fn(f32, f32) -> f32) {
        let x0 = (area[0].floor() as i32).max(0);
        let y0 = (area[1].floor() as i32).max(0);
        let x1 = (area[2].ceil() as i32).min(self.width as i32);
        let y1 = (area[3].ceil() as i32).min(self.height as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let cover = coverage(distance(x as f32 + 0.5, y as f32 + 0.5));
                self.blend(x, y, color, cover * opacity);
            }
        }
    }

    pub fn rounded_rect(&mut self, rect: [f32; 4], radius: f32, color: Color, opacity: f32) {
        let [x, y, w, h] = rect;
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hw, hh) = (w / 2.0 - r, h / 2.0 - r);
        self.shade([x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0], color, opacity, |px, py| {
            let dx = (px - cx).abs() - hw;
            let dy = (py - cy).abs() - hh;
            dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r
        });
    }

    /// Contorno de un rectángulo redondeado.
    pub fn rounded_frame(&mut self, rect: [f32; 4], radius: f32, thickness: f32, color: Color, opacity: f32) {
        let [x, y, w, h] = rect;
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hw, hh) = (w / 2.0 - r, h / 2.0 - r);
        self.shade([x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0], color, opacity, |px, py| {
            let dx = (px - cx).abs() - hw;
            let dy = (py - cy).abs() - hh;
            let outer = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
            outer.abs() - thickness / 2.0
        });
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Color, opacity: f32) {
        self.shade([cx - r - 1.0, cy - r - 1.0, cx + r + 1.0, cy + r + 1.0], color, opacity, |x, y| {
            (x - cx).hypot(y - cy) - r
        });
    }

    /// Arco con puntas redondas. `start` y `sweep` en grados, 0 arriba y en sentido horario.
    pub fn arc(&mut self, center: (f32, f32), radius: f32, thickness: f32, start: f32, sweep: f32, color: Color) {
        let (cx, cy) = center;
        let half = thickness / 2.0;
        let reach = radius + half + 1.0;
        let tip = |deg: f32| {
            let a = (deg - 90.0).to_radians();
            (cx + radius * a.cos(), cy + radius * a.sin())
        };
        let (a, b) = (tip(start), tip(start + sweep));
        let full = sweep.abs() >= 359.5;
        self.shade([cx - reach, cy - reach, cx + reach, cy + reach], color, 1.0, |x, y| {
            let distance_to_ring = (x - cx).hypot(y - cy) - radius;
            let angle = ((y - cy).atan2(x - cx).to_degrees() + 90.0).rem_euclid(360.0);
            let offset =
                if sweep >= 0.0 { (angle - start).rem_euclid(360.0) } else { (start - angle).rem_euclid(360.0) };
            if full || offset <= sweep.abs() {
                distance_to_ring.abs() - half
            } else {
                (x - a.0).hypot(y - a.1).min((x - b.0).hypot(y - b.1)) - half
            }
        });
    }

    /// Línea con puntas redondas.
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32, color: Color) {
        let half = thickness / 2.0;
        let area = [
            a.0.min(b.0) - half - 1.0,
            a.1.min(b.1) - half - 1.0,
            a.0.max(b.0) + half + 1.0,
            a.1.max(b.1) + half + 1.0,
        ];
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.shade(area, color, 1.0, |x, y| {
            let t = (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0);
            (x - a.0 - dx * t).hypot(y - a.1 - dy * t) - half
        });
    }

    /// La imagen final en RGB565, con tramado para que los degradados no formen bandas.
    pub fn to_frame(&self) -> Frame {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for y in 0..self.height as usize {
            let row = &self.pixels[y * self.width as usize..(y + 1) * self.width as usize];
            for (x, p) in row.iter().enumerate() {
                let t = BAYER[y & 3][x & 3];
                pixels.push(quantize(p[0], 5, t) << 11 | quantize(p[1], 6, t) << 5 | quantize(p[2], 5, t));
            }
        }
        Frame { width: self.width, height: self.height, pixels }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = Color::WHITE;

    #[test]
    fn a_rounded_rect_stays_inside_its_box() {
        let mut c = Canvas::new(40, 40, Color::BLACK);
        c.rounded_rect([10.0, 10.0, 20.0, 20.0], 6.0, WHITE, 1.0);
        assert_eq!(c.pixel(20, 20), [255, 255, 255]);
        assert_eq!(c.pixel(5, 20), [0, 0, 0]);
        assert_eq!(c.pixel(10, 10), [0, 0, 0], "la esquina va redondeada");
    }

    #[test]
    fn a_half_arc_covers_only_its_side() {
        let mut c = Canvas::new(60, 60, Color::BLACK);
        c.arc((30.0, 30.0), 20.0, 4.0, 0.0, 180.0, WHITE);
        assert_eq!(c.pixel(50, 30), [255, 255, 255], "derecha");
        assert_eq!(c.pixel(10, 30), [0, 0, 0], "izquierda");
    }

    #[test]
    fn a_full_arc_is_a_closed_ring() {
        let mut c = Canvas::new(60, 60, Color::BLACK);
        c.arc((30.0, 30.0), 20.0, 4.0, 0.0, 360.0, WHITE);
        assert_eq!(c.pixel(10, 30), [255, 255, 255]);
        assert_eq!(c.pixel(30, 30), [0, 0, 0]);
    }

    #[test]
    fn drawing_outside_the_canvas_does_nothing() {
        let mut c = Canvas::new(10, 10, Color::BLACK);
        c.rounded_rect([-50.0, -50.0, 20.0, 20.0], 3.0, WHITE, 1.0);
        c.line((500.0, 500.0), (600.0, 600.0), 3.0, WHITE);
        c.blend(-1, 99, WHITE, 1.0);
        assert!((0..10).all(|y| (0..10).all(|x| c.pixel(x, y) == [0, 0, 0])));
    }

    #[test]
    fn white_and_black_survive_the_conversion_exactly() {
        let frame = Canvas::new(8, 8, WHITE).to_frame();
        assert!(frame.pixels.iter().all(|p| *p == 0xffff));
        let frame = Canvas::new(8, 8, Color::BLACK).to_frame();
        assert!(frame.pixels.iter().all(|p| *p == 0));
    }

    #[test]
    fn the_same_image_always_gives_the_same_frame() {
        let mut c = Canvas::new(64, 64, Color::BLACK);
        c.fill_gradient(Color(10, 20, 40), Color(30, 90, 120), 45.0);
        assert_eq!(c.to_frame(), c.to_frame(), "el tramado no puede cambiar entre cuadros: reenviaría la pantalla");
    }

    #[test]
    fn a_gradient_reaches_both_ends() {
        let mut c = Canvas::new(100, 1, Color::BLACK);
        c.fill_gradient(Color(0, 0, 0), Color(200, 200, 200), 0.0);
        assert!(c.pixel(0, 0)[0] < 6);
        assert!(c.pixel(99, 0)[0] > 194);
    }
}
