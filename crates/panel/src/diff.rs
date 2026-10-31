//! Decide qué parte de la imagen hay que enviar. El panel recibe a 128 kB/s y dibuja de arriba abajo, así que cada byte de
//! menos se nota, y un envío de pantalla completa se ve como una línea que barre la imagen.
//!
//! Fila a fila se buscan los tramos que cambiaron y cada tramo alarga el rectángulo de la fila anterior si encaja con él.
//! Así el marco de una alerta son cuatro tiras delgadas, no una pantalla entera, y el reloj y una barra en lados opuestos
//! son dos envíos pequeños.

use crate::protocol::{Frame, Rect};

/// Columnas iguales a partir de las cuales compensa partir un tramo, y anchura que un rectángulo puede ganar o sobrar para
/// seguir creciendo.
const SPLIT_GAP: usize = 24;
/// Filas iguales a partir de las cuales un rectángulo se da por terminado.
const ROW_GAP: usize = 2;
/// Con más de esta fracción cambiada se envía la pantalla entera.
const FULL_RATIO: f32 = 0.6;

/// Rectángulo que aún puede crecer hacia abajo.
struct Open {
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
}

impl Open {
    fn close(&self) -> Rect {
        Rect {
            x: self.left as u16,
            y: self.top as u16,
            w: (self.right - self.left + 1) as u16,
            h: (self.bottom - self.top + 1) as u16,
        }
    }

    /// Un tramo encaja si se solapa y juntarlos no añade mucho que no cambió. Sin esto, el lado izquierdo del marco
    /// colgaría del borde superior, que ocupa toda la anchura.
    fn fits(&self, run: (usize, usize)) -> bool {
        let overlaps = run.0 <= self.right && run.1 >= self.left;
        let width = run.1.max(self.right) - run.0.min(self.left) + 1;
        overlaps && width - (run.1 - run.0 + 1) <= SPLIT_GAP && width - (self.right - self.left + 1) <= SPLIT_GAP
    }
}

/// Tramos de una fila que cambiaron. Los que están cerca van juntos: separarlos costaría más que enviarlos.
fn changed_runs(old: &[u16], new: &[u16], runs: &mut Vec<(usize, usize)>) {
    runs.clear();
    for (x, _) in old.iter().zip(new).enumerate().filter(|(_, (a, b))| a != b) {
        match runs.last_mut() {
            Some(run) if x - run.1 <= SPLIT_GAP => run.1 = x,
            _ => runs.push((x, x)),
        }
    }
}

