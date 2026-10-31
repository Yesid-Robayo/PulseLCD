//! El guardián: conecta con el panel y lo mantiene sano. Es el único dueño del puerto, y por eso una imagen jamás se
//! mezcla con otra orden.
//!
//! El panel no responde a nada, así que no se puede preguntar cómo está. Las reglas salen de lo medido en un panel real
//! (`docs/panel.md`):
//!
//! 1. Antes de dibujar en una sesión nueva se resincroniza, porque la anterior pudo cortarse a mitad de una imagen. Sin
//!    esto el panel lee píxeles como comandos y acaba reiniciándose solo.
//! 2. Nunca se usa RESET por iniciativa propia: resincronizar basta, y varias veces el RESET sobre un panel en mal estado
//!    lo dejó fuera del USB hasta desconectar el cable.
//! 3. Tras conectar se repinta varias veces, y después de vez en cuando. Si todo iba bien no se nota, porque la imagen es
//!    idéntica. Si algo salió mal, se cura solo.
//! 4. El tiempo de un cuadro completo delata el estado: dibujándolo tarda 2,40 s, y si no lo estaba dibujando, 2,33 s.

use std::fmt;
use std::time::{Duration, Instant};

use pulse_shared::ipc::PanelState;
use pulse_shared::log;

use crate::diff::changed_rects;
use crate::protocol::{
    self, CHUNK_BYTES, CMD_RESET, CMD_SCREEN_OFF, CMD_SCREEN_ON, FRAME_BYTES, Frame, Rect, Rotation,
};

/// Margen desde que el puerto vuelve hasta el primer byte: el USB reaparece antes de que el firmware termine de arrancar.
const SETTLE_MS: u64 = 6000;
/// Repintados tras conectar, en segundos.
const EARLY_REPAINTS: [u64; 3] = [20, 60, 300];
/// Repintado periódico en régimen normal.
const REPAINT_EVERY_SECS: u64 = 30 * 60;
/// Por debajo de este tiempo, un cuadro completo no se estaba dibujando. Medido: 2400 ms dibujando y 2328 sin dibujar.
const MIN_FRAME_MS: u64 = 2365;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Windows no dejó abrir el puerto (código de error).
    Open(u32),
    /// Windows no pudo escribir (código de error).
    Write(u32),
    /// Salieron menos bytes de los pedidos: el panel quedó con una imagen a medias.
    Partial { written: usize, wanted: usize },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Open(code) => write!(f, "no se pudo abrir el puerto (código {code})"),
            LinkError::Write(code) => write!(f, "fallo de escritura (código {code})"),
            LinkError::Partial { written, wanted } => write!(f, "escritura parcial: {written} de {wanted} bytes"),
        }
    }
}

/// Un canal abierto hacia el panel.
pub trait Wire {
    fn write(&mut self, data: &[u8]) -> Result<(), LinkError>;
}

/// Dónde se busca y se abre el panel.
pub trait Bus {
    type Wire: Wire;
    /// El puerto a usar si está presente.
    fn find(&mut self, wanted: &str) -> Option<String>;
    fn open(&mut self, port: &str) -> Result<Self::Wire, LinkError>;
}

/// Reloj, cambiable en las pruebas.
pub trait Clock {
    fn now_ms(&self) -> u64;
}

pub struct SystemBus;

impl Bus for SystemBus {
    type Wire = crate::serial::Serial;

    fn find(&mut self, wanted: &str) -> Option<String> {
        crate::discover::pick(&crate::discover::ports(), wanted)
    }

    fn open(&mut self, port: &str) -> Result<Self::Wire, LinkError> {
        crate::serial::Serial::open(port)
    }
}

pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

/// Lo que se quiere del panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub port: String,
    pub brightness: u8,
    pub rotation: Rotation,
}

