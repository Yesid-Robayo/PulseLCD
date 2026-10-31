//! FPS con PresentMon, la herramienta de Intel, que corre como proceso hijo solo mientras se necesita. Va dentro de un
//! objeto de trabajo: si el ayudante muere de golpe, Windows la cierra con él y no queda nada midiendo.
//!
//! Se prefiere la copia que trae el driver de AMD o NVIDIA, que es la que coincide con ese driver y la que se ha probado con
//! Frame Generation. La oficial de Intel que acompaña a PulseLCD es para los equipos que no traen ninguna.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::ptr::null;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pulse_shared::broker::{self, FPS_NO_TOOL, FPS_OK, FPS_SESSION};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, ControlTraceW, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_PROPERTIES,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

/// Cuadros que se promedian para que el número no baile.
const WINDOW: usize = 30;
/// Sin cuadros nuevos en este tiempo, el programa dejó de presentar.
const STALE: Duration = Duration::from_secs(3);
const READ_BUFFER: usize = 16 * 1024;
const MAX_LINE: usize = 64 * 1024;
/// Espera antes de volver a lanzar la herramienta si se cerró sola.
const RETRY: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Series {
    frames_ms: VecDeque<f32>,
    updated: Option<Instant>,
}

type Shared = Arc<Mutex<HashMap<u32, Series>>>;

/// Posiciones de las columnas que interesan, leídas de la cabecera porque cambian entre versiones.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Columns {
    pid: usize,
    frame: usize,
    displayed: Option<usize>,
}

fn columns(header: &str) -> Option<Columns> {
    let names: Vec<&str> = header.split(',').map(str::trim).collect();
    let find = |wanted: &[&str]| names.iter().position(|n| wanted.contains(n));
    Some(Columns {
        pid: find(&["ProcessID"])?,
        frame: find(&["FrameTime", "MsBetweenPresents", "msBetweenPresents"])?,
        displayed: find(&["DisplayedTime", "MsBetweenDisplayChange", "msBetweenDisplayChange"]),
    })
}

/// Proceso y duración de un cuadro. Con Frame Generation, el tiempo mostrado es el que cuenta lo que se ve en pantalla.
fn parse_line(line: &str, c: Columns, count_generated: bool) -> Option<(u32, f32)> {
    let fields: Vec<&str> = line.split(',').collect();
    let number = |i: usize| fields.get(i)?.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0);
    let pid = fields.get(c.pid)?.trim().parse().ok()?;
    let shown = if count_generated { c.displayed.and_then(number) } else { None };
    Some((pid, shown.or_else(|| number(c.frame))?))
}

/// Parte en líneas lo que escribe PresentMon. Sale en UTF-16 con su marca al principio, tanto la copia de AMD como la de
/// Intel; se admite también UTF-8 por si otra versión cambia. Leerlo como texto normal falla en la primera línea.
#[derive(Default)]
struct Lines {
    wide: Option<bool>,
    pending: Vec<u8>,
}

impl Lines {
    fn feed(&mut self, data: &[u8], mut line: impl FnMut(String)) {
        self.pending.extend_from_slice(data);
        let wide = match self.wide {
            Some(wide) => wide,
            None if self.pending.len() < 2 => return,
            None => {
                let wide = self.pending.starts_with(&[0xff, 0xfe]);
                if wide {
                    self.pending.drain(..2);
                }
                self.wide = Some(wide);
                wide
            }
        };
        let unit = if wide { 2 } else { 1 };
        let (mut start, mut at) = (0, 0);
        while at + unit <= self.pending.len() {
            let newline = self.pending[at] == b'\n' && (!wide || self.pending[at + 1] == 0);
            if newline {
                line(decode(&self.pending[start..at], wide));
                start = at + unit;
            }
            at += unit;
        }
        self.pending.drain(..start);
        // una línea así no existe: es basura y no debe crecer sin límite
        if self.pending.len() > MAX_LINE {
            self.pending.clear();
        }
    }
}

