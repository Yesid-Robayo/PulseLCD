//! CPU, memoria, disco, red y latencia con la API de Windows.

use std::mem::{size_of, zeroed};
use std::net::{SocketAddr, ToSocketAddrs};
use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::Win32::Foundation::{FILETIME, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, ICMP_ECHO_REPLY, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, MIB_IF_TABLE2,
};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const PING_TIMEOUT_MS: u32 = 1000;
const IF_OPER_UP: i32 = 1;

fn ticks(t: FILETIME) -> u64 {
    (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
}

/// Uso del CPU: la diferencia de tiempos entre dos lecturas.
#[derive(Default)]
pub struct CpuLoad {
    last: Option<(u64, u64)>,
}

impl CpuLoad {
    pub fn sample(&mut self) -> Option<f64> {
        let mut idle: FILETIME = unsafe { zeroed() };
        let mut kernel: FILETIME = unsafe { zeroed() };
        let mut user: FILETIME = unsafe { zeroed() };
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return None;
        }
        // el tiempo de kernel ya incluye el inactivo
        let now = (ticks(idle), ticks(kernel) + ticks(user));
        let (last_idle, last_total) = self.last.replace(now)?;
        let total = now.1.saturating_sub(last_total);
        (total > 0).then(|| {
            let idle = now.0.saturating_sub(last_idle);
            (100.0 * (1.0 - idle as f64 / total as f64)).clamp(0.0, 100.0)
        })
    }
}

pub struct Memory {
    pub percent: f64,
    pub used_gb: f64,
    pub swap_percent: f64,
}

pub fn memory() -> Option<Memory> {
    let mut m: MEMORYSTATUSEX = unsafe { zeroed() };
    m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return None;
    }
    let swap = if m.ullTotalPageFile > 0 {
        (m.ullTotalPageFile - m.ullAvailPageFile) as f64 / m.ullTotalPageFile as f64 * 100.0
    } else {
        0.0
    };
    Some(Memory {
        percent: m.dwMemoryLoad as f64,
        used_gb: (m.ullTotalPhys - m.ullAvailPhys) as f64 / GIB,
        swap_percent: swap,
    })
}

pub struct Disk {
    pub percent: f64,
    pub used_gb: f64,
    pub free_gb: f64,
}

/// Espacio de la unidad del sistema.
pub fn disk() -> Option<Disk> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let path: Vec<u16> = format!("{drive}\\").encode_utf16().chain(std::iter::once(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, null_mut()) } == 0 || total == 0 {
        return None;
    }
    let used = total - free;
    Some(Disk { percent: used as f64 / total as f64 * 100.0, used_gb: used as f64 / GIB, free_gb: free as f64 / GIB })
}

pub fn uptime_secs() -> u64 {
    unsafe { GetTickCount64() / 1000 }
}

/// Totales de una interfaz.
#[derive(Debug, Clone, PartialEq)]
struct Interface {
    name: String,
    received: u64,
    sent: u64,
}

fn interfaces() -> Vec<Interface> {
    let mut list = Vec::new();
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = null_mut();
        if GetIfTable2(&mut table) != 0 || table.is_null() {
            return list;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for row in rows {
            // solo tarjetas de verdad, sin túneles, filtros ni adaptadores virtuales de Windows
            let hardware = row.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            if !hardware || row.OperStatus != IF_OPER_UP {
                continue;
            }
            let len = row.Alias.iter().position(|c| *c == 0).unwrap_or(row.Alias.len());
            list.push(Interface {
                name: String::from_utf16_lossy(&row.Alias[..len]),
                received: row.InOctets,
                sent: row.OutOctets,
            });
        }
        FreeMibTable(table as *const _);
    }
    list
}

/// Nombres de las interfaces activas, para elegir una en los ajustes.
pub fn interface_names() -> Vec<String> {
    interfaces().into_iter().map(|i| i.name).collect()
}

pub struct Traffic {
    pub down_rate: f64,
    pub up_rate: f64,
    pub down_total: f64,
    pub up_total: f64,
}

