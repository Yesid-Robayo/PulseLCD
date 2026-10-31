//! Reúne las lecturas de un ciclo. Lo lento va en su propio hilo y solo existe mientras alguien usa ese dato.

use std::time::{Duration, Instant};

use pulse_shared::clock;
use pulse_shared::config::{AlertConfig, Config, WeatherConfig};
use pulse_shared::ipc::SensorState;
use pulse_shared::metrics::{Metric, Readings, Value};

use crate::battery::{self, BatteryState};
use crate::gpu::{GpuReading, Nvml};
use crate::poller::Poller;
use crate::system::{self, CpuLoad, NetMeter};
use crate::weather::{self, Weather};

const PING_EVERY: Duration = Duration::from_secs(5);
const BATTERY_EVERY: Duration = Duration::from_secs(30);
const WEATHER_EVERY: Duration = Duration::from_secs(20 * 60);
const WEATHER_ATTEMPTS: u32 = 3;
const WEATHER_RETRY: Duration = Duration::from_secs(30);
const GPU_RETRY: Duration = Duration::from_secs(60);
/// El espacio en disco casi no cambia.
const DISK_EVERY: Duration = Duration::from_secs(60);

/// Un fallo de red, como un Wi-Fi que aún no conecta al arrancar, no debe dejar el dato vacío hasta el siguiente ciclo.
fn with_retries<T>(attempts: u32, wait: Duration, mut fetch: impl FnMut() -> Option<T>) -> Option<T> {
    (0..attempts).find_map(|attempt| {
        if attempt > 0 {
            std::thread::sleep(wait);
        }
        fetch()
    })
}

fn uses(wanted: &[Metric], any: &[Metric]) -> bool {
    any.iter().any(|m| wanted.contains(m))
}

const GPU_METRICS: [Metric; 6] = [
    Metric::GpuUsage,
    Metric::GpuTemp,
    Metric::GpuMemoryUsed,
    Metric::GpuMemoryPercent,
    Metric::GpuFan,
    Metric::GpuClock,
];
const NET_METRICS: [Metric; 4] = [Metric::NetDown, Metric::NetUp, Metric::NetDownTotal, Metric::NetUpTotal];
const DISK_METRICS: [Metric; 3] = [Metric::DiskPercent, Metric::DiskUsed, Metric::DiskFree];
const RAM_METRICS: [Metric; 3] = [Metric::RamPercent, Metric::RamUsed, Metric::SwapPercent];
const WEATHER_METRICS: [Metric; 4] =
    [Metric::WeatherTemp, Metric::WeatherFeels, Metric::WeatherHumidity, Metric::WeatherText];

#[derive(Default)]
pub struct Sampler {
    cpu: CpuLoad,
    nvml: Option<Nvml>,
    gpu_checked: Option<Instant>,
    net: NetMeter,
    disk: Option<(Instant, system::Disk)>,
    ping: Option<(String, Poller<f64>)>,
    weather: Option<(WeatherConfig, Poller<Weather>)>,
    battery: Option<(AlertConfig, Poller<BatteryState>)>,
}

impl Sampler {
    /// Lee lo pedido. `wanted` son las métricas que alguien va a mostrar.
    pub fn sample(&mut self, wanted: &[Metric], config: &Config, into: &mut Readings) {
        if wanted.contains(&Metric::CpuUsage) {
            into.set_number(Metric::CpuUsage, self.cpu.sample());
        }
        if uses(wanted, &GPU_METRICS) {
            self.read_gpu(into);
        }
        if uses(wanted, &RAM_METRICS) {
            let memory = system::memory();
            into.set_number(Metric::RamPercent, memory.as_ref().map(|m| m.percent));
            into.set_number(Metric::RamUsed, memory.as_ref().map(|m| m.used_gb));
            into.set_number(Metric::SwapPercent, memory.as_ref().map(|m| m.swap_percent));
        }
        if uses(wanted, &DISK_METRICS) {
            self.read_disk(into);
        }
        if uses(wanted, &NET_METRICS) {
            let traffic = self.net.sample(&config.network);
            into.set_number(Metric::NetDown, traffic.as_ref().map(|t| t.down_rate));
            into.set_number(Metric::NetUp, traffic.as_ref().map(|t| t.up_rate));
            into.set_number(Metric::NetDownTotal, traffic.as_ref().map(|t| t.down_total));
            into.set_number(Metric::NetUpTotal, traffic.as_ref().map(|t| t.up_total));
        }
        if wanted.contains(&Metric::Uptime) {
            into.set(Metric::Uptime, Value::Number(system::uptime_secs() as f64));
        }
        if wanted.contains(&Metric::Ping) {
            self.read_ping(config, into);
        } else {
            self.ping = None;
        }
        if uses(wanted, &WEATHER_METRICS) {
            self.read_weather(config, into);
        } else {
            self.weather = None;
        }
        let now = clock::now();
        into.set(Metric::Time, Value::Text(now.time_text(config.clock_24h)));
        into.set(Metric::Date, Value::Text(now.date_text()));
        into.set(Metric::Weekday, Value::Text(now.weekday_text().into()));
    }

