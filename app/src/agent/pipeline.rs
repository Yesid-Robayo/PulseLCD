//! El ciclo de la pantalla: leer los datos, dibujar el cuadro y dárselo al guardián. Todo lo que llega a este hilo llega
//! como una orden en la cola, y eso garantiza que una imagen nunca se mezcle con otra cosa.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use pulse_panel::guardian::Target;
use pulse_panel::{Frame, Guardian, Rotation, SystemBus, SystemClock};
use pulse_render::{AlertView, History, Scene, patterns};
use pulse_sensors::Sampler;
use pulse_shared::broker;
use pulse_shared::clock::unix_secs;
use pulse_shared::config::Config;
use pulse_shared::i18n;
use pulse_shared::ipc::{Event, PanelState, Request, SensorState, Status};
use pulse_shared::log;
use pulse_shared::metrics::{Metric, Readings};
use pulse_shared::theme::Orientation;

use crate::programs;

use super::helper::Helper;
use super::server::Events;

const TICK: Duration = Duration::from_secs(1);
const ALERT_TEST: Duration = Duration::from_secs(8);
/// El reloj cambia al minuto: no hay que esperar al siguiente ciclo para que se vea.
const WAKE_GRACE: Duration = Duration::from_millis(50);
const SECONDS_PER_DAY: u64 = 86_400;
/// Ciclos hasta devolver la memoria de arranque, y luego cada cuánto repetirlo.
const FIRST_TRIM: u64 = 30;
const TRIM_EVERY: u64 = 600;

pub enum Order {
    Request(Request),
    WindowConnected,
    /// Suspender, apagar o cerrar sesión: soltar el panel y confirmar.
    Suspend(Sender<()>),
    Resume,
}

fn rotation(orientation: Orientation, flipped: bool) -> Rotation {
    match (orientation, flipped) {
        (Orientation::Landscape, false) => Rotation::Landscape,
        (Orientation::Landscape, true) => Rotation::LandscapeFlipped,
        (Orientation::Portrait, false) => Rotation::Portrait,
        (Orientation::Portrait, true) => Rotation::PortraitFlipped,
    }
}

fn target(config: &Config, scene: &Scene) -> Target {
    Target {
        port: config.panel.port.clone(),
        brightness: config.panel.brightness,
        rotation: rotation(scene.orientation(), config.panel.flipped),
    }
}

/// El tema pedido o, si no se puede abrir, el predeterminado: la pantalla no se queda en negro por un archivo roto.
fn open_scene(id: &str) -> Scene {
    crate::themes::scene(id)
        .inspect_err(|e| log::warn(&format!("el tema {id} no se pudo cargar: {e}")))
        .or_else(|_| crate::themes::scene("pulse"))
        .expect("el tema incluido siempre carga")
}

/// Devuelve a Windows lo que no se toca en cada ciclo: lo que las bibliotecas usaron al cargarse no vuelve a hacer falta.
fn release_idle_memory() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
}

/// Nombre del programa en primer plano, en minúsculas.
fn foreground_program() -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    programs::exe_name(pid).unwrap_or_default()
}

struct Pipeline {
    config: Config,
    guardian: Guardian<SystemBus, SystemClock>,
    scene: Scene,
    sampler: Sampler,
    helper: Helper,
    events: Events,
    readings: Readings,
    history: History,
    ignored: HashSet<String>,
    foreground: String,
    alert_test_until: Option<Instant>,
    sleeping: bool,
    published_state: Option<PanelState>,
    last_published: Option<Frame>,
    frame: Option<Frame>,
    cpu_state: SensorState,
    fps_state: SensorState,
    ticks: u64,
}

fn ignored_set(config: &Config) -> HashSet<String> {
    programs::NOT_GAMES
        .iter()
        .map(|s| s.to_string())
        .chain(config.fps.ignored.iter().map(|s| s.trim().to_lowercase()))
        .collect()
}

/// Sin un programa conocido al frente, como con el PC bloqueado, no hay nada que medir.
fn worth_measuring(foreground: &str, ignored: &HashSet<String>) -> bool {
    !foreground.is_empty() && !ignored.contains(foreground)
}

impl Pipeline {
    fn new(config: Config, events: Events, cold_start: bool) -> Self {
        let scene = open_scene(&config.panel.theme);
        let guardian = Guardian::new(SystemBus, SystemClock::default(), target(&config, &scene), cold_start);
        Self {
            ignored: ignored_set(&config),
            config,
            guardian,
            scene,
            sampler: Sampler::default(),
            helper: Helper::start(),
            events,
            readings: Readings::default(),
            history: History::default(),
            foreground: String::new(),
            alert_test_until: None,
            sleeping: false,
            published_state: None,
            last_published: None,
            frame: None,
            cpu_state: SensorState::Unknown,
            fps_state: SensorState::Unknown,
            ticks: 0,
        }
    }

