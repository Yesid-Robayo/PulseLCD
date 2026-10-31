//! Imágenes de la limpieza de pantalla. Un LCD que muestra lo mismo durante días retiene un poco la imagen; pasar por
//! colores sólidos y tableros invertidos ayuda a borrarla. Cada cuadro tarda 2,4 s en llegar al panel.

use pulse_panel::Frame;

const SQUARE: usize = 16;
const WHITE: u16 = 0xffff;
const BLACK: u16 = 0;
const RED: u16 = 0xf800;
const GREEN: u16 = 0x07e0;
const BLUE: u16 = 0x001f;

fn checker(width: u16, height: u16, inverted: bool) -> Frame {
    let mut frame = Frame::solid(width, height, BLACK);
    let w = width as usize;
    for (i, px) in frame.pixels.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        if (x / SQUARE + y / SQUARE).is_multiple_of(2) != inverted {
            *px = WHITE;
        }
    }
    frame
}

/// La secuencia completa de la limpieza, unos 22 s.
pub fn deghost_sequence(width: u16, height: u16) -> Vec<Frame> {
    let solid = |color| Frame::solid(width, height, color);
    vec![
        solid(WHITE),
        solid(BLACK),
        solid(RED),
        solid(GREEN),
        solid(BLUE),
        checker(width, height, false),
        checker(width, height, true),
        solid(WHITE),
        solid(BLACK),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_checkers_are_exact_opposites() {
        let (a, b) = (checker(480, 320, false), checker(480, 320, true));
        assert!(a.pixels.iter().zip(&b.pixels).all(|(x, y)| *x == !*y));
    }

    #[test]
    fn the_sequence_ends_on_black_and_fits_the_panel() {
        let frames = deghost_sequence(320, 480);
        assert!(frames.iter().all(|f| f.pixels.len() == 320 * 480));
        assert!(frames.last().unwrap().pixels.iter().all(|p| *p == 0));
    }
}