/// Velocidad de la red: la diferencia de bytes entre dos lecturas.
#[derive(Default)]
pub struct NetMeter {
    last: Option<(String, u64, u64, Instant)>,
}

/// La interfaz pedida o, en automático, la que más tráfico lleva.
fn pick(list: Vec<Interface>, wanted: &str) -> Option<Interface> {
    if wanted.eq_ignore_ascii_case("auto") {
        list.into_iter().max_by_key(|i| i.received + i.sent)
    } else {
        list.into_iter().find(|i| i.name.eq_ignore_ascii_case(wanted))
    }
}

impl NetMeter {
    pub fn sample(&mut self, wanted: &str) -> Option<Traffic> {
        let now = pick(interfaces(), wanted)?;
        let at = Instant::now();
        let mut traffic =
            Traffic { down_rate: 0.0, up_rate: 0.0, down_total: now.received as f64, up_total: now.sent as f64 };
        if let Some((name, received, sent, when)) = self.last.take()
            && name == now.name
        {
            let secs = at.duration_since(when).as_secs_f64().max(0.001);
            traffic.down_rate = now.received.saturating_sub(received) as f64 / secs;
            traffic.up_rate = now.sent.saturating_sub(sent) as f64 / secs;
        }
        self.last = Some((now.name, now.received, now.sent, at));
        Some(traffic)
    }
}

/// Latencia a un host en milisegundos. `None` si no responde.
pub fn ping(host: &str) -> Option<f64> {
    let address = (host, 0u16).to_socket_addrs().ok()?.find_map(|a| match a {
        SocketAddr::V4(v4) => Some(*v4.ip()),
        SocketAddr::V6(_) => None,
    })?;
    unsafe {
        let handle = IcmpCreateFile();
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let payload = [0u8; 8];
        let mut reply = [0u8; 128];
        let answers = IcmpSendEcho(
            handle,
            u32::from(address).to_be(),
            payload.as_ptr() as *const _,
            payload.len() as u16,
            null(),
            reply.as_mut_ptr() as *mut _,
            reply.len() as u32,
            PING_TIMEOUT_MS,
        );
        IcmpCloseHandle(handle);
        if answers == 0 {
            return None;
        }
        let echo = std::ptr::read_unaligned(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (echo.Status == 0).then_some(echo.RoundTripTime as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_load_needs_two_readings_and_stays_in_range() {
        let mut cpu = CpuLoad::default();
        assert_eq!(cpu.sample(), None);
        let mut spin = 0u64;
        for i in 0..50_000_000u64 {
            spin = spin.wrapping_add(i * i);
        }
        std::hint::black_box(spin);
        let load = cpu.sample().expect("segunda lectura");
        assert!((0.0..=100.0).contains(&load));
    }

    #[test]
    fn memory_and_disk_read_plausible_values() {
        let m = memory().expect("memoria");
        assert!(m.percent > 0.0 && m.percent <= 100.0 && m.used_gb > 0.0);
        let d = disk().expect("disco");
        assert!(d.percent > 0.0 && d.percent <= 100.0 && d.free_gb > 0.0);
    }

    #[test]
    fn uptime_only_goes_forward() {
        assert!(uptime_secs() > 0);
    }

    #[test]
    fn auto_chooses_the_busiest_interface() {
        let iface = |name: &str, received| Interface { name: name.into(), received, sent: 0 };
        let list = vec![iface("Wi-Fi", 10), iface("Ethernet", 500)];
        assert_eq!(pick(list.clone(), "auto").unwrap().name, "Ethernet");
        assert_eq!(pick(list.clone(), "wi-fi").unwrap().name, "Wi-Fi");
        assert_eq!(pick(list, "No existe"), None);
    }

    #[test]
    fn the_first_network_reading_reports_no_speed() {
        let mut meter = NetMeter::default();
        if let Some(first) = meter.sample("auto") {
            assert_eq!(first.down_rate, 0.0);
        }
    }

    #[test]
    fn the_local_machine_answers_a_ping() {
        assert!(ping("127.0.0.1").is_some());
    }
}
