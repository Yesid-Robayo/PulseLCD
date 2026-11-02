//! Configuración en `%APPDATA%\PulseLCD\config.json`. El agente es su único escritor: la ventana le pide los cambios.

use serde::{Deserialize, Serialize};

use crate::i18n::Language;

const MAX_IGNORED: usize = 256;
const MAX_TEXT: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// El mismo que Windows.
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    /// `auto` o un puerto como `COM5`.
    pub port: String,
    /// 0..=100.
    pub brightness: u8,
    /// Girar la imagen 180°, según cómo esté colocado el panel.
    pub flipped: bool,
    /// Identificador del tema en uso.
    pub theme: String,
    /// Apagar la pantalla al apagar, suspender o salir. Si no, se queda encendida sin datos.
    pub off_when_idle: bool,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self { port: "auto".into(), brightness: 50, flipped: false, theme: "pulse".into(), off_when_idle: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertConfig {
    /// Grados en los que avisar; 0 lo desactiva.
    pub cpu_temp: u8,
    pub gpu_temp: u8,
    /// Leer las baterías de YesCharge u otra fuente compatible.
    pub battery: bool,
    pub battery_url: String,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self { cpu_temp: 90, gpu_temp: 85, battery: false, battery_url: "http://127.0.0.1:8765/v1/status".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WeatherConfig {
    pub enabled: bool,
    /// Nombre de la ciudad, solo para mostrarlo.
    pub place: String,
    pub latitude: f64,
    pub longitude: f64,
    pub fahrenheit: bool,
}

impl WeatherConfig {
    /// Activado y con ciudad. Sin ciudad las coordenadas son 0, 0: en medio del mar, y parecería el clima de aquí.
    pub fn is_ready(&self) -> bool {
        self.enabled && !self.place.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FpsConfig {
    /// Programas que no son juegos, además de la lista incluida.
    pub ignored: Vec<String>,
    /// Contar los cuadros de Frame Generation, como hacen los overlays.
    pub count_generated: bool,
}

impl Default for FpsConfig {
    fn default() -> Self {
        Self { ignored: Vec::new(), count_generated: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MaintenanceConfig {
    /// Días entre limpiezas automáticas; 0 las desactiva.
    pub deghost_days: u16,
    /// Fecha de la última limpieza (segundos desde 1970).
    pub last_deghost: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub language: Language,
    pub appearance: Appearance,
    pub clock_24h: bool,
    pub start_with_windows: bool,
    /// Preguntar a GitHub si hay una versión nueva. Viene apagado: sin esto, PulseLCD no contacta con GitHub.
    pub check_updates: bool,
    /// Ya se pasó la bienvenida.
    pub welcomed: bool,
    /// Temas incluidos que el usuario quitó. Siguen en el exe: vaciar esta lista los devuelve.
    pub hidden_themes: Vec<String>,
    pub panel: PanelConfig,
    /// `auto` o el nombre de una interfaz de red.
    pub network: String,
    /// Host para medir la latencia; vacío la desactiva.
    pub ping_host: String,
    pub alerts: AlertConfig,
    pub weather: WeatherConfig,
    pub fps: FpsConfig,
    pub maintenance: MaintenanceConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::Spanish,
            appearance: Appearance::System,
            clock_24h: false,
            start_with_windows: false,
            check_updates: false,
            welcomed: false,
            hidden_themes: Vec::new(),
            panel: PanelConfig::default(),
            network: "auto".into(),
            ping_host: "1.1.1.1".into(),
            alerts: AlertConfig::default(),
            weather: WeatherConfig::default(),
            fps: FpsConfig::default(),
            maintenance: MaintenanceConfig::default(),
        }
    }
}

fn clipped(text: &mut String) {
    if text.chars().count() > MAX_TEXT {
        *text = text.chars().take(MAX_TEXT).collect();
    }
}

impl Config {
    pub fn load() -> Self {
        let path = crate::paths::config_file();
        let Ok(text) = std::fs::read_to_string(&path) else { return Self::default() };
        match serde_json::from_str::<Self>(&text) {
            Ok(mut cfg) => {
                cfg.sanitize();
                cfg
            }
            Err(e) => {
                // se deja una copia: el próximo guardado sobrescribiría lo que el usuario tenía
                crate::log::warn(&format!("config.json no se pudo leer: {e}"));
                let _ = std::fs::copy(&path, path.with_file_name("config.bad.json"));
                Self::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        crate::paths::write_atomic(&crate::paths::config_file(), text.as_bytes())
    }

    /// Deja cada valor dentro de lo razonable, venga de donde venga el archivo.
    pub fn sanitize(&mut self) {
        self.panel.brightness = self.panel.brightness.min(100);
        self.alerts.cpu_temp = self.alerts.cpu_temp.min(120);
        self.alerts.gpu_temp = self.alerts.gpu_temp.min(120);
        self.maintenance.deghost_days = self.maintenance.deghost_days.min(365);
        self.weather.latitude = self.weather.latitude.clamp(-90.0, 90.0);
        self.weather.longitude = self.weather.longitude.clamp(-180.0, 180.0);
        self.fps.ignored.truncate(MAX_IGNORED);
        self.hidden_themes.truncate(MAX_IGNORED);
        for text in [&mut self.panel.port, &mut self.network, &mut self.ping_host, &mut self.weather.place] {
            clipped(text);
        }
        clipped(&mut self.alerts.battery_url);
        // la dirección de baterías solo puede ser local: sus nombres de dispositivo no deben salir del PC
        if !is_local_url(&self.alerts.battery_url) {
            self.alerts.battery_url = AlertConfig::default().battery_url;
        }
        if !valid_port(&self.panel.port) {
            self.panel.port = "auto".into();
        }
    }
}

/// `auto` o `COM` seguido de un número.
pub fn valid_port(port: &str) -> bool {
    port.eq_ignore_ascii_case("auto")
        || port
            .strip_prefix("COM")
            .is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

fn is_local_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let host = rest.split(['/', ':']).next().unwrap_or("");
    host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weather_needs_a_city_as_well_as_the_switch() {
        let mut weather = WeatherConfig { enabled: true, ..WeatherConfig::default() };
        assert!(!weather.is_ready());
        weather.place = "Bogotá".into();
        assert!(weather.is_ready());
        weather.enabled = false;
        assert!(!weather.is_ready());
    }

    #[test]
    fn updates_are_only_checked_if_the_user_asks() {
        assert!(!Config::default().check_updates);
        let older: Config = serde_json::from_str("{}").unwrap();
        assert!(!older.check_updates);
    }

    #[test]
    fn values_from_a_file_are_kept_within_range() {
        let mut cfg: Config = serde_json::from_str(
            r#"{"panel":{"brightness":250,"port":"\\\\evil\\pipe"},"alerts":{"battery_url":"http://example.com/min"}}"#,
        )
        .unwrap();
        cfg.sanitize();
        assert_eq!(cfg.panel.brightness, 100);
        assert_eq!(cfg.panel.port, "auto");
        assert_eq!(cfg.alerts.battery_url, AlertConfig::default().battery_url);
    }

    #[test]
    fn a_partial_file_fills_in_the_rest() {
        let cfg: Config = serde_json::from_str(r#"{"clock_24h":true}"#).unwrap();
        assert!(cfg.clock_24h);
        assert_eq!(cfg.panel, PanelConfig::default());
    }

    #[test]
    fn ports_must_look_like_ports() {
        assert!(valid_port("COM5") && valid_port("auto"));
        assert!(!valid_port("COM") && !valid_port("COM5/../x") && !valid_port("NUL"));
    }
}