pub struct Guardian<B: Bus, C: Clock> {
    bus: B,
    clock: C,
    target: Target,
    wire: Option<B::Wire>,
    port: Option<String>,
    state: PanelState,
    /// Lo que hay ahora mismo en el panel.
    shown: Option<Frame>,
    /// El usuario pidió la pantalla apagada.
    screen_off: bool,
    /// Hay que esperar a que el puerto se asiente antes de abrirlo.
    must_settle: bool,
    seen_at: Option<u64>,
    missing_since: Option<u64>,
    connected_at: u64,
    repaints_done: usize,
    next_periodic: u64,
    repair_now: bool,
    /// Ya hubo una conexión: la siguiente cuenta como reconexión.
    connected_before: bool,
    /// Ya se registró que no se puede abrir el puerto: no repetirlo cada segundo.
    open_failure_logged: bool,
    pub reconnects: u32,
    pub healed: u32,
    pub frame_ms: u32,
}

impl<B: Bus, C: Clock> Guardian<B, C> {
    /// `cold_start`: el agente arrancó con Windows y el USB puede estar levantándose.
    pub fn new(bus: B, clock: C, target: Target, cold_start: bool) -> Self {
        Self {
            bus,
            clock,
            target,
            wire: None,
            port: None,
            state: PanelState::Searching,
            shown: None,
            screen_off: false,
            must_settle: cold_start,
            seen_at: None,
            missing_since: None,
            connected_at: 0,
            repaints_done: 0,
            next_periodic: 0,
            repair_now: false,
            connected_before: false,
            open_failure_logged: false,
            reconnects: 0,
            healed: 0,
            frame_ms: 0,
        }
    }

    /// Si el usuario la apagó está apagada, conectada o no: es lo que tiene que decir la ventana y lo que permite encenderla.
    pub fn state(&self) -> PanelState {
        if self.screen_off { PanelState::Off } else { self.state }
    }

    pub fn port(&self) -> Option<&str> {
        self.port.as_deref()
    }

    /// Lo que hay ahora mismo en la pantalla.
    pub fn shown(&self) -> Option<&Frame> {
        self.shown.as_ref()
    }

    pub fn missing_secs(&self) -> u32 {
        if self.screen_off {
            return 0;
        }
        self.missing_since.map_or(0, |t| ((self.clock.now_ms() - t) / 1000) as u32)
    }

    /// Cambia el puerto, el brillo o el giro. Lo que no exige reconectar se aplica al momento.
    pub fn retarget(&mut self, target: Target) {
        if target == self.target {
            return;
        }
        let old = std::mem::replace(&mut self.target, target);
        if old.port != self.target.port {
            log::info(&format!("cambio de puerto a {}", self.target.port));
            self.wire = None;
            self.port = None;
            self.state = PanelState::Searching;
            return;
        }
        let mut result = Ok(());
        if old.brightness != self.target.brightness {
            result = self.send(&protocol::brightness(self.target.brightness));
        }
        if old.rotation != self.target.rotation {
            self.shown = None;
            let rotation = protocol::orientation(self.target.rotation);
            result = result.and_then(|()| self.send(&rotation));
        }
        if let Err(e) = result {
            self.lost(&e);
        }
    }

    /// Muestra la imagen. Se llama en cada ciclo, y aquí pasa todo: conectar, repintar y enviar los cambios.
    pub fn show(&mut self, frame: &Frame) {
        if self.screen_off {
            return;
        }
        if self.wire.is_none() && !self.connect(frame) {
            return;
        }
        if let Err(e) = self.update(frame) {
            self.lost(&e);
        }
    }

    /// Pinta la imagen completa de inmediato (limpieza de pantalla).
    pub fn show_full(&mut self, frame: &Frame) {
        if self.screen_off {
            return;
        }
        if self.wire.is_none() && !self.connect(frame) {
            return;
        }
        if let Err(e) = self.send_frame(frame) {
            self.lost(&e);
        }
    }

    pub fn set_screen(&mut self, on: bool) {
        self.screen_off = !on;
        if on {
            self.missing_since = None;
        }
        if self.wire.is_none() {
            return;
        }
        let code = if on { CMD_SCREEN_ON } else { CMD_SCREEN_OFF };
        if let Err(e) = self.send(&protocol::simple(code)) {
            self.lost(&e);
        }
        if on {
            self.repair_now = true;
        }
    }

