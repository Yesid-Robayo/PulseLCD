//! Todo lo que se puede mostrar en la pantalla. Un `Metric` es un dato; un `Value`, su lectura actual. Solo aquí se
//! decide cómo se escribe cada dato, para que el panel, la vista previa y la tabla de sensores digan lo mismo.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::i18n::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    CpuUsage,
    CpuTemp,
    GpuUsage,
    GpuTemp,
    GpuMemoryUsed,
    GpuMemoryPercent,
    GpuFan,
    GpuClock,
    Fps,
    RamPercent,
    RamUsed,
    SwapPercent,
    DiskPercent,
    DiskUsed,
    DiskFree,
    NetDown,
    NetUp,
    NetDownTotal,
    NetUpTotal,
    Ping,
    Uptime,
    Time,
    Date,
    Weekday,
    WeatherTemp,
    WeatherFeels,
    WeatherHumidity,
    WeatherText,
}

/// Periférico con batería, para las alertas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Device {
    Headset,
    Mouse,
    Keyboard,
    Gamepad,
}

/// Una lectura. `Missing` no es cero: el sensor no está o aún no respondió, y así debe verse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    #[default]
    Missing,
    Number(f64),
    Text(String),
}

impl Value {
    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) if n.is_finite() => Some(*n),
            _ => None,
        }
    }
}

impl Metric {
    pub const ALL: [Metric; 28] = [
        Metric::CpuUsage,
        Metric::CpuTemp,
        Metric::GpuUsage,
        Metric::GpuTemp,
        Metric::GpuMemoryUsed,
        Metric::GpuMemoryPercent,
        Metric::GpuFan,
        Metric::GpuClock,
        Metric::Fps,
        Metric::RamPercent,
        Metric::RamUsed,
        Metric::SwapPercent,
        Metric::DiskPercent,
        Metric::DiskUsed,
        Metric::DiskFree,
        Metric::NetDown,
        Metric::NetUp,
        Metric::NetDownTotal,
        Metric::NetUpTotal,
        Metric::Ping,
        Metric::Uptime,
        Metric::Time,
        Metric::Date,
        Metric::Weekday,
        Metric::WeatherTemp,
        Metric::WeatherFeels,
        Metric::WeatherHumidity,
        Metric::WeatherText,
    ];

    /// Rango natural del dato, para barras, anillos y gráficas. `None` si no tiene un máximo fijo.
    pub fn range(self) -> Option<(f64, f64)> {
        match self {
            Metric::CpuUsage
            | Metric::GpuUsage
            | Metric::GpuMemoryPercent
            | Metric::GpuFan
            | Metric::RamPercent
            | Metric::SwapPercent
            | Metric::DiskPercent
            | Metric::WeatherHumidity => Some((0.0, 100.0)),
            Metric::CpuTemp | Metric::GpuTemp => Some((20.0, 100.0)),
            Metric::Ping => Some((0.0, 200.0)),
            Metric::Fps => Some((0.0, 240.0)),
            _ => None,
        }
    }

    pub fn is_weather(self) -> bool {
        matches!(self, Metric::WeatherTemp | Metric::WeatherFeels | Metric::WeatherHumidity | Metric::WeatherText)
    }

    /// El dato se puede dibujar como barra, anillo o gráfica.
    pub fn is_numeric(self) -> bool {
        !matches!(self, Metric::Time | Metric::Date | Metric::Weekday | Metric::WeatherText | Metric::Uptime)
    }

    /// Texto ya formateado. Lo que falta se escribe `--`, nunca 0.
    pub fn format(self, value: &Value, unit: bool) -> String {
        let with = |n: String, u: &str| if unit { format!("{n}{u}") } else { n };
        let number = match value {
            Value::Missing => return "--".into(),
            Value::Text(t) => return t.clone(),
            Value::Number(n) if !n.is_finite() => return "--".into(),
            Value::Number(n) => *n,
        };
        match self {
            Metric::CpuUsage
            | Metric::GpuUsage
            | Metric::GpuMemoryPercent
            | Metric::GpuFan
            | Metric::RamPercent
            | Metric::SwapPercent
            | Metric::DiskPercent
            | Metric::WeatherHumidity => with(format!("{}", number.round() as i64), " %"),
            Metric::CpuTemp | Metric::GpuTemp => with(format!("{}", number.round() as i64), "°C"),
            Metric::WeatherTemp | Metric::WeatherFeels => with(format!("{number:.0}"), "°"),
            Metric::GpuMemoryUsed => with(format!("{}", number.round() as i64), " MB"),
            Metric::GpuClock => with(format!("{number:.2}"), " GHz"),
            Metric::Fps => with(format!("{}", number.round() as i64), " FPS"),
            Metric::RamUsed | Metric::DiskUsed | Metric::DiskFree => with(format!("{number:.0}"), " GB"),
            Metric::NetDown | Metric::NetUp => bytes_text(number, true),
            Metric::NetDownTotal | Metric::NetUpTotal => bytes_text(number, false),
            Metric::Ping => with(format!("{}", number.round() as i64), " ms"),
            Metric::Uptime => uptime_text(number as u64),
            Metric::Time | Metric::Date | Metric::Weekday | Metric::WeatherText => "--".into(),
        }
    }

