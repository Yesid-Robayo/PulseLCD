//! Avisos sobre el tema: un marco de color y, en la esquina, una placa por cada cosa que necesita atención. Se ven de
//! reojo, sin leer nada: rojo es temperatura, ámbar es batería baja y verde es carga completa.

pub use pulse_shared::metrics::Device;
use pulse_shared::theme::Color;

use crate::canvas::Canvas;

const HOT: Color = Color(0xff, 0x3b, 0x4e);
const LOW: Color = Color(0xff, 0xb0, 0x20);
const FULL: Color = Color(0x2e, 0xe0, 0xa8);
const OFFLINE: Color = Color(0xff, 0x6b, 0x7a);
const PLATE: Color = Color(0x10, 0x14, 0x1c);

const MARGIN: f32 = 12.0;
const PLATE_W: f32 = 42.0;
const PLATE_H: f32 = 36.0;
const GAP: f32 = 6.0;

/// Lo que hay que avisar ahora mismo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertView {
    pub hot: bool,
    /// Dispositivos con la batería baja.
    pub low: Vec<Device>,
    /// Dispositivos que terminaron de cargar.
    pub full: Vec<Device>,
    /// La fuente de baterías dejó de responder.
    pub source_lost: bool,
}

impl AlertView {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Una muestra de cada aviso, para el botón de probar.
    pub fn demo() -> Self {
        Self { hot: true, low: vec![Device::Mouse], full: vec![Device::Headset], source_lost: false }
    }
}

pub fn paint(canvas: &mut Canvas, alert: &AlertView) {
    if alert.is_empty() {
        return;
    }
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    let border = if alert.hot {
        Some(HOT)
    } else if !alert.low.is_empty() {
        Some(LOW)
    } else if !alert.full.is_empty() {
        Some(FULL)
    } else {
        None
    };
    if let Some(color) = border {
        canvas.rounded_frame([2.0, 2.0, w - 4.0, h - 4.0], 10.0, 4.0, color, 1.0);
    }
    let mut x = w - MARGIN - PLATE_W;
    let y = h - MARGIN - PLATE_H;
    let mut place = |canvas: &mut Canvas, color: Color, glyph: &dyn Fn(&mut Canvas, f32, f32, Color)| {
        canvas.rounded_rect([x, y, PLATE_W, PLATE_H], 9.0, PLATE, 0.94);
        canvas.rounded_frame([x + 1.0, y + 1.0, PLATE_W - 2.0, PLATE_H - 2.0], 8.0, 2.0, color, 1.0);
        glyph(canvas, x + PLATE_W / 2.0, y + PLATE_H / 2.0, color);
        x -= PLATE_W + GAP;
    };
    if alert.source_lost {
        place(canvas, OFFLINE, &link_broken);
    }
    for device in &alert.low {
        place(canvas, LOW, &|c, cx, cy, col| device_glyph(c, *device, cx, cy, col));
    }
    for device in alert.full.iter().filter(|d| !alert.low.contains(d)) {
        place(canvas, FULL, &|c, cx, cy, col| device_glyph(c, *device, cx, cy, col));
    }
    if alert.hot {
        place(canvas, HOT, &thermometer);
    }
}

fn thermometer(c: &mut Canvas, cx: f32, cy: f32, color: Color) {
    c.line((cx, cy - 10.0), (cx, cy + 4.0), 6.0, color);
    c.circle(cx, cy + 7.0, 6.0, color, 1.0);
    c.line((cx, cy - 8.0), (cx, cy + 6.0), 2.0, PLATE);
    c.circle(cx, cy + 7.0, 3.0, PLATE, 1.0);
    c.line((cx + 6.0, cy - 8.0), (cx + 10.0, cy - 8.0), 2.0, color);
    c.line((cx + 6.0, cy - 3.0), (cx + 10.0, cy - 3.0), 2.0, color);
}

