//! Biblioteca de temas. Los incluidos viven en el exe y nunca se pierden. Cada tema puede tener además su carpeta en
//! `%APPDATA%\PulseLCD\themes`: los del usuario viven ahí, y un incluido guarda ahí lo que se le edite, sin copias y sin
//! tocar el original. Quitar un incluido solo lo oculta (`Config::hidden_themes`), así que restaurar no pisa nada.

use std::path::{Path, PathBuf};

use pulse_render::{BUILTIN, Scene, builtin_image, builtin_theme};
use pulse_shared::paths;
use pulse_shared::theme::{Background, Theme, is_plain_file_name};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    /// Un incluido con cambios guardados: se puede volver al original.
    pub edited: bool,
}

pub fn is_builtin(id: &str) -> bool {
    BUILTIN.contains(&id)
}

/// Carpeta de un tema, exista o no. `None` si el identificador podría salirse de la biblioteca.
pub fn folder(id: &str) -> Option<PathBuf> {
    is_plain_file_name(id).then(|| paths::themes_dir().join(id))
}

fn read(folder: &Path) -> Result<Theme, String> {
    let text = std::fs::read_to_string(folder.join("theme.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Lo guardado manda. Un incluido siempre tiene su original detrás, también si lo guardado se estropea.
fn load_from(library: &Path, id: &str) -> Result<Theme, String> {
    if !is_plain_file_name(id) {
        return Err("identificador no válido".into());
    }
    read(&library.join(id)).or_else(|e| builtin_theme(id).ok_or(e))
}

pub fn load(id: &str) -> Result<Theme, String> {
    load_from(&paths::themes_dir(), id)
}

fn scene_in(library: &Path, id: &str) -> Result<Scene, String> {
    let saved = load_from(library, id).and_then(|theme| Scene::new(theme, Some(library.join(id))));
    // lo guardado puede fallar de formas que solo se ven al abrirlo, como una imagen ilegible: el original sigue debajo
    saved.or_else(|e| Scene::new(builtin_theme(id).ok_or(e)?, None))
}

pub fn scene(id: &str) -> Result<Scene, String> {
    scene_in(&paths::themes_dir(), id)
}

fn list_in(library: &Path, hidden: &[String]) -> Vec<ThemeInfo> {
    let mut all: Vec<ThemeInfo> = BUILTIN
        .iter()
        .filter(|id| !hidden.iter().any(|gone| gone == *id))
        .filter_map(|id| {
            let saved = read(&library.join(id)).ok();
            let edited = saved.is_some();
            let theme = saved.or_else(|| builtin_theme(id))?;
            Some(ThemeInfo { id: id.to_string(), name: theme.name, builtin: true, edited })
        })
        .collect();
    let mut own: Vec<ThemeInfo> = std::fs::read_dir(library)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let theme = read(&entry.path()).ok()?;
            (!is_builtin(&id)).then_some(ThemeInfo { id, name: theme.name, builtin: false, edited: false })
        })
        .collect();
    own.sort_by_key(|t| t.name.to_lowercase());
    all.append(&mut own);
    all
}

/// Todos los temas: primero los incluidos que no se hayan quitado, luego los del usuario por nombre.
pub fn list(hidden: &[String]) -> Vec<ThemeInfo> {
    list_in(&paths::themes_dir(), hidden)
}

/// Los incluidos que el usuario quitó, en el orden de la galería, para elegir cuáles devolver.
pub fn hidden(ids: &[String]) -> Vec<ThemeInfo> {
    BUILTIN
        .iter()
        .filter(|id| ids.iter().any(|gone| gone == *id))
        .filter_map(|id| {
            builtin_theme(id).map(|t| ThemeInfo { id: id.to_string(), name: t.name, builtin: true, edited: false })
        })
        .collect()
}

/// Identificador libre a partir de un nombre.
fn free_id(name: &str) -> String {
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(40)
        .collect();
    let base = if base.is_empty() { "tema".to_string() } else { base };
    let taken = |id: &str| is_builtin(id) || paths::themes_dir().join(id).exists();
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).unwrap_or(base)
}

pub fn save(id: &str, theme: &Theme) -> Result<(), String> {
    let dir = folder(id).ok_or("identificador no válido")?;
    // lo que no se pueda abrir no se guarda: el agente lo intentaría en cada arranque
    Scene::new(theme.clone(), Some(dir.clone()))?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(theme).map_err(|e| e.to_string())?;
    paths::write_atomic(&dir.join("theme.json"), text.as_bytes()).map_err(|e| e.to_string())
}

