//! Temas incluidos. Son diseños originales: unos solo con degradados y formas, otros con fondos que genera
//! `scripts/make_backgrounds.py`, sin imágenes de nadie. Se guardan como JSON, igual que los del usuario, para que sirvan de
//! ejemplo del formato.

use pulse_shared::theme::Theme;

macro_rules! themes {
    ($($id:literal),* $(,)?) => {
        /// Identificadores de los temas incluidos, en el orden de la galería.
        pub const BUILTIN: [&str; [$($id),*].len()] = [$($id),*];
        const SOURCES: [&str; BUILTIN.len()] = [$(include_str!(concat!("../../../themes/", $id, ".json"))),*];
    };
}

macro_rules! images {
    ($($file:literal),* $(,)?) => {
        const IMAGES: [(&str, &[u8]); [$($file),*].len()] =
            [$(($file, include_bytes!(concat!("../../../themes/images/", $file)))),*];
    };
}

themes!(
    "pulse",
    "signal",
    "arcade",
    "orbit",
    "synthwave",
    "retro",
    "cyberpunk",
    "sakura",
    "dungeon",
    "mars",
    "reef",
    "blocks",
    "blackhole",
    "aurora",
    "cavern",
    "saturn",
    "forest",
    "rain",
    "earthrise",
    "nebula",
    "galaxy",
);

images!(
    "orbit.jpg",
    "synthwave.jpg",
    "retro.png",
    "cyberpunk.jpg",
    "sakura.jpg",
    "dungeon.jpg",
    "mars.jpg",
    "reef.jpg",
    "blocks.jpg",
    "blackhole.jpg",
    "aurora.jpg",
    "cavern.jpg",
    "saturn.jpg",
    "forest.jpg",
    "rain.jpg",
    "earthrise.jpg",
    "nebula.jpg",
    "galaxy.jpg",
);

pub fn builtin_theme(id: &str) -> Option<Theme> {
    let index = BUILTIN.iter().position(|known| *known == id)?;
    serde_json::from_str(SOURCES[index]).ok()
}

/// Fondo de un tema incluido, por el nombre que aparece en su JSON.
pub fn builtin_image(file: &str) -> Option<&'static [u8]> {
    IMAGES.iter().find(|(name, _)| *name == file).map(|(_, bytes)| *bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_shared::theme::Background;

    #[test]
    fn every_builtin_theme_loads_and_is_valid() {
        for id in BUILTIN {
            let theme = builtin_theme(id).unwrap_or_else(|| panic!("{id} no carga"));
            theme.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }

    #[test]
    fn every_builtin_background_is_inside_the_app() {
        for id in BUILTIN {
            if let Background::Image { file } = builtin_theme(id).unwrap().background {
                assert!(builtin_image(&file).is_some(), "{id}: falta {file}");
            }
        }
    }

    #[test]
    fn no_image_is_left_unused() {
        for (file, _) in IMAGES {
            let used = BUILTIN.iter().any(
                |id| matches!(builtin_theme(id).unwrap().background, Background::Image { file: used } if used == file),
            );
            assert!(used, "{file}");
        }
    }

    #[test]
    fn the_themes_with_weather_are_recognised() {
        assert!(builtin_theme("forest").unwrap().shows_weather());
        assert!(!builtin_theme("orbit").unwrap().shows_weather());
    }

    #[test]
    fn an_unknown_theme_or_image_is_not_found() {
        assert!(builtin_theme("nada").is_none());
        assert!(builtin_image("nada.png").is_none());
    }
}
