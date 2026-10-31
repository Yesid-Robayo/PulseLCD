//! GPU NVIDIA con NVML, la biblioteca oficial que instala su driver. Se carga al vuelo y solo desde System32: sin GPU
//! NVIDIA no hay `nvml.dll` y todo lo demás sigue funcionando.

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};

type Device = *mut c_void;

const TEMPERATURE_GPU: u32 = 0;
const CLOCK_GRAPHICS: u32 = 0;
const MB: f64 = 1024.0 * 1024.0;

#[repr(C)]
#[derive(Clone, Copy)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryInfo {
    total: u64,
    free: u64,
    used: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuReading {
    pub usage: Option<f64>,
    pub temp: Option<f64>,
    pub memory_used_mb: Option<f64>,
    pub memory_percent: Option<f64>,
    pub fan: Option<f64>,
    pub clock_ghz: Option<f64>,
}

struct Calls {
    shutdown: unsafe extern "C" fn() -> i32,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> i32,
    temperature: unsafe extern "C" fn(Device, u32, *mut u32) -> i32,
    memory: unsafe extern "C" fn(Device, *mut MemoryInfo) -> i32,
    fan: unsafe extern "C" fn(Device, *mut u32) -> i32,
    clock: unsafe extern "C" fn(Device, u32, *mut u32) -> i32,
}

pub struct Nvml {
    library: HMODULE,
    device: Device,
    calls: Calls,
}

// NVML permite llamadas desde cualquier hilo, y el dispositivo es un identificador opaco.
unsafe impl Send for Nvml {}

/// Busca una función de la biblioteca. `T` tiene que ser un puntero a función con la firma real.
unsafe fn load<T: Copy>(lib: HMODULE, name: &[u8]) -> Option<T> {
    let address = unsafe { GetProcAddress(lib, name.as_ptr()) }?;
    Some(unsafe { std::mem::transmute_copy(&address) })
}

impl Nvml {
    /// La primera GPU NVIDIA, si la hay.
    pub fn open() -> Option<Self> {
        unsafe {
            let name: Vec<u16> = "nvml.dll".encode_utf16().chain(std::iter::once(0)).collect();
            let lib = LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if lib.is_null() {
                return None;
            }
            let opened = Self::start(lib);
            if opened.is_none() {
                FreeLibrary(lib);
            }
            opened
        }
    }

    unsafe fn start(lib: HMODULE) -> Option<Self> {
        unsafe {
            let init: unsafe extern "C" fn() -> i32 = load(lib, b"nvmlInit_v2\0")?;
            let handle: unsafe extern "C" fn(u32, *mut Device) -> i32 = load(lib, b"nvmlDeviceGetHandleByIndex_v2\0")?;
            let calls = Calls {
                shutdown: load(lib, b"nvmlShutdown\0")?,
                utilization: load(lib, b"nvmlDeviceGetUtilizationRates\0")?,
                temperature: load(lib, b"nvmlDeviceGetTemperature\0")?,
                memory: load(lib, b"nvmlDeviceGetMemoryInfo\0")?,
                fan: load(lib, b"nvmlDeviceGetFanSpeed\0")?,
                clock: load(lib, b"nvmlDeviceGetClockInfo\0")?,
            };
            if init() != 0 {
                return None;
            }
            let mut device: Device = null_mut();
            if handle(0, &mut device) != 0 || device.is_null() {
                (calls.shutdown)();
                return None;
            }
            Some(Self { library: lib, device, calls })
        }
    }

    pub fn read(&self) -> GpuReading {
        unsafe fn fetch<T: Copy>(call: impl FnOnce(*mut T) -> i32) -> Option<T> {
            let mut value = MaybeUninit::<T>::uninit();
            (call(value.as_mut_ptr()) == 0).then(|| unsafe { value.assume_init() })
        }
        let d = self.device;
        let c = &self.calls;
        unsafe {
            let usage = fetch(|p| (c.utilization)(d, p));
            let temp = fetch(|p| (c.temperature)(d, TEMPERATURE_GPU, p));
            let memory = fetch(|p| (c.memory)(d, p));
            let fan = fetch(|p| (c.fan)(d, p));
            let clock = fetch(|p| (c.clock)(d, CLOCK_GRAPHICS, p));
            GpuReading {
                usage: usage.map(|u: Utilization| u.gpu as f64),
                temp: temp.map(|t: u32| t as f64),
                memory_used_mb: memory.map(|m: MemoryInfo| m.used as f64 / MB),
                memory_percent: memory.filter(|m| m.total > 0).map(|m| m.used as f64 / m.total as f64 * 100.0),
                fan: fan.map(|f: u32| f as f64),
                clock_ghz: clock.map(|c: u32| c as f64 / 1000.0),
            }
        }
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // soltarla importa: tras actualizar el driver, volver a abrir debe cargar la versión nueva
        unsafe {
            (self.calls.shutdown)();
            FreeLibrary(self.library);
        }
    }
}