/// El fondo de un tema incluido vive dentro del exe: su copia necesita el suyo, salvo que ya traiga otro.
fn copy_builtin_background(theme: &Theme, dir: &Path) -> Result<(), String> {
    let Background::Image { file } = &theme.background else { return Ok(()) };
    match builtin_image(file) {
        Some(image) if !dir.join(file).exists() => std::fs::write(dir.join(file), image).map_err(|e| e.to_string()),
        _ => Ok(()),
    }
}

/// Crea un tema nuevo a partir de otro, con sus imágenes y fuentes. Devuelve el identificador nuevo.
pub fn duplicate(id: &str, name: &str) -> Result<String, String> {
    let mut theme = load(id)?;
    theme.name = name.chars().take(40).collect();
    let new_id = free_id(&theme.name);
    let dir = folder(&new_id).ok_or("identificador no válido")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if let Some(source) = folder(id).filter(|f| f.is_dir()) {
        for entry in std::fs::read_dir(source).map_err(|e| e.to_string())?.flatten() {
            if entry.file_name() != "theme.json" && entry.path().is_file() {
                std::fs::copy(entry.path(), dir.join(entry.file_name())).map_err(|e| e.to_string())?;
            }
        }
    }
    copy_builtin_background(&theme, &dir)?;
    save(&new_id, &theme)?;
    Ok(new_id)
}

/// Borra lo guardado de un tema: del todo si es del usuario, y sus cambios si es incluido, que vuelve a ser el original.
/// Solo lo que un tema puede contener: nunca sigue fuera de su carpeta.
pub fn delete(id: &str) -> Result<(), String> {
    let dir = folder(id).ok_or("identificador no válido")?;
    if !is_builtin(id) && !dir.join("theme.json").is_file() {
        return Err("no es un tema".into());
    }
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
}

