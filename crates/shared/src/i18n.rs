//! Idioma de los textos que salen de Rust: bandeja, avisos, días y meses. La ventana lleva su propio diccionario.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Spanish,
    English,
}

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// Aplica el idioma a todo el proceso.
pub fn set(language: Language) {
    ENGLISH.store(language == Language::English, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

pub fn tr(es: &'static str, en: &'static str) -> &'static str {
    if is_english() { en } else { es }
}

/// `format!` en los dos idiomas.
#[macro_export]
macro_rules! trf {
    ($es:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_english() { format!($en $(, $arg)*) } else { format!($es $(, $arg)*) }
    };
}