    /// Resincroniza y repinta en el próximo ciclo. Lo pide el usuario o lo decide el propio guardián.
    pub fn repair(&mut self) {
        self.repair_now = true;
    }

    /// Último recurso, solo a petición del usuario: reinicia el panel con RESET.
    pub fn hard_reset(&mut self) {
        log::warn("RESET pedido por el usuario");
        if let Some(wire) = self.wire.as_mut() {
            // primero se alinea el panel: así el RESET se lee como comando y no como píxeles
            let _ = write_all(wire, &protocol::filler());
            let _ = write_all(wire, &protocol::resync_train());
            let _ = write_all(wire, &protocol::simple(CMD_RESET));
        }
        self.drop_link();
    }

    /// Suelta el panel antes de suspender, apagar o salir. Se llama entre imágenes, nunca a mitad de una.
    pub fn release(&mut self, screen_off: bool) {
        if screen_off && let Some(wire) = self.wire.as_mut() {
            let _ = wire.write(&protocol::simple(CMD_SCREEN_OFF));
        }
        self.drop_link();
        self.state = PanelState::Off;
    }

    /// Vuelve a buscar el panel tras despertar el PC.
    pub fn resume(&mut self) {
        self.drop_link();
        self.state = PanelState::Searching;
    }

    fn drop_link(&mut self) {
        self.wire = None;
        self.shown = None;
        self.must_settle = true;
        self.seen_at = None;
    }

    fn lost(&mut self, error: &LinkError) {
        log::warn(&format!("se perdió el panel: {error}"));
        self.drop_link();
        self.state = PanelState::Searching;
        self.port = None;
    }

    /// Intenta conectar. `true` si el panel quedó listo y ya tiene la imagen.
    fn connect(&mut self, frame: &Frame) -> bool {
        let now = self.clock.now_ms();
        let Some(port) = self.bus.find(&self.target.port) else {
            self.state = PanelState::Searching;
            self.seen_at = None;
            self.missing_since.get_or_insert(now);
            return false;
        };
        if self.must_settle {
            let seen = *self.seen_at.get_or_insert(now);
            if now - seen < SETTLE_MS {
                self.state = PanelState::Waking;
                return false;
            }
        }
        self.state = PanelState::Syncing;
        let wire = match self.bus.open(&port) {
            Ok(wire) => wire,
            Err(e) => {
                if !self.open_failure_logged {
                    self.open_failure_logged = true;
                    log::warn(&format!("{port}: {e}"));
                }
                self.state = PanelState::Searching;
                return false;
            }
        };
        self.wire = Some(wire);
        match self.bring_up(frame) {
            Ok(()) => {
                log::info(&format!("panel listo en {port}: cuadro en {} ms", self.frame_ms));
                if self.connected_before {
                    self.reconnects += 1;
                }
                self.connected_before = true;
                self.open_failure_logged = false;
                self.port = Some(port);
                self.state = PanelState::Healthy;
                self.must_settle = false;
                self.seen_at = None;
                self.missing_since = None;
                self.connected_at = self.clock.now_ms();
                self.repaints_done = 0;
                self.next_periodic = 0;
                self.repair_now = false;
                true
            }
            Err(e) => {
                self.lost(&e);
                false
            }
        }
    }

    fn bring_up(&mut self, frame: &Frame) -> Result<(), LinkError> {
        self.resync()?;
        self.send(&protocol::simple(CMD_SCREEN_ON))?;
        self.send(&protocol::brightness(self.target.brightness))?;
        self.send(&protocol::orientation(self.target.rotation))?;
        self.send_verified(frame)?;
        // al salir de un apagado la orden temprana a veces no enciende; con la imagen ya dibujada siempre funciona
        self.send(&protocol::simple(CMD_SCREEN_ON))
    }