/// Un archivo elegido no debe pisar otro del tema, ni tapar el fondo de un incluido.
fn free_file_name(dir: &Path, name: &str) -> String {
    let taken = |candidate: &str| dir.join(candidate).exists() || builtin_image(candidate).is_some();
    if !taken(name) {
        return name.to_string();
    }
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..)
        .map(|n| format!("{stem}-{n}.{extension}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_string())
}

/// Copia a la carpeta del tema una imagen o fuente que eligió el usuario. Devuelve el nombre dentro del tema.
pub fn add_file(id: &str, source: &Path) -> Result<String, String> {
    let dir = folder(id).ok_or("identificador no válido")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("archivo no válido")?;
    if !is_plain_file_name(&name) {
        return Err("nombre de archivo no válido".into());
    }
    let name = free_file_name(&dir, &name);
    std::fs::copy(source, dir.join(&name)).map_err(|e| e.to_string())?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Una biblioteca de usar y tirar: las pruebas no deben tocar los temas de nadie.
    struct Library(PathBuf);

    impl Library {
        fn new(name: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("pulselcd-library-{name}-{}-{unique}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, id: &str, text: &str) {
            std::fs::create_dir_all(self.0.join(id)).unwrap();
            std::fs::write(self.0.join(id).join("theme.json"), text).unwrap();
        }

        fn save(&self, id: &str, name: &str) {
            let mut theme = builtin_theme("pulse").unwrap();
            theme.name = name.into();
            self.write(id, &serde_json::to_string(&theme).unwrap());
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn info<'a>(list: &'a [ThemeInfo], id: &str) -> Option<&'a ThemeInfo> {
        list.iter().find(|theme| theme.id == id)
    }

    #[test]
    fn ids_cannot_escape_the_library() {
        assert_eq!(folder("..\\..\\Windows"), None);
        assert_eq!(folder("a/b"), None);
        assert!(delete("..\\config").is_err());
        assert!(load_from(Path::new("."), "..\\config").is_err());
    }

    #[test]
    fn a_builtin_theme_needs_nothing_on_disk() {
        let library = Library::new("empty");
        assert_eq!(load_from(&library.0, "orbit").unwrap().name, "Orbit");
        assert!(!info(&list_in(&library.0, &[]), "orbit").unwrap().edited);
    }

    #[test]
    fn editing_a_builtin_theme_keeps_its_id_and_creates_no_copy() {
        let library = Library::new("edited");
        library.save("orbit", "Mi órbita");
        assert_eq!(load_from(&library.0, "orbit").unwrap().name, "Mi órbita");
        let list = list_in(&library.0, &[]);
        let edited = info(&list, "orbit").unwrap();
        assert!(edited.builtin && edited.edited);
        assert_eq!(edited.name, "Mi órbita");
        assert_eq!(list.len(), BUILTIN.len());
    }

    #[test]
    fn a_broken_edit_falls_back_to_the_original() {
        let library = Library::new("broken");
        library.write("orbit", "esto no es json");
        assert_eq!(load_from(&library.0, "orbit").unwrap().name, "Orbit");
    }

    #[test]
    fn a_saved_theme_that_cannot_open_falls_back_to_the_original() {
        let library = Library::new("no-image");
        for id in ["pulse", "orbit"] {
            let mut theme = builtin_theme(id).unwrap();
            theme.background = Background::Image { file: "no-esta.png".into() };
            library.write(id, &serde_json::to_string(&theme).unwrap());
            assert!(scene_in(&library.0, id).is_ok(), "{id}");
        }
    }

    #[test]
    fn a_broken_user_theme_is_reported_instead_of_replaced() {
        let library = Library::new("broken-own");
        library.write("mio", "esto no es json");
        assert!(scene_in(&library.0, "mio").is_err());
    }

    #[test]
    fn a_picked_file_never_replaces_another() {
        let library = Library::new("names");
        std::fs::write(library.0.join("fondo.png"), "mía").unwrap();
        assert_eq!(free_file_name(&library.0, "nuevo.png"), "nuevo.png");
        assert_eq!(free_file_name(&library.0, "fondo.png"), "fondo-2.png");
        std::fs::write(library.0.join("fondo-2.png"), "otra").unwrap();
        assert_eq!(free_file_name(&library.0, "fondo.png"), "fondo-3.png");
    }

    #[test]
    fn a_picked_file_never_hides_a_builtin_background() {
        let library = Library::new("hides");
        assert_eq!(free_file_name(&library.0, "orbit.jpg"), "orbit-2.jpg");
    }

    #[test]
    fn removed_themes_leave_the_list_and_the_rest_stay() {
        let library = Library::new("hidden");
        library.save("mio", "Mío");
        let list = list_in(&library.0, &["orbit".to_string()]);
        assert!(info(&list, "orbit").is_none());
        assert!(info(&list, "mars").is_some());
        assert!(info(&list, "mio").is_some());
    }

    #[test]
    fn restoring_brings_themes_back_without_touching_what_exists() {
        let library = Library::new("restore");
        library.save("mio", "Mío");
        library.save("mars", "Mi Marte");
        let hidden = ["orbit".to_string(), "reef".to_string()];
        assert_eq!(list_in(&library.0, &hidden).len(), BUILTIN.len() - 1);
        let restored = list_in(&library.0, &[]);
        assert_eq!(restored.len(), BUILTIN.len() + 1);
        assert_eq!(info(&restored, "mars").unwrap().name, "Mi Marte");
        assert_eq!(info(&restored, "mio").unwrap().name, "Mío");
        assert!(!info(&restored, "orbit").unwrap().edited);
    }

    #[test]
    fn the_removed_themes_can_be_listed_to_choose_from() {
        let gone = ["reef".to_string(), "orbit".to_string(), "ya-no-existe".to_string()];
        let names: Vec<String> = hidden(&gone).into_iter().map(|theme| theme.id).collect();
        assert_eq!(names, ["orbit", "reef"], "en el orden de la galería y sin ids desconocidos");
    }

    #[test]
    fn restoring_only_some_leaves_the_others_removed() {
        let library = Library::new("partial");
        let still_gone = ["reef".to_string()];
        let list = list_in(&library.0, &still_gone);
        assert!(info(&list, "orbit").is_some());
        assert!(info(&list, "reef").is_none());
    }

    #[test]
    fn a_copy_of_a_builtin_theme_keeps_its_background() {
        let library = Library::new("copy");
        copy_builtin_background(&builtin_theme("orbit").unwrap(), &library.0).unwrap();
        assert!(library.0.join("orbit.jpg").is_file());
    }

    #[test]
    fn a_background_the_copy_already_has_is_not_replaced() {
        let library = Library::new("own-image");
        std::fs::write(library.0.join("orbit.jpg"), "mía").unwrap();
        copy_builtin_background(&builtin_theme("orbit").unwrap(), &library.0).unwrap();
        assert_eq!(std::fs::read_to_string(library.0.join("orbit.jpg")).unwrap(), "mía");
    }

    #[test]
    fn a_builtin_theme_without_an_image_copies_nothing() {
        let library = Library::new("no-image");
        copy_builtin_background(&builtin_theme("pulse").unwrap(), &library.0).unwrap();
        assert_eq!(std::fs::read_dir(&library.0).unwrap().count(), 0);
    }

    #[test]
    fn a_name_becomes_a_safe_id() {
        let id = free_id("Mi Tema: Ñandú / 2");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{id}");
        assert!(is_plain_file_name(&id));
    }

    #[test]
    fn the_builtin_names_are_never_taken_by_a_copy() {
        assert_ne!(free_id("pulse"), "pulse");
    }
}