    /// Valor de ejemplo para la galería, el editor y las pantallas sin panel.
    pub fn sample(self) -> Value {
        match self {
            Metric::CpuUsage => Value::Number(42.0),
            Metric::CpuTemp => Value::Number(58.0),
            Metric::GpuUsage => Value::Number(76.0),
            Metric::GpuTemp => Value::Number(64.0),
            Metric::GpuMemoryUsed => Value::Number(9360.0),
            Metric::GpuMemoryPercent => Value::Number(58.0),
            Metric::GpuFan => Value::Number(45.0),
            Metric::GpuClock => Value::Number(2.68),
            Metric::Fps => Value::Number(144.0),
            Metric::RamPercent => Value::Number(49.0),
            Metric::RamUsed => Value::Number(31.0),
            Metric::SwapPercent => Value::Number(12.0),
            Metric::DiskPercent => Value::Number(63.0),
            Metric::DiskUsed => Value::Number(1180.0),
            Metric::DiskFree => Value::Number(680.0),
            Metric::NetDown => Value::Number(4_200_000.0),
            Metric::NetUp => Value::Number(380_000.0),
            Metric::NetDownTotal => Value::Number(52_000_000_000.0),
            Metric::NetUpTotal => Value::Number(6_100_000_000.0),
            Metric::Ping => Value::Number(14.0),
            Metric::Uptime => Value::Number(190_800.0),
            Metric::Time => Value::Text("10:36 AM".into()),
            Metric::Date => Value::Text(tr("31 oct 2026", "Oct 31, 2026").into()),
            Metric::Weekday => Value::Text(tr("Sábado", "Saturday").into()),
            Metric::WeatherTemp => Value::Number(17.0),
            Metric::WeatherFeels => Value::Number(15.0),
            Metric::WeatherHumidity => Value::Number(68.0),
            Metric::WeatherText => Value::Text(tr("Nublado", "Cloudy").into()),
        }
    }
}

/// Bytes en unidades legibles; `per_second` añade `/s`.
pub fn bytes_text(bytes: f64, per_second: bool) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes.max(0.0);
    let mut i = 0;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    let decimals = if i == 0 || v >= 100.0 { 0 } else { 1 };
    format!("{v:.decimals$} {}{}", UNITS[i], if per_second { "/s" } else { "" })
}

fn uptime_text(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs % 86_400) / 3_600, (secs % 3_600) / 60);
    if d > 0 { format!("{d}d {h:02}:{m:02}") } else { format!("{h:02}:{m:02}") }
}

/// Las lecturas de un instante.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Readings {
    pub values: BTreeMap<Metric, Value>,
}

impl Readings {
    pub fn get(&self, metric: Metric) -> &Value {
        static MISSING: Value = Value::Missing;
        self.values.get(&metric).unwrap_or(&MISSING)
    }

    pub fn set(&mut self, metric: Metric, value: Value) {
        self.values.insert(metric, value);
    }

    /// Guarda un número, o `Missing` si no hay lectura.
    pub fn set_number(&mut self, metric: Metric, value: Option<f64>) {
        self.set(metric, value.filter(|n| n.is_finite()).map_or(Value::Missing, Value::Number));
    }

    pub fn samples() -> Self {
        Self { values: Metric::ALL.iter().map(|m| (*m, m.sample())).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_sensor_never_reads_as_zero() {
        for m in Metric::ALL {
            assert_eq!(m.format(&Value::Missing, true), "--", "{m:?}");
        }
    }

    #[test]
    fn every_metric_has_a_sample_and_all_lists_each_once() {
        let unique: std::collections::BTreeSet<_> = Metric::ALL.iter().collect();
        assert_eq!(unique.len(), Metric::ALL.len());
        assert!(Metric::ALL.iter().all(|m| m.sample() != Value::Missing));
    }

    #[test]
    fn traffic_is_written_in_readable_units() {
        assert_eq!(bytes_text(512.0, true), "512 B/s");
        assert_eq!(bytes_text(1536.0, true), "1.5 KB/s");
        assert_eq!(bytes_text(5_368_709_120.0, false), "5.0 GB");
    }

    #[test]
    fn uptime_shows_days_only_when_there_are_any() {
        assert_eq!(uptime_text(3_660), "01:01");
        assert_eq!(uptime_text(90_000), "1d 01:00");
    }
}
