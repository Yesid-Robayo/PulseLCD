//! Panel de mentira para las pruebas: lee grupos de 6 bytes, cuenta los píxeles que espera y se equivoca igual que el
//! real cuando queda desalineado. Consume a 128 kB/s y permite provocar fallos.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::guardian::{Bus, Clock, LinkError, Wire};
use crate::protocol::{self, CMD_BITMAP, CMD_BRIGHTNESS, CMD_ORIENTATION, CMD_RESET, CMD_SCREEN_OFF, CMD_SCREEN_ON};

const W: usize = 480;
const H: usize = 320;
/// Velocidad real del panel en cada modo, en bytes por segundo.
const PIXEL_RATE: f64 = 128_000.0;
const COMMAND_RATE: f64 = 132_000.0;
/// Visto a veces en el panel real, sin explicación: cuadros de unos 2 s que se dibujan bien.
const FAST_LINK_RATE: f64 = 153_000.0;

#[derive(Clone, Copy, PartialEq)]
pub enum Fault {
    WriteFails,
    PartialWrite,
    Unplug,
    Replug,
    /// El siguiente cuadro completo llega al panel en modo comando.
    PanelNotDrawing,
    /// Todo pasa más rápido de lo normal, y el panel lo dibuja igual.
    FastLink,
}

#[derive(Default)]
struct Parser {
    group: Vec<u8>,
    extra: usize,
    pixels_left: usize,
    window: (usize, usize, usize, usize),
    cursor: (usize, usize),
    low: Option<u8>,
}

struct Panel {
    parser: Parser,
    image: Vec<u16>,
    screen_on: bool,
    resets: u32,
    brightness: u16,
    misread: bool,
    received: usize,
}

impl Panel {
    fn feed(&mut self, data: &[u8]) {
        self.received += data.len();
        for &byte in data {
            self.byte(byte);
        }
    }

    fn byte(&mut self, byte: u8) {
        let p = &mut self.parser;
        if p.pixels_left > 0 {
            p.pixels_left -= 1;
            if let Some(low) = p.low.take() {
                let (x, y) = p.cursor;
                let (x0, _, x1, y1) = p.window;
                if x < W && y < H {
                    self.image[y * W + x] = u16::from_le_bytes([low, byte]);
                }
                p.cursor = if x >= x1 { (x0, (y + 1).min(y1 + 1)) } else { (x + 1, y) };
            } else {
                p.low = Some(byte);
            }
            return;
        }
        if p.extra > 0 {
            p.extra -= 1;
            return;
        }
        p.group.push(byte);
        if p.group.len() < 6 {
            return;
        }
        let g = std::mem::take(&mut p.group);
        let bits = u64::from_be_bytes([0, 0, 0, g[0], g[1], g[2], g[3], g[4]]);
        let coord = |shift: u32| ((bits >> shift) & 0x3ff) as usize;
        let (x, y, ex, ey) = (coord(30), coord(20), coord(10), coord(0));
        match g[5] {
            CMD_BITMAP => {
                if ex >= x && ey >= y {
                    p.window = (x, y, ex, ey);
                    p.cursor = (x, y);
                    p.pixels_left = (ex - x + 1) * (ey - y + 1) * 2;
                }
            }
            CMD_ORIENTATION => p.extra = 10,
            CMD_SCREEN_ON => self.screen_on = true,
            CMD_SCREEN_OFF => self.screen_on = false,
            CMD_BRIGHTNESS => self.brightness = x as u16,
            CMD_RESET => self.resets += 1,
            _ => {}
        }
    }
}

struct World {
    panel: Panel,
    present: bool,
    write_fails: bool,
    partial: bool,
    /// Bytes de píxeles que aún se perderán.
    lost_frame_bytes: usize,
    /// Ya pasó la cabecera del cuadro que se pierde.
    armed: bool,
    fast_link: bool,
    opens: u32,
}

#[derive(Clone)]
pub struct SimBus {
    world: Rc<RefCell<World>>,
    now: Rc<Cell<f64>>,
}

#[derive(Clone)]
pub struct SimClock(Rc<Cell<f64>>);

impl SimClock {
    pub fn advance(&self, ms: u64) {
        self.0.set(self.0.get() + ms as f64);
    }
}

impl Clock for SimClock {
    fn now_ms(&self) -> u64 {
        self.0.get() as u64
    }
}

pub struct SimWire {
    world: Rc<RefCell<World>>,
    now: Rc<Cell<f64>>,
}

