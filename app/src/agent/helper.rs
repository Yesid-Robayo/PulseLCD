//! Conexión con el ayudante de sensores. Si no está instalado o se cierra, la app sigue funcionando sin temperatura de
//! CPU ni FPS, y vuelve a conectar sola.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pulse_shared::broker::{self, CPU_NEEDS_DRIVER, CPU_OK, FPS_OK, Sample};
use pulse_shared::ipc::SensorState;

const RETRY: Duration = Duration::from_secs(5);
/// Una lectura más vieja que esto ya no representa nada.
const STALE: Duration = Duration::from_secs(4);

#[derive(Default)]
struct Latest {
    sample: Option<(Sample, Instant)>,
}

pub struct Helper {
    latest: Arc<Mutex<Latest>>,
    want: Arc<AtomicU8>,
}

fn serve(mut pipe: File, latest: &Mutex<Latest>, want: &AtomicU8) {
    let mut sent = None;
    loop {
        let order = want.load(Ordering::Relaxed);
        if sent != Some(order) {
            if pipe.write_all(&[order]).is_err() {
                return;
            }
            sent = Some(order);
        }
        match Sample::read(&mut pipe) {
            Ok(sample) => latest.lock().unwrap_or_else(|e| e.into_inner()).sample = Some((sample, Instant::now())),
            Err(_) => return,
        }
    }
}

impl Helper {
    pub fn start() -> Self {
        let latest = Arc::new(Mutex::new(Latest::default()));
        let want = Arc::new(AtomicU8::new(broker::WANT_NOTHING));
        let (slot, order) = (latest.clone(), want.clone());
        std::thread::spawn(move || {
            loop {
                if let Ok(pipe) = OpenOptions::new().read(true).write(true).open(broker::PIPE_NAME) {
                    serve(pipe, &slot, &order);
                }
                std::thread::sleep(RETRY);
            }
        });
        Self { latest, want }
    }

    pub fn request(&self, order: u8) {
        self.want.store(order, Ordering::Relaxed);
    }

    fn sample(&self) -> Option<Sample> {
        let latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        latest.sample.filter(|(_, at)| at.elapsed() < STALE).map(|(s, _)| s)
    }

    /// Temperatura del CPU y su estado.
    pub fn cpu_temp(&self) -> (Option<f64>, SensorState) {
        let Some(sample) = self.sample() else { return (None, SensorState::NeedsHelper) };
        let state = match sample.cpu_status {
            CPU_OK => SensorState::Ok,
            CPU_NEEDS_DRIVER => SensorState::NeedsDriver,
            _ => SensorState::Unavailable,
        };
        (sample.cpu_temp.is_finite().then_some(sample.cpu_temp as f64), state)
    }

    /// FPS del proceso en primer plano y su estado.
    pub fn fps(&self) -> (Option<f64>, SensorState) {
        let Some(sample) = self.sample() else { return (None, SensorState::NeedsHelper) };
        if sample.fps_status != FPS_OK {
            return (None, SensorState::Unavailable);
        }
        ((sample.fps > 0.0).then_some(sample.fps as f64), SensorState::Ok)
    }
}