    /// Métricas que hay que leer: las del tema, las de las alertas y, con la ventana abierta, todas.
    fn wanted(&self) -> Vec<Metric> {
        if self.events.window_open() {
            return Metric::ALL.to_vec();
        }
        let mut wanted = self.scene.used_metrics();
        if self.config.alerts.cpu_temp > 0 {
            wanted.push(Metric::CpuTemp);
        }
        if self.config.alerts.gpu_temp > 0 {
            wanted.push(Metric::GpuTemp);
        }
        wanted
    }

    fn read_sensors(&mut self) {
        let wanted = self.wanted();
        self.sampler.sample(&wanted, &self.config, &mut self.readings);
        let (temp, cpu_state) = self.helper.cpu_temp();
        self.readings.set_number(Metric::CpuTemp, temp);
        self.cpu_state = cpu_state;
        self.foreground = foreground_program();
        let measure = wanted.contains(&Metric::Fps) && worth_measuring(&self.foreground, &self.ignored);
        self.helper.request(match (measure, self.config.fps.count_generated) {
            (false, _) => broker::WANT_NOTHING,
            (true, true) => broker::WANT_FPS,
            (true, false) => broker::WANT_FPS_REAL_ONLY,
        });
        let (fps, fps_state) = self.helper.fps();
        self.readings.set_number(Metric::Fps, fps.filter(|_| measure));
        self.fps_state = fps_state;
        self.history.push(&self.scene.graphed_metrics(), &self.readings);
    }

    fn alert(&mut self) -> AlertView {
        if self.alert_test_until.is_some_and(|t| t > Instant::now()) {
            return AlertView::demo();
        }
        let over =
            |metric, limit: u8| limit > 0 && self.readings.get(metric).number().is_some_and(|v| v >= limit as f64);
        let mut alert = AlertView {
            hot: over(Metric::CpuTemp, self.config.alerts.cpu_temp)
                || over(Metric::GpuTemp, self.config.alerts.gpu_temp),
            ..AlertView::default()
        };
        match self.sampler.batteries(&self.config) {
            Some(Some(state)) => {
                alert.low = state.low;
                alert.full = state.full;
            }
            Some(None) => alert.source_lost = true,
            None => {}
        }
        alert
    }

    fn panel_state(&self) -> PanelState {
        if self.sleeping { PanelState::Off } else { self.guardian.state() }
    }

    fn tick(&mut self) {
        // la pantalla apagada y la ventana cerrada: nadie vería nada, así que no se lee ni se dibuja
        if self.panel_state() == PanelState::Off && !self.events.window_open() {
            self.helper.request(broker::WANT_NOTHING);
            self.publish();
            return;
        }
        self.read_sensors();
        let alert = self.alert();
        let frame = self.scene.frame(&self.readings, &self.history, &alert);
        if !self.sleeping {
            self.guardian.show(&frame);
        }
        self.frame = Some(frame);
        self.publish();
        self.maybe_deghost();
        self.ticks += 1;
        if self.ticks == FIRST_TRIM || self.ticks.is_multiple_of(TRIM_EVERY) {
            release_idle_memory();
        }
    }

    fn status(&self) -> Status {
        Status {
            state: self.panel_state(),
            port: self.guardian.port().map(str::to_string),
            readings: self.readings.clone(),
            cpu_temp: self.cpu_state,
            gpu: self.sampler.gpu_state(),
            fps: self.fps_state,
            foreground: self.foreground.clone(),
            reconnects: self.guardian.reconnects,
            healed: self.guardian.healed,
            frame_ms: self.guardian.frame_ms,
            missing_secs: self.guardian.missing_secs(),
        }
    }

    fn publish(&mut self) {
        let status = self.status();
        if self.published_state != Some(status.state) {
            self.published_state = Some(status.state);
            log::info(&format!("panel: {:?}", status.state));
            super::notify_state(status.state);
        }
        if !self.events.window_open() {
            self.last_published = None;
            return;
        }
        self.events.send(Event::Status(Box::new(status)));
        let shown = self.guardian.shown().or(self.frame.as_ref());
        if let Some(frame) = shown
            && self.last_published.as_ref() != Some(frame)
        {
            let pixels = frame.bytes_of(frame.full());
            self.events.send(Event::Frame { width: frame.width, height: frame.height, pixels });
            self.last_published = Some(frame.clone());
        }
    }

