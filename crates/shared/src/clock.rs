//! Hora local de Windows, con su formato para el log y para la pantalla.

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::i18n::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Now {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    /// 0 = domingo.
    pub weekday: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millis: u16,
}

pub fn now() -> Now {
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    Now {
        year: t.wYear,
        month: t.wMonth,
        day: t.wDay,
        weekday: t.wDayOfWeek,
        hour: t.wHour,
        minute: t.wMinute,
        second: t.wSecond,
        millis: t.wMilliseconds,
    }
}

/// Segundos desde 1970, para fechas guardadas en disco.
pub fn unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Now {
    pub fn stamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millis
        )
    }

    pub fn time_text(&self, h24: bool) -> String {
        if h24 {
            format!("{:02}:{:02}", self.hour, self.minute)
        } else {
            let h = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            format!("{h}:{:02} {}", self.minute, if self.hour < 12 { "AM" } else { "PM" })
        }
    }

    pub fn weekday_text(&self) -> &'static str {
        match self.weekday {
            0 => tr("Domingo", "Sunday"),
            1 => tr("Lunes", "Monday"),
            2 => tr("Martes", "Tuesday"),
            3 => tr("Miércoles", "Wednesday"),
            4 => tr("Jueves", "Thursday"),
            5 => tr("Viernes", "Friday"),
            _ => tr("Sábado", "Saturday"),
        }
    }

    fn month_text(&self) -> &'static str {
        match self.month {
            1 => tr("ene", "Jan"),
            2 => tr("feb", "Feb"),
            3 => tr("mar", "Mar"),
            4 => tr("abr", "Apr"),
            5 => tr("may", "May"),
            6 => tr("jun", "Jun"),
            7 => tr("jul", "Jul"),
            8 => tr("ago", "Aug"),
            9 => tr("sep", "Sep"),
            10 => tr("oct", "Oct"),
            11 => tr("nov", "Nov"),
            _ => tr("dic", "Dec"),
        }
    }

    /// "31 oct 2026" u "Oct 31, 2026".
    pub fn date_text(&self) -> String {
        if crate::i18n::is_english() {
            format!("{} {}, {}", self.month_text(), self.day, self.year)
        } else {
            format!("{} {} {}", self.day, self.month_text(), self.year)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twelve_hour_clock_handles_noon_and_midnight() {
        let at = |hour| Now { hour, minute: 5, ..Now::default() };
        assert_eq!(at(0).time_text(false), "12:05 AM");
        assert_eq!(at(12).time_text(false), "12:05 PM");
        assert_eq!(at(13).time_text(false), "1:05 PM");
        assert_eq!(at(13).time_text(true), "13:05");
    }
}
