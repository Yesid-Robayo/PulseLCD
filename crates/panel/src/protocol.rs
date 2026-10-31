//! Bytes que entiende el panel de 3,5" (320×480, conectado como puerto serie USB).
//!
//! Medido en un panel real (ver `docs/panel.md`): no responde a nada, ni siquiera al saludo. Los píxeles van en RGB565
//! con el byte bajo primero y consume unos 128 kB/s, así que un cuadro completo tarda 2,4 s.

pub const CMD_RESET: u8 = 101;
pub const CMD_SCREEN_OFF: u8 = 108;
pub const CMD_SCREEN_ON: u8 = 109;
pub const CMD_BRIGHTNESS: u8 = 110;
pub const CMD_ORIENTATION: u8 = 121;
pub const CMD_BITMAP: u8 = 197;

/// Lados físicos del panel.
pub const SHORT_SIDE: u16 = 320;
pub const LONG_SIDE: u16 = 480;
pub const FRAME_BYTES: usize = SHORT_SIDE as usize * LONG_SIDE as usize * 2;

/// Mayor bloque que se entrega de una vez al puerto.
pub const CHUNK_BYTES: usize = LONG_SIDE as usize * 8;

/// Giro del panel, con el valor que espera su comando de orientación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Portrait = 0,
    PortraitFlipped = 1,
    Landscape = 2,
    LandscapeFlipped = 3,
}

impl Rotation {
    pub fn size(self) -> (u16, u16) {
        match self {
            Rotation::Portrait | Rotation::PortraitFlipped => (SHORT_SIDE, LONG_SIDE),
            Rotation::Landscape | Rotation::LandscapeFlipped => (LONG_SIDE, SHORT_SIDE),
        }
    }
}

/// Zona de la pantalla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn area(self) -> usize {
        self.w as usize * self.h as usize
    }
}

/// Una imagen lista para el panel, en RGB565.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u16>,
}

impl Frame {
    pub fn solid(width: u16, height: u16, color: u16) -> Self {
        Self { width, height, pixels: vec![color; width as usize * height as usize] }
    }

    pub fn full(&self) -> Rect {
        Rect { x: 0, y: 0, w: self.width, h: self.height }
    }

    /// Píxeles de una zona en el orden del cable.
    pub fn bytes_of(&self, rect: Rect) -> Vec<u8> {
        let mut out = Vec::with_capacity(rect.area() * 2);
        for row in rect.y..rect.y + rect.h {
            let start = row as usize * self.width as usize + rect.x as usize;
            for px in &self.pixels[start..start + rect.w as usize] {
                out.extend(px.to_le_bytes());
            }
        }
        out
    }
}

/// Comando de 6 bytes: cuatro coordenadas de 10 bits empaquetadas y el código al final.
pub fn command(code: u8, x: u16, y: u16, end_x: u16, end_y: u16) -> [u8; 6] {
    let bits =
        (x as u64 & 0x3ff) << 30 | (y as u64 & 0x3ff) << 20 | (end_x as u64 & 0x3ff) << 10 | (end_y as u64 & 0x3ff);
    let b = bits.to_be_bytes();
    [b[3], b[4], b[5], b[6], b[7], code]
}

pub fn simple(code: u8) -> [u8; 6] {
    command(code, 0, 0, 0, 0)
}

/// `percent` de 0 a 100. El panel lo entiende al revés: 0 es el máximo y 255 el mínimo.
pub fn brightness(percent: u8) -> [u8; 6] {
    let level = 255 - (percent.min(100) as u16 * 255 / 100);
    command(CMD_BRIGHTNESS, level, 0, 0, 0)
}

/// La orientación lleva 10 bytes más: el giro y el tamaño lógico.
pub fn orientation(rotation: Rotation) -> [u8; 16] {
    let (w, h) = rotation.size();
    let mut b = [0u8; 16];
    b[5] = CMD_ORIENTATION;
    b[6] = rotation as u8 + 100;
    b[7..9].copy_from_slice(&w.to_be_bytes());
    b[9..11].copy_from_slice(&h.to_be_bytes());
    b
}

pub fn bitmap_header(rect: Rect) -> [u8; 6] {
    command(CMD_BITMAP, rect.x, rect.y, rect.x + rect.w - 1, rect.y + rect.h - 1)
}

/// Ceros para terminar cualquier imagen que una sesión anterior dejara a medias. Si el panel no esperaba píxeles, los
/// ceros son comandos vacíos y los ignora.
pub fn filler() -> Vec<u8> {
    vec![0; FRAME_BYTES]
}

/// Tren de bitmaps de 1×1 en la esquina: el panel que esté alineado lo pinta y sigue alineado, y el que no lo esté cae
/// en el patrón a las pocas repeticiones, porque 6 y 8 bytes se cruzan. Sano, solo cambia un píxel que el cuadro
/// siguiente restaura. Medido en el panel real: recuperó 5 de 5 cortes sin RESET y sin que el USB rebotara.
pub fn resync_train() -> Vec<u8> {
    const REPEATS: usize = 16;
    let mut unit = [0u8; 8];
    unit[5] = CMD_BITMAP;
    unit.repeat(REPEATS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_are_packed_in_ten_bits_each() {
        assert_eq!(command(CMD_BITMAP, 0, 0, 479, 319), [0, 0, 7, 125, 63, 197]);
        assert_eq!(command(CMD_BITMAP, 14, 160, 153, 173), [3, 138, 2, 100, 173, 197]);
    }

    #[test]
    fn brightness_runs_backwards() {
        assert_eq!(brightness(100)[0], 0);
        assert_eq!(brightness(0), [63, 192, 0, 0, 0, CMD_BRIGHTNESS]);
    }

    #[test]
    fn orientation_carries_the_logical_size() {
        let b = orientation(Rotation::LandscapeFlipped);
        assert_eq!(&b[5..11], &[CMD_ORIENTATION, 103, 1, 224, 1, 64]);
        assert_eq!(b.len(), 16);
    }

    #[test]
    fn pixels_go_low_byte_first_row_by_row() {
        let frame = Frame { width: 2, height: 2, pixels: vec![0x1234, 0x5678, 0x9abc, 0xdef0] };
        assert_eq!(frame.bytes_of(Rect { x: 1, y: 0, w: 1, h: 2 }), [0x78, 0x56, 0xf0, 0xde]);
    }
}