impl Wire for SimWire {
    fn write(&mut self, data: &[u8]) -> Result<(), LinkError> {
        let mut w = self.world.borrow_mut();
        if !w.present || std::mem::take(&mut w.write_fails) {
            return Err(LinkError::Write(31));
        }
        if std::mem::take(&mut w.partial) {
            let half = data.len() / 2;
            w.panel.feed(&data[..half]);
            return Err(LinkError::Partial { written: half, wanted: data.len() });
        }
        let drawing = w.panel.parser.pixels_left > 0 && !w.armed;
        let rate = match (w.fast_link, drawing) {
            (true, _) => FAST_LINK_RATE,
            (false, true) => PIXEL_RATE,
            (false, false) => COMMAND_RATE,
        };
        self.now.set(self.now.get() + data.len() as f64 * 1000.0 / rate);
        if w.lost_frame_bytes > 0 {
            // el cuadro se consume sin llegar a la imagen: el fallo que el tiempo tiene que delatar. Los comandos
            // cortos también se pierden, y solo los bloques de píxeles cuentan para terminar el cuadro.
            if w.armed {
                if data.len() >= protocol::CHUNK_BYTES {
                    w.lost_frame_bytes = w.lost_frame_bytes.saturating_sub(data.len());
                }
                // pasado el cuadro perdido, el panel vuelve a dibujar
                w.armed = w.lost_frame_bytes > 0;
                return Ok(());
            }
            // el relleno de la conexión no cuenta: el fallo es del cuadro que viene detrás
            w.armed = data == protocol::bitmap_header(crate::protocol::Rect { x: 0, y: 0, w: 480, h: 320 });
            if w.armed {
                return Ok(());
            }
        }
        w.panel.feed(data);
        Ok(())
    }
}

impl SimBus {
    pub fn new() -> Self {
        let panel = Panel {
            parser: Parser::default(),
            image: vec![0; W * H],
            screen_on: false,
            resets: 0,
            brightness: 0,
            misread: false,
            received: 0,
        };
        let world = World {
            panel,
            present: true,
            write_fails: false,
            partial: false,
            lost_frame_bytes: 0,
            armed: false,
            fast_link: false,
            opens: 0,
        };
        Self { world: Rc::new(RefCell::new(world)), now: Rc::new(Cell::new(0.0)) }
    }

    pub fn clock(&self) -> SimClock {
        SimClock(self.now.clone())
    }

    pub fn inject(&self, fault: Fault) {
        let mut w = self.world.borrow_mut();
        match fault {
            Fault::WriteFails => w.write_fails = true,
            Fault::PartialWrite => w.partial = true,
            Fault::Unplug => w.present = false,
            Fault::Replug => w.present = true,
            Fault::PanelNotDrawing => {
                w.lost_frame_bytes = protocol::FRAME_BYTES;
                w.armed = false;
            }
            Fault::FastLink => w.fast_link = true,
        }
    }

    /// El panel se queda esperando píxeles, como tras un apagado a mitad de una imagen.
    pub fn leave_unfinished_bitmap(&self, pending: usize) {
        let mut w = self.world.borrow_mut();
        let p = &mut w.panel.parser;
        p.window = (14, 160, 153, 173);
        p.cursor = (14, 160);
        p.pixels_left = pending;
        p.low = None;
        p.group.clear();
    }

    /// Lo que hacía una sesión al arrancar sin resincronizar: comandos y fondo directamente.
    pub fn feed_startup_without_resync(&self) {
        let background: Vec<u8> = (0..W * H).flat_map(|i| (0x6500u16 | (i % 256) as u16).to_le_bytes()).collect();
        let mut stream = Vec::new();
        stream.extend(protocol::simple(CMD_SCREEN_ON));
        stream.extend(protocol::bitmap_header(crate::protocol::Rect { x: 0, y: 0, w: 480, h: 320 }));
        stream.extend(background);
        let mut w = self.world.borrow_mut();
        let resets = w.panel.resets;
        w.panel.feed(&stream);
        w.panel.misread = w.panel.resets > resets;
    }

    pub fn misread_pixels_as_commands(&self) -> bool {
        self.world.borrow().panel.misread
    }

    pub fn panel_image(&self) -> Vec<u16> {
        self.world.borrow().panel.image.clone()
    }

    pub fn corrupt_image(&self) {
        let mut w = self.world.borrow_mut();
        for px in w.panel.image.iter_mut().step_by(3) {
            *px = !*px;
        }
    }

    pub fn resets(&self) -> u32 {
        self.world.borrow().panel.resets
    }

    pub fn bytes_received(&self) -> usize {
        self.world.borrow().panel.received
    }

    pub fn screen_on(&self) -> bool {
        self.world.borrow().panel.screen_on
    }

    pub fn waiting_for_pixels(&self) -> bool {
        self.world.borrow().panel.parser.pixels_left > 0
    }

    pub fn opens(&self) -> u32 {
        self.world.borrow().opens
    }

    pub fn brightness_level(&self) -> u16 {
        self.world.borrow().panel.brightness
    }
}

impl Bus for SimBus {
    type Wire = SimWire;

    fn find(&mut self, _wanted: &str) -> Option<String> {
        self.world.borrow().present.then(|| "COM5".to_string())
    }

    fn open(&mut self, _port: &str) -> Result<SimWire, LinkError> {
        self.world.borrow_mut().opens += 1;
        Ok(SimWire { world: self.world.clone(), now: self.now.clone() })
    }
}
