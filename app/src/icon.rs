//! Iconos dibujados por código: un latido sobre una insignia verde azulada. Se usa también desde `build.rs`, por eso no
//! depende de nada del resto del proyecto.

/// Puntos del latido, en coordenadas de 0 a 1.
const BEAT: [(f32, f32); 7] =
    [(0.16, 0.54), (0.36, 0.54), (0.44, 0.30), (0.54, 0.74), (0.62, 0.46), (0.68, 0.54), (0.84, 0.54)];

const BRAND_A: [f32; 3] = [0x0b as f32, 0x6e as f32, 0x63 as f32];
const BRAND_B: [f32; 3] = [0x16 as f32, 0xb2 as f32, 0x8f as f32];

pub struct Icon {
    pub size: u32,
    pub rgba: Vec<u8>,
}

fn segment_distance(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    (x - a.0 - dx * t).hypot(y - a.1 - dy * t)
}

fn beat_distance(x: f32, y: f32, scale: f32) -> f32 {
    let at = |p: (f32, f32)| (0.5 + (p.0 - 0.5) * scale, 0.5 + (p.1 - 0.5) * scale);
    BEAT.windows(2).map(|s| segment_distance(x, y, at(s[0]), at(s[1]))).fold(f32::MAX, f32::min)
}

/// Cubre de 0 a 1, con el borde suavizado un píxel.
fn cover(distance: f32, pixel: f32) -> f32 {
    (0.5 - distance / pixel).clamp(0.0, 1.0)
}

fn rounded_box(x: f32, y: f32, inset: f32, radius: f32) -> f32 {
    let half = 0.5 - inset - radius;
    let (dx, dy) = ((x - 0.5).abs() - half, (y - 0.5).abs() - half);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - radius
}

/// Icono de la app: insignia con degradado y el latido en blanco.
pub fn app_icon(size: u32) -> Icon {
    let pixel = 1.0 / size as f32;
    let width = (0.042f32).max(pixel * 0.9);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = ((px as f32 + 0.5) * pixel, (py as f32 + 0.5) * pixel);
            let badge = cover(rounded_box(x, y, 0.03, 0.22), pixel);
            let line = cover(beat_distance(x, y, 1.0) - width, pixel);
            let t = (x + y) / 2.0;
            let mut color = [0.0f32; 3];
            for (i, c) in color.iter_mut().enumerate() {
                let brand = BRAND_A[i] + (BRAND_B[i] - BRAND_A[i]) * t;
                *c = brand + (255.0 - brand) * line;
            }
            rgba.extend([color[0] as u8, color[1] as u8, color[2] as u8, (badge * 255.0) as u8]);
        }
    }
    Icon { size, rgba }
}

/// Icono de la bandeja: solo el latido, y un punto si el panel necesita atención.
pub fn tray_icon(size: u32, color: [u8; 3], alert: Option<[u8; 3]>) -> Icon {
    let pixel = 1.0 / size as f32;
    let width = (0.06f32).max(pixel * 0.75);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = ((px as f32 + 0.5) * pixel, (py as f32 + 0.5) * pixel);
            let line = cover(beat_distance(x, y, 1.3) - width, pixel);
            let dot = alert.map_or(0.0, |_| cover((x - 0.78).hypot(y - 0.78) - 0.2, pixel));
            let c = if dot > 0.0 { alert.unwrap_or(color) } else { color };
            rgba.extend([c[0], c[1], c[2], (line.max(dot) * 255.0) as u8]);
        }
    }
    Icon { size, rgba }
}

/// Archivo .ico con varias resoluciones, en entradas BMP de 32 bits. Lo usa `build.rs`.
#[allow(dead_code)]
pub fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|s| bmp_entry(&app_icon(*s))).collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, image) in sizes.iter().zip(&images) {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((image.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    out.extend(images.into_iter().flatten());
    out
}

#[allow(dead_code)]
fn bmp_entry(icon: &Icon) -> Vec<u8> {
    let s = icon.size;
    let mut out = Vec::new();
    out.extend(40u32.to_le_bytes());
    out.extend((s as i32).to_le_bytes());
    out.extend((s as i32 * 2).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..s).rev() {
        for x in 0..s {
            let i = ((y * s + x) * 4) as usize;
            out.extend([icon.rgba[i + 2], icon.rgba[i + 1], icon.rgba[i], icon.rgba[i + 3]]);
        }
    }
    out.extend(vec![0u8; (s.div_ceil(32) * 4 * s) as usize]);
    out
}