    fn apply_config(&mut self, mut config: Config) {
        config.sanitize();
        // la fecha de limpieza la lleva el agente: la ventana podría tener una copia vieja
        config.maintenance.last_deghost = self.config.maintenance.last_deghost;
        let theme_changed = config.panel.theme != self.config.panel.theme;
        if let Err(e) = config.save() {
            log::error(&format!("no se pudo guardar la configuración: {e}"));
        }
        if config.start_with_windows != self.config.start_with_windows {
            crate::autostart::apply(config.start_with_windows);
        }
        i18n::set(config.language);
        self.ignored = ignored_set(&config);
        self.config = config;
        if theme_changed {
            self.reload_theme();
        }
        self.guardian.retarget(target(&self.config, &self.scene));
        self.events.send(Event::Config(Box::new(self.config.clone())));
    }

    fn reload_theme(&mut self) {
        self.scene = open_scene(&self.config.panel.theme);
        self.history = History::default();
        self.guardian.retarget(target(&self.config, &self.scene));
    }

    fn deghost(&mut self) {
        log::info("limpieza de pantalla");
        let (width, height) = self.scene.orientation().size();
        for frame in patterns::deghost_sequence(width, height) {
            self.guardian.show_full(&frame);
        }
        self.config.maintenance.last_deghost = unix_secs();
        let _ = self.config.save();
        self.guardian.repair();
    }

    fn maybe_deghost(&mut self) {
        let days = self.config.maintenance.deghost_days as u64;
        if days == 0 || self.guardian.state() != PanelState::Healthy {
            return;
        }
        let last = self.config.maintenance.last_deghost;
        if last == 0 {
            // el contador empieza hoy, no con una limpieza nada más instalar
            self.config.maintenance.last_deghost = unix_secs();
            let _ = self.config.save();
        } else if unix_secs().saturating_sub(last) >= days * SECONDS_PER_DAY {
            self.deghost();
        }
    }

    /// `false` si hay que terminar.
    fn handle(&mut self, order: Order) -> bool {
        match order {
            Order::Request(Request::SetConfig(config)) => self.apply_config(*config),
            Order::Request(Request::ReloadTheme) => self.reload_theme(),
            Order::Request(Request::SetScreen(on)) => self.guardian.set_screen(on),
            Order::Request(Request::Repair) => self.guardian.repair(),
            Order::Request(Request::HardReset) => self.guardian.hard_reset(),
            Order::Request(Request::Deghost) => self.deghost(),
            Order::Request(Request::TestAlert) => self.alert_test_until = Some(Instant::now() + ALERT_TEST),
            Order::Request(Request::Quit) => {
                self.guardian.release(self.config.panel.off_when_idle);
                return false;
            }
            Order::WindowConnected => {
                self.last_published = None;
                self.events.send(Event::Config(Box::new(self.config.clone())));
            }
            Order::Suspend(done) => {
                self.sleeping = true;
                self.guardian.release(self.config.panel.off_when_idle);
                let _ = done.send(());
            }
            // Windows avisa dos veces al despertar: la segunda no debe tirar una conexión ya sana
            Order::Resume if self.sleeping => {
                self.sleeping = false;
                self.guardian.resume();
            }
            Order::Resume => {}
        }
        true
    }
}

pub fn run(config: Config, inbox: Receiver<Order>, events: Events, cold_start: bool) {
    let mut pipeline = Pipeline::new(config, events, cold_start);
    let mut next = Instant::now();
    loop {
        let wait = next.saturating_duration_since(Instant::now()).max(WAKE_GRACE);
        match inbox.recv_timeout(wait) {
            Ok(order) => {
                if !pipeline.handle(order) {
                    return;
                }
                // lo que el usuario acaba de pedir se ve enseguida
                next = next.min(Instant::now() + WAKE_GRACE);
            }
            Err(RecvTimeoutError::Timeout) => {
                pipeline.tick();
                next = Instant::now() + TICK;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_are_not_measured_without_a_known_program_in_front() {
        let ignored = ignored_set(&Config::default());
        assert!(!worth_measuring("", &ignored));
        assert!(!worth_measuring("lockapp.exe", &ignored));
        assert!(!worth_measuring("pulselcd.exe", &ignored));
        assert!(worth_measuring("hollow_knight.exe", &ignored));
    }
}