/// Zonas que difieren entre lo que hay en el panel y lo nuevo. Vacío si son iguales.
pub fn changed_rects(old: &Frame, new: &Frame) -> Vec<Rect> {
    if old.width != new.width || old.height != new.height {
        return vec![new.full()];
    }
    let width = new.width as usize;
    let mut open: Vec<Open> = Vec::new();
    let mut rects = Vec::new();
    let mut runs = Vec::new();
    for y in 0..new.height as usize {
        let (before, after) = (&old.pixels[y * width..(y + 1) * width], &new.pixels[y * width..(y + 1) * width]);
        if before == after {
            runs.clear();
        } else {
            changed_runs(before, after, &mut runs);
        }
        for &run in &runs {
            match open.iter_mut().find(|o| o.bottom != y && o.fits(run)) {
                Some(rect) => {
                    rect.left = rect.left.min(run.0);
                    rect.right = rect.right.max(run.1);
                    rect.bottom = y;
                }
                None => open.push(Open { left: run.0, right: run.1, top: y, bottom: y }),
            }
        }
        open.retain(|o| {
            let alive = y - o.bottom < ROW_GAP;
            if !alive {
                rects.push(o.close());
            }
            alive
        });
    }
    rects.extend(open.iter().map(Open::close));
    let changed: usize = rects.iter().map(|r| r.area()).sum();
    if changed as f32 > FULL_RATIO * new.pixels.len() as f32 {
        return vec![new.full()];
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with(points: &[(u16, u16)]) -> Frame {
        let mut f = Frame::solid(480, 320, 0);
        for (x, y) in points {
            f.pixels[*y as usize * 480 + *x as usize] = 0xffff;
        }
        f
    }

    /// Marco de `thickness` píxeles alrededor de la pantalla, como el de una alerta.
    fn framed(thickness: usize) -> Frame {
        let mut f = Frame::solid(480, 320, 0);
        for y in 0..320 {
            for x in 0..480 {
                if x < thickness || x >= 480 - thickness || y < thickness || y >= 320 - thickness {
                    f.pixels[y * 480 + x] = 0xf800;
                }
            }
        }
        f
    }

    fn covers(rects: &[Rect], x: u16, y: u16) -> bool {
        rects.iter().any(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
    }

    #[test]
    fn identical_frames_send_nothing() {
        let a = frame_with(&[(5, 5)]);
        assert!(changed_rects(&a, &a.clone()).is_empty());
    }

    #[test]
    fn one_pixel_sends_one_pixel() {
        let rects = changed_rects(&frame_with(&[]), &frame_with(&[(100, 50)]));
        assert_eq!(rects, [Rect { x: 100, y: 50, w: 1, h: 1 }]);
    }

    #[test]
    fn distant_changes_on_the_same_row_are_two_sends() {
        let rects = changed_rects(&frame_with(&[]), &frame_with(&[(10, 40), (450, 40)]));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects.iter().map(|r| r.area()).sum::<usize>(), 2);
    }

    #[test]
    fn an_alert_frame_is_four_strips_not_a_whole_screen() {
        let (old, new) = (Frame::solid(480, 320, 0), framed(4));
        let rects = changed_rects(&old, &new);
        assert_eq!(rects.len(), 4, "{rects:?}");
        let sent: usize = rects.iter().map(|r| r.area()).sum();
        let changed = new.pixels.iter().filter(|p| **p != 0).count();
        assert_eq!(sent, changed, "no se envía nada de más: {rects:?}");
    }

    #[test]
    fn taking_the_alert_frame_away_is_just_as_small() {
        let rects = changed_rects(&framed(4), &Frame::solid(480, 320, 0));
        assert_eq!(rects.len(), 4);
    }

    #[test]
    fn a_small_widget_stays_small_next_to_the_frame() {
        let (old, mut new) = (Frame::solid(480, 320, 0), framed(4));
        for y in 270..306 {
            for x in 420..462 {
                new.pixels[y * 480 + x] = 0xffff;
            }
        }
        let rects = changed_rects(&old, &new);
        let sent: usize = rects.iter().map(|r| r.area()).sum();
        assert!(sent < 480 * 320 / 8, "{rects:?}");
    }

    #[test]
    fn every_changed_pixel_is_inside_some_rect() {
        let points: Vec<(u16, u16)> = (0..400).map(|i| (i * 7 % 480, i * 13 % 320)).collect();
        let (old, new) = (frame_with(&[]), frame_with(&points));
        let rects = changed_rects(&old, &new);
        for (x, y) in points {
            assert!(covers(&rects, x, y), "({x},{y})");
        }
    }

    #[test]
    fn rects_never_leave_the_screen() {
        let points: Vec<(u16, u16)> = (0..300).map(|i| (i * 31 % 480, i * 17 % 320)).collect();
        for r in changed_rects(&frame_with(&[]), &frame_with(&points)) {
            assert!(r.w > 0 && r.h > 0 && r.x + r.w <= 480 && r.y + r.h <= 320, "{r:?}");
        }
    }

    #[test]
    fn a_widget_with_uneven_rows_is_still_fully_covered() {
        let mut new = Frame::solid(480, 320, 0);
        for y in 100..130usize {
            let reach = 30 + (y - 100) * 3;
            for x in 200..200 + reach {
                new.pixels[y * 480 + x] = 1;
            }
        }
        let rects = changed_rects(&Frame::solid(480, 320, 0), &new);
        for y in 100..130u16 {
            let reach = 30 + (y - 100) * 3;
            for x in 200..200 + reach {
                assert!(covers(&rects, x, y), "({x},{y})");
            }
        }
    }

    #[test]
    fn a_mostly_different_frame_is_sent_whole() {
        let old = Frame::solid(480, 320, 0);
        let new = Frame::solid(480, 320, 1);
        assert_eq!(changed_rects(&old, &new), [new.full()]);
    }

    #[test]
    fn a_new_size_is_sent_whole() {
        let old = Frame::solid(480, 320, 0);
        let new = Frame::solid(320, 480, 0);
        assert_eq!(changed_rects(&old, &new), [new.full()]);
    }
}