    fn read_gpu(&mut self, into: &mut Readings) {
        let reading = |nvml: &Option<Nvml>| nvml.as_ref().map(Nvml::read).unwrap_or_default();
        let mut r = reading(&self.nvml);
        // sin datos puede ser que el driver se actualizó: esa sesión ya no vuelve sola, y sin esto la alerta callaría
        if r == GpuReading::default() && self.gpu_checked.is_none_or(|at| at.elapsed() >= GPU_RETRY) {
            self.gpu_checked = Some(Instant::now());
            self.nvml = None;
            self.nvml = Nvml::open();
            r = reading(&self.nvml);
        }
        into.set_number(Metric::GpuUsage, r.usage);
        into.set_number(Metric::GpuTemp, r.temp);
        into.set_number(Metric::GpuMemoryUsed, r.memory_used_mb);
        into.set_number(Metric::GpuMemoryPercent, r.memory_percent);
        into.set_number(Metric::GpuFan, r.fan);
        into.set_number(Metric::GpuClock, r.clock_ghz);
    }

    pub fn gpu_state(&self) -> SensorState {
        match (self.gpu_checked.is_some(), self.nvml.is_some()) {
            (false, _) => SensorState::Unknown,
            (true, true) => SensorState::Ok,
            (true, false) => SensorState::Unavailable,
        }
    }

    fn read_disk(&mut self, into: &mut Readings) {
        let stale = self.disk.as_ref().is_none_or(|(at, _)| at.elapsed() >= DISK_EVERY);
        if stale {
            self.disk = system::disk().map(|d| (Instant::now(), d));
        }
        let disk = self.disk.as_ref().map(|(_, d)| d);
        into.set_number(Metric::DiskPercent, disk.map(|d| d.percent));
        into.set_number(Metric::DiskUsed, disk.map(|d| d.used_gb));
        into.set_number(Metric::DiskFree, disk.map(|d| d.free_gb));
    }

    fn read_ping(&mut self, config: &Config, into: &mut Readings) {
        let host = config.ping_host.trim();
        if host.is_empty() {
            self.ping = None;
            into.set(Metric::Ping, Value::Missing);
            return;
        }
        if self.ping.as_ref().is_none_or(|(h, _)| h != host) {
            let target = host.to_string();
            self.ping = Some((host.to_string(), Poller::spawn(PING_EVERY, move || system::ping(&target))));
        }
        into.set_number(Metric::Ping, self.ping.as_ref().and_then(|(_, p)| p.latest()));
    }

    fn read_weather(&mut self, config: &Config, into: &mut Readings) {
        if !config.weather.is_ready() {
            self.weather = None;
        } else if self.weather.as_ref().is_none_or(|(c, _)| *c != config.weather) {
            let target = config.weather.clone();
            let mut last = None;
            let poller = Poller::spawn(WEATHER_EVERY, move || {
                let fresh = with_retries(WEATHER_ATTEMPTS, WEATHER_RETRY, || weather::fetch(&target));
                last = fresh.or(last.take());
                last.clone()
            });
            self.weather = Some((config.weather.clone(), poller));
        }
        let now = self.weather.as_ref().and_then(|(_, p)| p.latest());
        into.set_number(Metric::WeatherTemp, now.as_ref().map(|w| w.temp));
        into.set_number(Metric::WeatherFeels, now.as_ref().map(|w| w.feels));
        into.set_number(Metric::WeatherHumidity, now.as_ref().map(|w| w.humidity));
        into.set(Metric::WeatherText, now.map_or(Value::Missing, |w| Value::Text(w.text)));
    }

    /// Estado de las baterías. `None` dentro de `Some` significa que la fuente dejó de responder.
    pub fn batteries(&mut self, config: &Config) -> Option<Option<BatteryState>> {
        if !config.alerts.battery {
            self.battery = None;
            return None;
        }
        if self.battery.as_ref().is_none_or(|(c, _)| *c != config.alerts) {
            let url = config.alerts.battery_url.clone();
            self.battery = Some((config.alerts.clone(), Poller::spawn(BATTERY_EVERY, move || battery::fetch(&url))));
        }
        self.battery.as_ref().map(|(_, p)| p.latest())
    }
}

/// Interfaces de red activas, para los ajustes.
pub fn network_interfaces() -> Vec<String> {
    system::interface_names()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_fetch_is_tried_again_until_it_works() {
        let mut calls = 0;
        let found = with_retries(3, Duration::ZERO, || {
            calls += 1;
            (calls == 2).then_some(7)
        });
        assert_eq!((found, calls), (Some(7), 2));
    }

    #[test]
    fn retries_stop_at_the_limit() {
        let mut calls = 0;
        let found = with_retries(3, Duration::ZERO, || -> Option<u8> {
            calls += 1;
            None
        });
        assert_eq!((found, calls), (None, 3));
    }

    #[test]
    fn only_what_is_wanted_gets_read() {
        let mut sampler = Sampler::default();
        let mut r = Readings::default();
        sampler.sample(&[Metric::RamPercent], &Config::default(), &mut r);
        assert!(r.get(Metric::RamPercent).number().is_some());
        assert!(matches!(r.get(Metric::GpuUsage), Value::Missing));
        assert!(sampler.gpu_checked.is_none(), "la GPU no se toca si nadie la muestra");
    }

    #[test]
    fn the_clock_is_always_there() {
        let mut r = Readings::default();
        Sampler::default().sample(&[], &Config::default(), &mut r);
        assert!(matches!(r.get(Metric::Time), Value::Text(_)));
    }

    #[test]
    fn dropping_a_metric_stops_its_background_work() {
        let mut sampler = Sampler::default();
        let mut r = Readings::default();
        let cfg = Config { ping_host: "127.0.0.1".into(), ..Config::default() };
        sampler.sample(&[Metric::Ping], &cfg, &mut r);
        assert!(sampler.ping.is_some());
        sampler.sample(&[], &cfg, &mut r);
        assert!(sampler.ping.is_none());
    }

    #[test]
    fn battery_reading_is_off_by_default() {
        assert_eq!(Sampler::default().batteries(&Config::default()), None);
    }
}
