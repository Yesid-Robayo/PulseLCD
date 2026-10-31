//! Guarda en PNG cómo se ve cada tema incluido, con datos de ejemplo. `snapshot <carpeta> [escala] [carpeta de un tema]`.

use pulse_render::{AlertView, BUILTIN, History, Scene, builtin_theme};
use pulse_shared::metrics::{Metric, Readings};

fn main() {
    let folder = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let scale: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    if let Some(theme) = std::env::args().nth(3) {
        let mut scene = Scene::open(std::path::Path::new(&theme)).expect("tema");
        let canvas = scene.paint(&Readings::samples(), &History::default(), &AlertView::default());
        save(&canvas, &format!("{folder}/custom.png"), scale);
        return;
    }
    for id in BUILTIN {
        let mut scene = Scene::new(builtin_theme(id).expect("tema"), None).expect("escena");
        let readings = Readings::samples();
        let mut history = History::default();
        let mut wave = readings.clone();
        for i in 0..150 {
            let t = i as f64 / 9.0;
            wave.set_number(Metric::CpuUsage, Some(42.0 + 28.0 * t.sin() + 11.0 * (t * 2.7).cos()));
            wave.set_number(Metric::Fps, Some(138.0 + 22.0 * (t * 0.8).sin() + 9.0 * (t * 3.1).cos()));
            history.push(&scene.graphed_metrics(), &wave);
        }
        let canvas = scene.paint(&readings, &history, &AlertView::default());
        save(&canvas, &format!("{folder}/{id}.png"), scale);
        if id == "orbit" {
            let canvas = scene.paint(&readings, &history, &AlertView::demo());
            save(&canvas, &format!("{folder}/{id}-alerts.png"), scale);
        }
    }
}

fn save(canvas: &pulse_render::Canvas, path: &str, scale: u32) {
    let (w, h) = (canvas.width as u32, canvas.height as u32);
    let image = image::RgbaImage::from_raw(w, h, canvas.to_rgba()).expect("imagen");
    image::imageops::resize(&image, w * scale, h * scale, image::imageops::FilterType::Nearest)
        .save(path)
        .expect("guardar");
}
