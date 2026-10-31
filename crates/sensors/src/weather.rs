//! Clima con Open-Meteo: gratis, sin cuenta y sin clave. Solo se consulta si el tema muestra el clima, y lo único que
//! sale del PC son las coordenadas que el usuario eligió.

use pulse_shared::config::WeatherConfig;
use pulse_shared::i18n::{is_english, tr};
use serde::{Deserialize, Serialize};

use crate::http;

const TIMEOUT_MS: i32 = 8000;
const MAX_RESULTS: u8 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub temp: f64,
    pub feels: f64,
    pub humidity: f64,
    pub text: String,
}

#[derive(Deserialize)]
struct Answer {
    current: Current,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    weather_code: u32,
}

/// Texto del código meteorológico de la OMM.
fn description(code: u32) -> &'static str {
    match code {
        0 => tr("Despejado", "Clear"),
        1 | 2 => tr("Poco nuboso", "Partly cloudy"),
        3 => tr("Nublado", "Cloudy"),
        45 | 48 => tr("Niebla", "Fog"),
        51..=57 => tr("Llovizna", "Drizzle"),
        61..=67 | 80..=82 => tr("Lluvia", "Rain"),
        71..=77 | 85 | 86 => tr("Nieve", "Snow"),
        95..=99 => tr("Tormenta", "Storm"),
        _ => tr("Variable", "Variable"),
    }
}

fn parse(text: &str) -> Option<Weather> {
    let answer: Answer = serde_json::from_str(text).ok()?;
    let c = answer.current;
    Some(Weather {
        temp: c.temperature_2m,
        feels: c.apparent_temperature,
        humidity: c.relative_humidity_2m,
        text: description(c.weather_code).into(),
    })
}

pub fn fetch(config: &WeatherConfig) -> Option<Weather> {
    let unit = if config.fahrenheit { "fahrenheit" } else { "celsius" };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code&temperature_unit={unit}",
        config.latitude, config.longitude
    );
    parse(&http::get(&url, TIMEOUT_MS).ok()?)
}

/// Una ciudad encontrada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    #[serde(default)]
    pub admin1: String,
    #[serde(default)]
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Deserialize, Default)]
struct Search {
    #[serde(default)]
    results: Vec<Place>,
}

/// Busca ciudades por nombre.
pub fn search(name: &str) -> Result<Vec<Place>, String> {
    let language = if is_english() { "en" } else { "es" };
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count={MAX_RESULTS}&language={language}&format=json",
        http::escape(name.trim())
    );
    let text = http::get(&url, TIMEOUT_MS)?;
    serde_json::from_str::<Search>(&text).map(|s| s.results).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_becomes_a_reading() {
        let text = r#"{"latitude":4.7,"current":{"time":"2026-10-31T10:00","temperature_2m":17.4,
            "apparent_temperature":16.1,"relative_humidity_2m":71,"weather_code":3}}"#;
        let w = parse(text).unwrap();
        assert_eq!((w.temp, w.feels, w.humidity), (17.4, 16.1, 71.0));
        assert_eq!(w.text, tr("Nublado", "Cloudy"));
    }

    #[test]
    fn a_broken_answer_is_ignored() {
        assert_eq!(parse("<html>error</html>"), None);
        assert_eq!(parse(r#"{"error":true}"#), None);
    }

    #[test]
    fn a_search_without_results_is_an_empty_list() {
        assert!(serde_json::from_str::<Search>(r#"{"generationtime_ms":0.5}"#).unwrap().results.is_empty());
    }

    #[test]
    fn every_code_has_a_description() {
        assert!((0..120).all(|c| !description(c).is_empty()));
    }
}