fn decode(bytes: &[u8], wide: bool) -> String {
    let text = if wide {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.trim_end_matches('\r').to_string()
}

fn record(shared: &Shared, pid: u32, ms: f32) {
    let mut map = shared.lock().unwrap_or_else(|e| e.into_inner());
    let series = map.entry(pid).or_default();
    if series.frames_ms.len() >= WINDOW {
        series.frames_ms.pop_front();
    }
    series.frames_ms.push_back(ms);
    series.updated = Some(Instant::now());
}

fn read_output(child: &mut Child, shared: Shared, count_generated: bool) {
    let Some(mut stdout) = child.stdout.take() else { return };
    std::thread::spawn(move || {
        let mut lines = Lines::default();
        let mut cols = None;
        let mut buffer = [0u8; READ_BUFFER];
        while let Ok(read) = stdout.read(&mut buffer) {
            if read == 0 {
                return;
            }
            lines.feed(&buffer[..read], |line| match cols {
                None => cols = columns(&line),
                Some(c) => {
                    if let Some((pid, ms)) = parse_line(&line, c, count_generated) {
                        record(&shared, pid, ms);
                    }
                }
            });
        }
    });
}

/// La copia de un driver o, si no hay, la que está junto al ayudante, en Program Files.
fn find_tool() -> Option<PathBuf> {
    let system = crate::pawnio::program_files()?;
    let exe = std::env::current_exe().ok()?;
    Some(broker::find_fps_tool(&system, &[exe.parent()?])?.1)
}

/// Las sesiones de seguimiento son del sistema, no del proceso: cerrar PresentMon a la fuerza deja la suya abierta hasta
/// reiniciar, gastando memoria del kernel para nadie.
pub fn close_session() {
    const NAMES: usize = 2048;
    let head = size_of::<EVENT_TRACE_PROPERTIES>();
    let name: Vec<u16> = FPS_SESSION.encode_utf16().chain(std::iter::once(0)).collect();
    // Windows escribe los nombres de la sesión justo detrás de la estructura
    let mut space = vec![0u64; (head + NAMES).div_ceil(size_of::<u64>())];
    let properties = space.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>();
    unsafe {
        (*properties).Wnode.BufferSize = (head + NAMES) as u32;
        (*properties).LoggerNameOffset = head as u32;
        (*properties).LogFileNameOffset = (head + NAMES / 2) as u32;
        ControlTraceW(CONTROLTRACE_HANDLE { Value: 0 }, name.as_ptr(), properties, EVENT_TRACE_CONTROL_STOP);
    }
}

struct Job(HANDLE);

impl Job {
    fn new() -> Option<Self> {
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            Some(Self(job))
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub struct FpsMeter {
    child: Option<(Child, Job, bool)>,
    shared: Shared,
    last_try: Option<Instant>,
    tool: bool,
}

impl Default for FpsMeter {
    fn default() -> Self {
        Self { child: None, shared: Shared::default(), last_try: None, tool: find_tool().is_some() }
    }
}

impl FpsMeter {
    pub fn status(&self) -> u8 {
        if self.tool { FPS_OK } else { FPS_NO_TOOL }
    }

    /// Mide si aún no lo hace. `count_generated` elige qué cuadros cuentan.
    pub fn start(&mut self, count_generated: bool) {
        if let Some((child, _, mode)) = self.child.as_mut() {
            let running = matches!(child.try_wait(), Ok(None));
            if running && *mode == count_generated {
                return;
            }
            // cambiar de modo es inmediato; si se cerró sola, se espera para no relanzarla cada segundo
            if running {
                self.last_try = None;
            }
            self.stop();
        }
        if self.last_try.is_some_and(|t| t.elapsed() < RETRY) {
            return;
        }
        self.last_try = Some(Instant::now());
        let Some(tool) = find_tool() else {
            self.tool = false;
            return;
        };
        let spawned = Command::new(tool)
            .args(["--output_stdout", "--no_console_stats", "--stop_existing_session", "--session_name", FPS_SESSION])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let Ok(mut child) = spawned else { return };
        let Some(job) = Job::new() else {
            let _ = child.kill();
            return;
        };
        unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) };
        read_output(&mut child, self.shared.clone(), count_generated);
        self.child = Some((child, job, count_generated));
    }

    pub fn stop(&mut self) {
        if let Some((mut child, _job, _)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            close_session();
        }
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// FPS de un proceso; 0 si no está presentando cuadros.
    pub fn fps_of(&self, pid: u32) -> f32 {
        let map = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let Some(series) = map.get(&pid) else { return 0.0 };
        if series.updated.is_none_or(|t| t.elapsed() > STALE) || series.frames_ms.is_empty() {
            return 0.0;
        }
        let average = series.frames_ms.iter().sum::<f32>() / series.frames_ms.len() as f32;
        (1000.0 / average).round()
    }
}

impl Drop for FpsMeter {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "Application,ProcessID,SwapChainAddress,PresentRuntime,FrameTime,CPUBusy,DisplayedTime,GPUBusy";

    /// Cómo sale de verdad: UTF-16 con su marca delante, capturado de las dos copias.
    fn wide(text: &str) -> Vec<u8> {
        std::iter::once(0xfeffu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect()
    }

    const AMD_OUTPUT: &str = "Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,CPUStartTime,FrameTime,CPUBusy,CPUWait,GPULatency,GPUTime,GPUBusy,GPUWait,DisplayLatency,DisplayedTime\r\nhollow_knight.exe,32484,0x2329A988B70,DXGI,1,0,0,Hardware Composed: Independent Flip,1.5,6.9,1.2,5.7,0.5,3.1,2.9,0.2,12.5,6.95\r\n";
    const INTEL_OUTPUT: &str = "Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,TimeInMs,MsBetweenSimulationStart,MsBetweenPresents,MsBetweenDisplayChange\r\nhollow_knight.exe,32484,0x2329A988B70,DXGI,1,0,0,Hardware Composed: Independent Flip,9.9,NA,6.9,6.95\r\n";

    fn read_all(bytes: &[u8], step: usize) -> Vec<String> {
        let (mut lines, mut found) = (Lines::default(), Vec::new());
        for piece in bytes.chunks(step) {
            lines.feed(piece, |line| found.push(line));
        }
        found
    }

    #[test]
    fn the_real_output_of_both_copies_gives_the_game_and_its_frame_time() {
        for output in [AMD_OUTPUT, INTEL_OUTPUT] {
            let lines = read_all(&wide(output), 4096);
            let cols = columns(&lines[0]).expect("cabecera");
            assert_eq!(parse_line(&lines[1], cols, true), Some((32484, 6.95)));
            assert_eq!(parse_line(&lines[1], cols, false), Some((32484, 6.9)));
        }
    }

    #[test]
    fn output_cut_at_any_byte_still_gives_whole_lines() {
        for step in [1, 2, 3, 7, 63] {
            assert_eq!(read_all(&wide(AMD_OUTPUT), step).len(), 2, "de {step} en {step}");
        }
    }

    #[test]
    fn plain_text_output_also_works() {
        assert_eq!(read_all(b"una\r\ndos\n", 3), ["una", "dos"]);
    }

    #[test]
    fn a_character_outside_ascii_does_not_stop_the_reading() {
        let lines = read_all(&wide("Código ñ.exe,1\nsigue,2\n"), 5);
        assert_eq!(lines, ["Código ñ.exe,1", "sigue,2"]);
    }

    #[test]
    fn a_line_that_never_ends_does_not_grow_forever() {
        let mut lines = Lines::default();
        lines.feed(&vec![b'x'; MAX_LINE * 2], |_| {});
        assert!(lines.pending.is_empty());
    }

    #[test]
    fn the_columns_come_from_the_header() {
        assert_eq!(columns(HEADER), Some(Columns { pid: 1, frame: 4, displayed: Some(6) }));
        assert_eq!(columns("Application,Otra"), None);
    }

    #[test]
    fn the_old_column_names_also_work() {
        let c = columns("Application,ProcessID,msBetweenPresents,msBetweenDisplayChange").unwrap();
        assert_eq!(c, Columns { pid: 1, frame: 2, displayed: Some(3) });
    }

    #[test]
    fn the_names_of_the_nvidia_copy_also_work() {
        let c = columns("Application,ProcessID,MsBetweenPresents,MsBetweenDisplayChange").unwrap();
        assert_eq!(c, Columns { pid: 1, frame: 2, displayed: Some(3) });
    }

    #[test]
    fn generated_frames_use_the_displayed_time() {
        let c = columns(HEADER).unwrap();
        let line = "game.exe,4242,0x1,DXGI,16.6,4.0,8.3,5.0";
        assert_eq!(parse_line(line, c, true), Some((4242, 8.3)));
        assert_eq!(parse_line(line, c, false), Some((4242, 16.6)));
    }

    #[test]
    fn a_frame_without_displayed_time_falls_back_to_frame_time() {
        let c = columns(HEADER).unwrap();
        assert_eq!(parse_line("game.exe,4242,0x1,DXGI,16.6,4.0,NA,5.0", c, true), Some((4242, 16.6)));
    }

    #[test]
    fn broken_lines_are_skipped() {
        let c = columns(HEADER).unwrap();
        assert_eq!(parse_line("nada", c, true), None);
        assert_eq!(parse_line("game.exe,no-es-pid,0x1,DXGI,16.6,4.0,8.3,5.0", c, true), None);
    }

    #[test]
    fn nothing_measured_reads_zero() {
        assert_eq!(FpsMeter::default().fps_of(1234), 0.0);
    }
}
