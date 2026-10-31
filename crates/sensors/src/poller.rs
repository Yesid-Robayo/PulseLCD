//! Tareas lentas en su propio hilo: una red que tarda no debe frenar la pantalla. Quien la creó solo lee el último valor,
//! y al soltarla el hilo termina en cuanto acaba lo que estaba haciendo.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Poller<T> {
    latest: Arc<Mutex<Option<T>>>,
    _stop: Sender<()>,
}

impl<T: Clone + Send + 'static> Poller<T> {
    /// Ejecuta `job` ya mismo y luego cada `every`. Si devuelve `None`, se olvida el valor anterior.
    pub fn spawn(every: Duration, mut job: impl FnMut() -> Option<T> + Send + 'static) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let (stop, stopped) = mpsc::channel::<()>();
        let slot = latest.clone();
        std::thread::spawn(move || {
            loop {
                let value = job();
                *slot.lock().unwrap_or_else(|e| e.into_inner()) = value;
                match stopped.recv_timeout(every) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            }
        });
        Self { latest, _stop: stop }
    }

    pub fn latest(&self) -> Option<T> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn wait_for(condition: impl Fn() -> bool) {
        for _ in 0..200 {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no llegó a tiempo");
    }

    #[test]
    fn it_publishes_the_latest_value() {
        let poller = Poller::spawn(Duration::from_millis(1), || Some(7));
        wait_for(|| poller.latest() == Some(7));
    }

    #[test]
    fn dropping_it_stops_the_work() {
        let runs = Arc::new(AtomicU32::new(0));
        let counter = runs.clone();
        let poller = Poller::spawn(Duration::from_millis(1), move || {
            counter.fetch_add(1, Ordering::Relaxed);
            Some(1)
        });
        wait_for(|| runs.load(Ordering::Relaxed) > 0);
        drop(poller);
        std::thread::sleep(Duration::from_millis(30));
        let after = runs.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(runs.load(Ordering::Relaxed), after);
    }
}