    /// Envía el cuadro y, si el tiempo dice que el panel no lo dibujó, resincroniza y lo repite una vez.
    fn send_verified(&mut self, frame: &Frame) -> Result<(), LinkError> {
        self.send_frame(frame)?;
        if (self.frame_ms as u64) >= MIN_FRAME_MS {
            return Ok(());
        }
        log::warn(&format!("cuadro demasiado rápido ({} ms): se resincroniza", self.frame_ms));
        self.resync()?;
        self.send(&protocol::orientation(self.target.rotation))?;
        self.send_frame(frame)?;
        // solo es una reparación si al resincronizar volvió el tiempo normal: si sigue igual, nada indica que el cuadro fallara
        if (self.frame_ms as u64) >= MIN_FRAME_MS {
            self.healed += 1;
        }
        log::info(&format!("cuadro repetido en {} ms", self.frame_ms));
        Ok(())
    }

    /// Termina cualquier imagen a medias y alinea el panel.
    fn resync(&mut self) -> Result<(), LinkError> {
        self.send(&protocol::filler())?;
        self.send(&protocol::resync_train())
    }

    fn update(&mut self, frame: &Frame) -> Result<(), LinkError> {
        if std::mem::take(&mut self.repair_now) {
            log::info("reparación pedida: resincronizar y repintar");
            self.state = PanelState::Syncing;
            self.resync()?;
            self.send(&protocol::orientation(self.target.rotation))?;
            self.send_verified(frame)?;
            log::info(&format!("reparado: cuadro en {} ms", self.frame_ms));
            self.state = PanelState::Healthy;
            return Ok(());
        }
        if self.repaint_due() {
            return self.repaint(frame);
        }
        let Some(shown) = self.shown.as_ref() else { return self.send_frame(frame) };
        let rects = changed_rects(shown, frame);
        for rect in rects {
            self.send_rect(frame, rect)?;
        }
        self.shown = Some(frame.clone());
        Ok(())
    }

    fn repaint_due(&mut self) -> bool {
        let up = (self.clock.now_ms() - self.connected_at) / 1000;
        if let Some(at) = EARLY_REPAINTS.get(self.repaints_done) {
            if up >= *at {
                self.repaints_done += 1;
                self.next_periodic = up + REPAINT_EVERY_SECS;
                return true;
            }
            return false;
        }
        if up >= self.next_periodic {
            self.next_periodic = up + REPAINT_EVERY_SECS;
            return true;
        }
        false
    }

    /// Repinta sin que se note: el tren alinea y la imagen es la misma que ya había.
    fn repaint(&mut self, frame: &Frame) -> Result<(), LinkError> {
        self.send(&protocol::resync_train())?;
        self.send_verified(frame)?;
        log::info(&format!("repintado en {} ms", self.frame_ms));
        Ok(())
    }

    fn send_frame(&mut self, frame: &Frame) -> Result<(), LinkError> {
        let started = self.clock.now_ms();
        self.send_rect(frame, frame.full())?;
        self.frame_ms = (self.clock.now_ms() - started) as u32;
        self.shown = Some(frame.clone());
        Ok(())
    }

    fn send_rect(&mut self, frame: &Frame, rect: Rect) -> Result<(), LinkError> {
        self.send(&protocol::bitmap_header(rect))?;
        self.send(&frame.bytes_of(rect))
    }

    fn send(&mut self, data: &[u8]) -> Result<(), LinkError> {
        match self.wire.as_mut() {
            Some(wire) => write_all(wire, data),
            None => Ok(()),
        }
    }
}

fn write_all(wire: &mut impl Wire, data: &[u8]) -> Result<(), LinkError> {
    data.chunks(CHUNK_BYTES).try_for_each(|chunk| wire.write(chunk))
}

