//! Aviso de versión nueva, solo si el usuario lo activa. Pregunta a GitHub cuál es la última versión y nada más: no descarga
//! ni instala nada. De la respuesta solo se usa un número, y la página que se abre después es siempre la del repositorio.

use pulse_sensors::http;
use serde_json::Value;

const LATEST: &str = "https://api.github.com/repos/Yesid-Robayo/PulseLCD/releases/latest";
const TIMEOUT_MS: i32 = 8_000;

/// `0.2.0` o `v0.2.0`. Un número con letras, como `0.2.0-beta`, no cuenta como versión publicada.
fn version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.').map(|part| part.parse::<u32>().ok());
    let found = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(found)
}

fn newer(current: &str, answer: &str) -> Option<String> {
    let answer: Value = serde_json::from_str(answer).ok()?;
    let (latest, current) = (version(answer.get("tag_name")?.as_str()?)?, version(current)?);
    (latest > current).then(|| format!("{}.{}.{}", latest.0, latest.1, latest.2))
}

/// La versión publicada, si es más nueva que esta. Sin conexión o sin una respuesta válida simplemente no hay aviso.
pub fn newer_version() -> Option<String> {
    newer(pulse_shared::VERSION, &http::get(LATEST, TIMEOUT_MS).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_version_is_announced() {
        assert_eq!(newer("0.1.0", r#"{"tag_name":"v0.2.0"}"#).as_deref(), Some("0.2.0"));
        assert_eq!(newer("0.9.0", r#"{"tag_name":"v0.10.0"}"#).as_deref(), Some("0.10.0"));
        assert_eq!(newer("1.2.3", r#"{"tag_name":"2.0.0"}"#).as_deref(), Some("2.0.0"));
    }

    #[test]
    fn the_same_or_an_older_version_is_not() {
        assert_eq!(newer("0.2.0", r#"{"tag_name":"v0.2.0"}"#), None);
        assert_eq!(newer("0.2.0", r#"{"tag_name":"v0.1.9"}"#), None);
    }

    #[test]
    fn anything_that_is_not_a_published_version_is_ignored() {
        for answer in [
            r#"{"message":"Not Found"}"#,
            r#"{"tag_name":"v0.2.0-beta"}"#,
            r#"{"tag_name":"v0.2"}"#,
            r#"{"tag_name":"v0.2.0.1"}"#,
            r#"{"tag_name":"<img src=x>"}"#,
            r#"{"tag_name":7}"#,
            "no es json",
            "",
        ] {
            assert_eq!(newer("0.1.0", answer), None, "{answer}");
        }
    }

    #[test]
    fn what_is_shown_is_rebuilt_from_the_numbers() {
        assert_eq!(newer("0.1.0", r#"{"tag_name":"v00.02.00"}"#).as_deref(), Some("0.2.0"));
    }

    #[test]
    fn the_current_version_can_be_compared() {
        assert!(version(pulse_shared::VERSION).is_some(), "{}", pulse_shared::VERSION);
    }
}