fn link_broken(c: &mut Canvas, cx: f32, cy: f32, color: Color) {
    c.arc((cx - 8.0, cy), 5.5, 2.5, 20.0, 320.0, color);
    c.arc((cx + 8.0, cy), 5.5, 2.5, 200.0, 320.0, color);
    c.line((cx - 7.0, cy + 10.0), (cx + 7.0, cy - 10.0), 2.5, color);
}

fn device_glyph(c: &mut Canvas, device: Device, cx: f32, cy: f32, color: Color) {
    match device {
        Device::Headset => {
            c.arc((cx, cy + 3.0), 10.5, 2.5, 270.0, 180.0, color);
            c.rounded_rect([cx - 13.0, cy + 1.0, 5.0, 10.0], 2.0, color, 1.0);
            c.rounded_rect([cx + 8.0, cy + 1.0, 5.0, 10.0], 2.0, color, 1.0);
        }
        Device::Mouse => {
            c.rounded_frame([cx - 7.0, cy - 11.0, 14.0, 22.0], 7.0, 2.2, color, 1.0);
            c.line((cx, cy - 7.0), (cx, cy - 2.0), 2.4, color);
        }
        Device::Keyboard => {
            c.rounded_frame([cx - 14.0, cy - 8.0, 28.0, 16.0], 3.0, 2.0, color, 1.0);
            for row in 0..2 {
                for col in 0..4 {
                    let (kx, ky) = (cx - 9.0 + col as f32 * 6.0, cy - 3.0 + row as f32 * 6.0);
                    c.rounded_rect([kx - 1.2, ky - 1.2, 2.4, 2.4], 0.5, color, 1.0);
                }
            }
        }
        Device::Gamepad => {
            c.rounded_frame([cx - 14.0, cy - 7.0, 28.0, 15.0], 7.0, 2.2, color, 1.0);
            c.line((cx - 10.0, cy), (cx - 4.0, cy), 2.0, color);
            c.line((cx - 7.0, cy - 3.0), (cx - 7.0, cy + 3.0), 2.0, color);
            c.circle(cx + 5.0, cy + 2.0, 1.6, color, 1.0);
            c.circle(cx + 9.0, cy - 2.0, 1.6, color, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(alert: &AlertView) -> Canvas {
        let mut c = Canvas::new(480, 320, Color::BLACK);
        paint(&mut c, alert);
        c
    }

    #[test]
    fn no_alert_leaves_the_image_alone() {
        let c = painted(&AlertView::default());
        assert!(c.to_frame().pixels.iter().all(|p| *p == 0));
    }

    #[test]
    fn heat_wins_the_border_colour() {
        let alert = AlertView { hot: true, low: vec![Device::Mouse], ..AlertView::default() };
        assert_eq!(painted(&alert).pixel(240, 2), [HOT.0, HOT.1, HOT.2]);
    }

    #[test]
    fn a_low_battery_marks_amber_and_a_full_one_green() {
        let low = AlertView { low: vec![Device::Mouse], ..AlertView::default() };
        assert_eq!(painted(&low).pixel(240, 2), [LOW.0, LOW.1, LOW.2]);
        let full = AlertView { full: vec![Device::Headset], ..AlertView::default() };
        assert_eq!(painted(&full).pixel(240, 2), [FULL.0, FULL.1, FULL.2]);
    }

    #[test]
    fn every_device_draws_something_in_its_plate() {
        for device in [Device::Headset, Device::Mouse, Device::Keyboard, Device::Gamepad] {
            let c = painted(&AlertView { low: vec![device], ..AlertView::default() });
            let colored = (420..468).flat_map(|x| (272..308).map(move |y| (x, y)));
            assert!(colored.filter(|(x, y)| c.pixel(*x, *y)[0] > 200).count() > 40, "{device:?}");
        }
    }

    #[test]
    fn a_device_never_shows_as_both_low_and_full() {
        let both = AlertView { low: vec![Device::Mouse], full: vec![Device::Mouse], ..AlertView::default() };
        let only = AlertView { low: vec![Device::Mouse], ..AlertView::default() };
        assert_eq!(painted(&both).to_frame(), painted(&only).to_frame());
    }
}