/// Duración esperada de un cuadro completo.
pub fn expected_frame_time() -> Duration {
    Duration::from_millis(FRAME_BYTES as u64 * 1000 / 128_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulator::{Fault, SimBus, SimClock};

    fn target() -> Target {
        Target { port: "auto".into(), brightness: 50, rotation: Rotation::LandscapeFlipped }
    }

    fn frame(color: u16) -> Frame {
        Frame::solid(480, 320, color)
    }

    fn guardian(bus: &SimBus, cold: bool) -> Guardian<SimBus, SimClock> {
        Guardian::new(bus.clone(), bus.clock(), target(), cold)
    }

    #[test]
    fn a_new_session_draws_correctly_over_a_transfer_left_unfinished() {
        for pending in [1000, 1002, 1004] {
            let bus = SimBus::new();
            bus.leave_unfinished_bitmap(pending);
            let mut g = guardian(&bus, false);
            g.show(&frame(0x1234));
            assert_eq!(g.state(), PanelState::Healthy);
            assert_eq!(bus.panel_image(), frame(0x1234).pixels, "{pending}");
            assert_eq!(bus.resets(), 0, "{pending}");
        }
    }

    #[test]
    fn without_the_resync_that_same_cut_would_reset_the_panel() {
        let bus = SimBus::new();
        bus.leave_unfinished_bitmap(1000);
        bus.feed_startup_without_resync();
        assert!(bus.misread_pixels_as_commands());
    }

    #[test]
    fn the_guardian_never_sends_reset_by_itself() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        for tick in 0..4000 {
            g.show(&frame(tick as u16 % 4));
            bus.clock().advance(1000);
            if tick == 100 {
                bus.inject(Fault::WriteFails);
            }
            if tick == 900 {
                bus.inject(Fault::Unplug);
            }
            if tick == 1000 {
                bus.inject(Fault::Replug);
            }
        }
        assert_eq!(bus.resets(), 0);
        assert_eq!(g.state(), PanelState::Healthy);
    }

    #[test]
    fn a_lost_panel_comes_back_by_itself_after_settling() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        bus.inject(Fault::Unplug);
        g.show(&frame(2));
        assert_eq!(g.state(), PanelState::Searching);
        bus.clock().advance(30_000);
        g.show(&frame(3));
        assert!(g.missing_secs() < 40);
        bus.inject(Fault::Replug);
        g.show(&frame(4));
        assert_eq!(g.state(), PanelState::Waking, "el firmware aún no está listo");
        bus.clock().advance(SETTLE_MS);
        g.show(&frame(5));
        assert_eq!(g.state(), PanelState::Healthy);
        assert_eq!(bus.panel_image(), frame(5).pixels);
        assert_eq!(g.reconnects, 1);
    }

    #[test]
    fn a_screen_turned_off_survives_sleep_and_can_be_turned_on_again() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        g.set_screen(false);
        g.release(true);
        g.resume();
        g.show(&frame(2));
        assert_eq!(g.state(), PanelState::Off, "la ventana y la bandeja tienen que verla apagada");
        g.set_screen(true);
        g.show(&frame(2));
        bus.clock().advance(SETTLE_MS);
        g.show(&frame(2));
        assert_eq!(g.state(), PanelState::Healthy);
        assert!(bus.screen_on());
        assert_eq!(bus.panel_image(), frame(2).pixels);
    }

    #[test]
    fn turning_the_screen_off_while_unplugged_is_not_reported_as_missing() {
        let bus = SimBus::new();
        bus.inject(Fault::Unplug);
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        g.set_screen(false);
        bus.clock().advance(120_000);
        g.show(&frame(1));
        assert_eq!((g.state(), g.missing_secs()), (PanelState::Off, 0));
    }

    #[test]
    fn cleaning_never_turns_on_a_screen_the_user_turned_off() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        g.set_screen(false);
        let before = bus.bytes_received();
        g.show_full(&frame(2));
        assert_eq!(bus.bytes_received(), before);
    }

    #[test]
    fn a_cold_start_is_not_a_reconnection() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, true);
        bus.clock().advance(SETTLE_MS);
        g.show(&frame(1));
        g.show(&frame(1));
        bus.clock().advance(SETTLE_MS);
        g.show(&frame(1));
        assert_eq!(g.reconnects, 0);
    }

    #[test]
    fn nothing_is_written_until_a_cold_start_has_settled() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, true);
        g.show(&frame(1));
        assert_eq!(bus.bytes_received(), 0);
        bus.clock().advance(SETTLE_MS);
        g.show(&frame(1));
        assert_eq!(g.state(), PanelState::Healthy);
    }

    #[test]
    fn a_partial_write_counts_as_a_lost_panel() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        bus.inject(Fault::PartialWrite);
        g.show(&frame(2));
        assert_eq!(g.state(), PanelState::Searching);
    }

    #[test]
    fn only_what_changed_is_sent() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(0));
        let before = bus.bytes_received();
        let mut next = frame(0);
        next.pixels[100] = 0xffff;
        g.show(&next);
        assert_eq!(bus.bytes_received() - before, 6 + 2);
        assert_eq!(bus.panel_image(), next.pixels);
    }

    #[test]
    fn an_unchanged_image_sends_nothing() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(0));
        let before = bus.bytes_received();
        g.show(&frame(0));
        assert_eq!(bus.bytes_received(), before);
    }

    #[test]
    fn repaints_heal_a_panel_that_was_silently_damaged() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(7));
        bus.corrupt_image();
        assert_ne!(bus.panel_image(), frame(7).pixels);
        bus.clock().advance(20_000);
        g.show(&frame(7));
        assert_eq!(bus.panel_image(), frame(7).pixels);
    }

    #[test]
    fn a_frame_that_went_too_fast_triggers_a_resync() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(7));
        bus.clock().advance(20_000);
        bus.inject(Fault::PanelNotDrawing);
        g.show(&frame(7));
        assert_eq!(g.healed, 1);
        assert_eq!(bus.panel_image(), frame(7).pixels);
    }

    #[test]
    fn a_fast_link_resyncs_but_is_not_counted_as_a_repair() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(7));
        bus.clock().advance(20_000);
        bus.inject(Fault::FastLink);
        let before = bus.bytes_received();
        g.show(&frame(7));
        assert!(bus.bytes_received() - before > 2 * FRAME_BYTES, "se tiene que resincronizar y repetir el cuadro");
        assert_eq!(g.healed, 0);
        assert_eq!(bus.panel_image(), frame(7).pixels);
    }

    #[test]
    fn the_first_frame_after_connecting_is_verified_too() {
        let bus = SimBus::new();
        bus.inject(Fault::PanelNotDrawing);
        let mut g = guardian(&bus, false);
        g.show(&frame(9));
        assert_eq!(g.healed, 1);
        assert_eq!(bus.panel_image(), frame(9).pixels);
    }

    #[test]
    fn a_healthy_panel_is_never_marked_as_healed() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        for _ in 0..7200 {
            g.show(&frame(1));
            bus.clock().advance(1000);
        }
        assert_eq!(g.healed, 0);
    }

    #[test]
    fn the_screen_stays_off_until_asked_and_comes_back_repainted() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        g.set_screen(false);
        assert!(!bus.screen_on());
        let before = bus.bytes_received();
        g.show(&frame(2));
        assert_eq!(bus.bytes_received(), before);
        g.set_screen(true);
        g.show(&frame(2));
        assert!(bus.screen_on());
        assert_eq!(bus.panel_image(), frame(2).pixels);
    }

    #[test]
    fn releasing_never_cuts_an_image_and_turns_the_screen_off_last() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        g.release(true);
        assert!(!bus.screen_on());
        assert!(!bus.waiting_for_pixels());
    }

    #[test]
    fn waking_up_reconnects_and_repaints() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        g.release(true);
        g.resume();
        g.show(&frame(2));
        bus.clock().advance(SETTLE_MS);
        g.show(&frame(2));
        assert_eq!(g.state(), PanelState::Healthy);
        assert!(bus.screen_on());
        assert_eq!(bus.panel_image(), frame(2).pixels);
    }

    #[test]
    fn a_change_of_brightness_needs_no_reconnection() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        let opens = bus.opens();
        g.retarget(Target { brightness: 80, ..target() });
        assert_eq!(bus.opens(), opens);
        assert_eq!(bus.brightness_level(), 255 - 204);
    }

    #[test]
    fn a_hard_reset_is_read_as_a_command_even_with_an_unfinished_image() {
        let bus = SimBus::new();
        let mut g = guardian(&bus, false);
        g.show(&frame(1));
        bus.leave_unfinished_bitmap(1000);
        g.hard_reset();
        assert_eq!(bus.resets(), 1);
    }

    #[test]
    fn expected_time_matches_the_measured_panel() {
        assert_eq!(expected_frame_time().as_millis(), 2400);
    }
}
