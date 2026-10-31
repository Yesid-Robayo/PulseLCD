//! Banco de pruebas sobre el panel real. Solo se usa en desarrollo y con el agente cerrado.
//!
//! `cargo run -p pulse-panel --release --example lab -- <ciclos>` corta una imagen a propósito, recupera el panel con el
//! guardián y comprueba que el USB no rebote. Al final mide varios cuadros para ver si el tiempo sirve de señal de salud.

use std::time::{Duration, Instant};

use pulse_panel::guardian::Target;
use pulse_panel::protocol::{self, Frame, Rect, Rotation};
use pulse_panel::serial::Serial;
use pulse_panel::{Guardian, SystemBus, SystemClock, Wire, discover};

/// Rejilla sobre un degradado: cualquier cizalla o píxel corrido se ve a simple vista.
fn grid(tint: u16) -> Frame {
    let mut f = Frame::solid(480, 320, 0);
    for y in 0..320usize {
        for x in 0..480usize {
            let base = (((x * 31 / 480) as u16) << 11) | (((y * 63 / 320) as u16) << 5) | tint;
            let line = x % 40 == 0 || y % 40 == 0 || x == 479 || y == 319;
            f.pixels[y * 480 + x] = if line { 0xffff } else { base };
        }
    }
    f
}

fn wait(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn main() {
    pulse_shared::log::init("lab");
    let cycles: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(12);
    let port = discover::pick(&discover::ports(), "auto").expect("no se encontró el panel");
    println!("panel en {port}");
    let target = Target { port: "auto".into(), brightness: 53, rotation: Rotation::LandscapeFlipped };
    let mut bounces = 0;
    for i in 0..cycles {
        let pending = [1000usize, 1002, 1004, 1001, 1003, 1005][i % 6] + (i / 6) * 640;
        {
            let mut wire = Serial::open(&port).expect("abrir el puerto");
            let rect = Rect { x: 14, y: 160, w: 140, h: 40 };
            let total = rect.area() * 2;
            wire.write(&protocol::bitmap_header(rect)).expect("cabecera");
            for chunk in vec![0u8; total - pending].chunks(protocol::CHUNK_BYTES) {
                wire.write(chunk).expect("píxeles");
            }
        }
        wait(800);
        let frame = grid((i as u16 * 5) % 32);
        let mut guardian = Guardian::new(SystemBus, SystemClock::default(), target.clone(), false);
        let started = Instant::now();
        guardian.show(&frame);
        let healthy = guardian.state() == pulse_shared::ipc::PanelState::Healthy;
        println!(
            "ciclo {i:2} corte {pending:5}: {} en {:.2} s, cuadro {} ms",
            if healthy { "ok      " } else { "REBOTE  " },
            started.elapsed().as_secs_f32(),
            guardian.frame_ms
        );
        if !healthy {
            bounces += 1;
            wait(9000);
        }
        wait(300);
    }
    println!("rebotes: {bounces} de {cycles}");

    println!("-- tiempos de cuadro con el panel sano");
    let mut guardian = Guardian::new(SystemBus, SystemClock::default(), target, false);
    let frame = grid(4);
    guardian.show(&frame);
    let mut times = Vec::new();
    for _ in 0..8 {
        guardian.show_full(&frame);
        times.push(guardian.frame_ms);
    }
    println!("{times:?}");
    drop(guardian);
    if let Ok(mut wire) = Serial::open(&port) {
        let started = Instant::now();
        for chunk in protocol::filler().chunks(protocol::CHUNK_BYTES) {
            wire.write(chunk).expect("relleno");
        }
        println!("-- mismo volumen en modo comando: {} ms", started.elapsed().as_millis());
    }
}
