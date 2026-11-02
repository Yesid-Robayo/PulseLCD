//! Lo que la página le pide a Rust. Se sirve por un protocolo propio de la vista web, así que ningún otro programa puede
//! llegar aquí, y cada ruta hace una cosa concreta: la página nunca puede pedir que se ejecute o se abra una ruta libre.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use pulse_panel::discover;
use pulse_render::{AlertView, History, Scene};
use pulse_sensors::{network_interfaces, weather};
use pulse_shared::config::Config;
use pulse_shared::ipc::Request;
use pulse_shared::log;
use pulse_shared::metrics::Readings;
use pulse_shared::paths;
use pulse_shared::theme::{Theme, is_plain_file_name};
use serde_json::{Value, json};
use wry::http::{Method, Response, StatusCode, header};

use super::bridge::Bridge;
use super::dialogs::pick_file;
use crate::autostart::Elevated;
use crate::{autostart, programs, themes, updates};

const KOFI: &str = "https://ko-fi.com/yesidrobayo";
const REPOSITORY: &str = "https://github.com/Yesid-Robayo/PulseLCD";
const PAWNIO_PAGE: &str = "https://pawnio.eu/";
const QUIT_GRACE: std::time::Duration = std::time::Duration::from_millis(400);

const SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'";

/// Fuentes de Windows que se ofrecen en el editor. Se leen del sistema, no se distribuyen.
const SYSTEM_FONTS: [(&str, &str); 11] = [
    ("segoeuil.ttf", "Segoe UI Light"),
    ("segoeuisl.ttf", "Segoe UI Semilight"),
    ("segoeui.ttf", "Segoe UI"),
    ("seguisb.ttf", "Segoe UI Semibold"),
    ("segoeuib.ttf", "Segoe UI Bold"),
    ("bahnschrift.ttf", "Bahnschrift"),
    ("consola.ttf", "Consolas"),
    ("consolab.ttf", "Consolas Bold"),
    ("arial.ttf", "Arial"),
    ("impact.ttf", "Impact"),
    ("verdana.ttf", "Verdana"),
];

struct Asset {
    mime: &'static str,
    body: &'static [u8],
}

fn asset(path: &str) -> Option<Asset> {
    macro_rules! file {
        ($mime:literal, $name:literal) => {
            Some(Asset { mime: $mime, body: include_bytes!(concat!("../../web/", $name)) })
        };
    }
    match path {
        "/" | "/index.html" => file!("text/html; charset=utf-8", "index.html"),
        "/style.css" => file!("text/css; charset=utf-8", "style.css"),
        "/app.js" => file!("text/javascript; charset=utf-8", "app.js"),
        "/pages.js" => file!("text/javascript; charset=utf-8", "pages.js"),
        "/editor.js" => file!("text/javascript; charset=utf-8", "editor.js"),
        "/strings.js" => file!("text/javascript; charset=utf-8", "strings.js"),
        _ => None,
    }
}

type Reply = Response<Cow<'static, [u8]>>;

fn reply(status: StatusCode, mime: &str, body: Vec<u8>) -> Reply {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_SECURITY_POLICY, SECURITY_POLICY)
        .header("X-Content-Type-Options", "nosniff")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Owned(Vec::new())))
}

fn elevated_reply(result: Elevated) -> Reply {
    let error = match result {
        Elevated::Done => None,
        Elevated::Denied => Some("denied"),
        Elevated::OtherAccount => Some("other_account"),
        Elevated::Failed => Some("failed"),
    };
    json_reply(json!({ "ok": result == Elevated::Done, "error": error }))
}

fn json_reply(value: Value) -> Reply {
    reply(StatusCode::OK, "application/json", value.to_string().into_bytes())
}

fn failure(message: impl Into<String>) -> Reply {
    reply(StatusCode::BAD_REQUEST, "application/json", json!({ "error": message.into() }).to_string().into_bytes())
}

fn query(uri: &wry::http::Uri) -> HashMap<String, String> {
    let text = uri.query().unwrap_or("");
    text.split('&').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (k.to_string(), unescape(v))).collect()
}

/// Deshace el escape de `encodeURIComponent`.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// BMP de 16 bits a partir de RGB565: el navegador lo muestra sin que haya que comprimir nada.
pub fn bmp565(width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
    const HEADER: u32 = 14 + 40 + 12;
    let row = width as usize * 2;
    let padded = row.next_multiple_of(4);
    let size = padded * height as usize;
    let mut out = Vec::with_capacity(HEADER as usize + size);
    out.extend(b"BM");
    out.extend((HEADER + size as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(HEADER.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((width as i32).to_le_bytes());
    out.extend((-(height as i32)).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(3u32.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend([0u8; 16]);
    for mask in [0xf800u32, 0x07e0, 0x001f] {
        out.extend(mask.to_le_bytes());
    }
    for line in pixels.chunks_exact(row.max(1)) {
        out.extend(line);
        out.extend(std::iter::repeat_n(0u8, padded - row));
    }
    out
}

fn frame_bmp(frame: &pulse_panel::Frame) -> Reply {
    reply(StatusCode::OK, "image/bmp", bmp565(frame.width, frame.height, &frame.bytes_of(frame.full())))
}

pub struct Api {
    bridge: Bridge,
    /// Ventana de Windows, dueña de los diálogos.
    owner: Mutex<isize>,
    /// Escena del editor: se reutiliza para que arrastrar sea fluido.
    editing: Mutex<Option<(String, Scene)>>,
}

impl Api {
    pub fn new(bridge: Bridge) -> Self {
        Self { bridge, owner: Mutex::new(0), editing: Mutex::new(None) }
    }

    pub fn set_owner(&self, hwnd: isize) {
        *self.owner.lock().unwrap_or_else(|e| e.into_inner()) = hwnd;
    }

    fn owner(&self) -> isize {
        *self.owner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn config(&self) -> Config {
        let live = self.bridge.live.lock().unwrap_or_else(|e| e.into_inner());
        live.config.clone().unwrap_or_else(Config::load)
    }

    fn readings(&self, live_data: bool) -> Readings {
        let live = self.bridge.live.lock().unwrap_or_else(|e| e.into_inner());
        let current = live.status.as_ref().filter(|_| live_data).map(|s| s.readings.clone());
        current.unwrap_or_else(Readings::samples)
    }

    pub fn handle(&self, request: wry::http::Request<Vec<u8>>) -> Reply {
        let path = request.uri().path().to_string();
        let args = query(request.uri());
        let post = request.method() == Method::POST;
        match (post, path.as_str()) {
            (false, "/api/state") => self.state(),
            (false, "/api/frame") => self.frame(),
            (false, "/api/themes") => json_reply(json!(themes::list(&self.config().hidden_themes))),
            (false, "/api/themes/hidden") => json_reply(json!(themes::hidden(&self.config().hidden_themes))),
            (false, "/api/theme") => self.theme(&args),
            (false, "/api/thumb") => self.thumb(&args),
            (false, "/api/fonts") => self.fonts(&args),
            (false, "/api/places") => self.places(&args),
            (false, "/api/programs") => json_reply(json!(programs::open())),
            (false, "/api/update") => self.update(),
            (true, "/api/preview") => self.preview(&args, request.body()),
            (true, "/api/config") => self.set_config(request.body()),
            (true, "/api/action") => self.action(&args),
            (true, "/api/theme/save") => self.save_theme(&args, request.body()),
            (true, "/api/theme/duplicate") => self.duplicate(&args),
            (true, "/api/theme/delete") => self.delete(&args),
            (true, "/api/theme/revert") => self.revert(&args),
            (true, "/api/theme/pick") => self.pick(&args),
            _ => match asset(&path) {
                Some(file) => reply(StatusCode::OK, file.mime, file.body.to_vec()),
                None => reply(StatusCode::NOT_FOUND, "text/plain", Vec::new()),
            },
        }
    }

    fn state(&self) -> Reply {
        let config = self.config();
        let (connected, status, serial) = {
            let live = self.bridge.live.lock().unwrap_or_else(|e| e.into_inner());
            (live.connected, live.status.clone(), live.mirror.serial)
        };
        let helper = autostart::helper_state_cached();
        let shows_weather = |id: &str| themes::load(id).is_ok_and(|theme| theme.shows_weather());
        let needs_weather = !config.weather.is_ready() && shows_weather(&config.panel.theme);
        let ports: Vec<Value> =
            discover::ports().into_iter().map(|p| json!({ "name": p.name, "panel": p.is_panel })).collect();
        json_reply(json!({
            "version": pulse_shared::VERSION,
            "connected": connected,
            "frame": serial,
            "status": status,
            "config": config,
            "ports": ports,
            "interfaces": network_interfaces(),
            "helper": helper.installed,
            "helper_outdated": helper.installed && !helper.in_place,
            "fps_tool": autostart::fps_tool(),
            "pawnio": pawnio_installed(),
            "theme_needs_weather": needs_weather,
        }))
    }

    fn frame(&self) -> Reply {
        let live = self.bridge.live.lock().unwrap_or_else(|e| e.into_inner());
        let m = &live.mirror;
        if m.pixels.is_empty() {
            return reply(StatusCode::NOT_FOUND, "text/plain", Vec::new());
        }
        reply(StatusCode::OK, "image/bmp", bmp565(m.width, m.height, &m.pixels))
    }

    fn theme(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        match themes::load(id) {
            Ok(theme) => json_reply(json!(theme)),
            Err(e) => failure(e),
        }
    }

    fn thumb(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        match themes::scene(id) {
            Ok(mut scene) => {
                let history = History::sample(&scene.graphed_metrics());
                frame_bmp(&scene.frame(&Readings::samples(), &history, &AlertView::default()))
            }
            Err(e) => failure(e),
        }
    }

    fn fonts(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        let mut list: Vec<Value> = Vec::new();
        if let Some(entries) = themes::folder(id).and_then(|f| std::fs::read_dir(f).ok()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.to_lowercase().ends_with(".ttf") || name.to_lowercase().ends_with(".otf") {
                    list.push(json!({ "file": name, "label": name }));
                }
            }
        }
        let fonts = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        for (file, label) in SYSTEM_FONTS {
            if fonts.join(file).is_file() {
                list.push(json!({ "file": file, "label": label }));
            }
        }
        json_reply(json!(list))
    }

    fn places(&self, args: &HashMap<String, String>) -> Reply {
        let name = args.get("q").map_or("", String::as_str);
        if name.trim().chars().count() < 2 {
            return json_reply(json!([]));
        }
        match weather::search(name) {
            Ok(places) => json_reply(json!(places)),
            Err(e) => failure(e),
        }
    }

    /// Se comprueba aquí, no solo en la página: con el aviso apagado no sale ninguna petición, pase lo que pase en la vista.
    fn update(&self) -> Reply {
        let version = self.config().check_updates.then(updates::newer_version).flatten();
        json_reply(json!({ "version": version }))
    }

    /// Dibuja un tema a medio editar, con datos de ejemplo o reales.
    fn preview(&self, args: &HashMap<String, String>, body: &[u8]) -> Reply {
        let id = args.get("id").cloned().unwrap_or_default();
        let theme: Theme = match serde_json::from_slice(body) {
            Ok(theme) => theme,
            Err(e) => return failure(e.to_string()),
        };
        let mut editing = self.editing.lock().unwrap_or_else(|e| e.into_inner());
        let current = editing.take().filter(|(open, _)| *open == id);
        let scene = match current {
            Some((_, mut scene)) => scene.update(theme.clone()).map(|()| scene),
            None => Scene::new(theme.clone(), themes::folder(&id)),
        };
        let mut scene = match scene.or_else(|_| Scene::new(theme, themes::folder(&id))) {
            Ok(scene) => scene,
            Err(e) => return failure(e),
        };
        let readings = self.readings(args.contains_key("live"));
        let history = History::sample(&scene.graphed_metrics());
        let frame = scene.frame(&readings, &history, &AlertView::default());
        *editing = Some((id, scene));
        frame_bmp(&frame)
    }

    fn set_config(&self, body: &[u8]) -> Reply {
        match serde_json::from_slice::<Config>(body) {
            Ok(mut config) => {
                config.sanitize();
                pulse_shared::i18n::set(config.language);
                self.bridge.send(Request::SetConfig(Box::new(config.clone())));
                // sin agente el cambio no se perdería: se guarda aquí y lo leerá al arrancar
                if !self.bridge.live.lock().unwrap_or_else(|e| e.into_inner()).connected {
                    let _ = config.save();
                }
                self.bridge.live.lock().unwrap_or_else(|e| e.into_inner()).config = Some(config);
                json_reply(json!({ "ok": true }))
            }
            Err(e) => failure(e.to_string()),
        }
    }

    fn action(&self, args: &HashMap<String, String>) -> Reply {
        let request = match args.get("name").map_or("", String::as_str) {
            "repair" => Some(Request::Repair),
            // nunca en cola: una orden así no puede dispararse sola cuando vuelva la conexión
            "hard_reset" => self.bridge.live.lock().is_ok_and(|live| live.connected).then_some(Request::HardReset),
            "deghost" => Some(Request::Deghost),
            "test_alert" => Some(Request::TestAlert),
            "screen_on" => Some(Request::SetScreen(true)),
            "screen_off" => Some(Request::SetScreen(false)),
            "quit" => {
                self.bridge.send(Request::Quit);
                // la ventana no tiene sentido sin el agente; se le da tiempo a la orden para que salga
                std::thread::spawn(|| {
                    std::thread::sleep(QUIT_GRACE);
                    std::process::exit(0);
                });
                None
            }
            "install_helper" => return elevated_reply(autostart::request_helper_install()),
            "remove_helper" => return elevated_reply(autostart::request_helper_removal()),
            "open_log" => {
                open_external(&paths::log_file().to_string_lossy());
                None
            }
            "open_themes" => {
                open_external(&paths::themes_dir().to_string_lossy());
                None
            }
            "kofi" => {
                open_external(KOFI);
                None
            }
            "repository" => {
                open_external(REPOSITORY);
                None
            }
            "pawnio" => {
                open_external(PAWNIO_PAGE);
                None
            }
            "releases" => {
                open_external(&format!("{REPOSITORY}/releases/latest"));
                None
            }
            other => return failure(format!("acción desconocida: {other}")),
        };
        if let Some(request) = request {
            self.bridge.send(request);
        }
        json_reply(json!({ "ok": true }))
    }

    fn save_theme(&self, args: &HashMap<String, String>, body: &[u8]) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        let theme: Theme = match serde_json::from_slice(body) {
            Ok(theme) => theme,
            Err(e) => return failure(e.to_string()),
        };
        match themes::save(id, &theme) {
            Ok(()) => {
                if self.config().panel.theme == id {
                    self.bridge.send(Request::ReloadTheme);
                }
                json_reply(json!({ "ok": true }))
            }
            Err(e) => failure(e),
        }
    }

    fn duplicate(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        let name = args.get("name").map_or("Tema", String::as_str);
        match themes::duplicate(id, name) {
            Ok(id) => json_reply(json!({ "id": id })),
            Err(e) => failure(e),
        }
    }

    /// Quita lo editado de un tema incluido. A diferencia de borrar, vale también para el que está en uso.
    fn revert(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        if !themes::is_builtin(id) {
            return failure("no es un tema incluido");
        }
        match themes::delete(id) {
            Ok(()) => {
                if self.config().panel.theme == id {
                    self.bridge.send(Request::ReloadTheme);
                }
                json_reply(json!({ "ok": true }))
            }
            Err(e) => failure(e),
        }
    }

    fn delete(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        if self.config().panel.theme == id {
            return failure("en uso");
        }
        match themes::delete(id) {
            Ok(()) => json_reply(json!({ "ok": true })),
            Err(e) => failure(e),
        }
    }

    /// Pide al usuario una imagen o una fuente y la copia al tema.
    fn pick(&self, args: &HashMap<String, String>) -> Reply {
        let id = args.get("id").map_or("", String::as_str);
        let (title, filter) = match args.get("kind").map_or("", String::as_str) {
            "image" => ("Imagen de fondo", "Imágenes|*.png;*.jpg;*.jpeg"),
            "font" => ("Fuente", "Fuentes|*.ttf;*.otf"),
            _ => return failure("tipo desconocido"),
        };
        let Some(source) = pick_file(self.owner(), title, filter) else { return json_reply(json!({ "file": null })) };
        match themes::add_file(id, &source) {
            Ok(file) if is_plain_file_name(&file) => {
                // la imagen nueva tiene que leerse de disco
                *self.editing.lock().unwrap_or_else(|e| e.into_inner()) = None;
                json_reply(json!({ "file": file }))
            }
            Ok(_) => failure("nombre no válido"),
            Err(e) => {
                log::warn(&format!("no se pudo copiar el archivo al tema: {e}"));
                failure(e)
            }
        }
    }
}

fn pawnio_installed() -> bool {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .is_some_and(|p| p.join("PawnIO").join("PawnIOLib.dll").is_file())
}

/// Abre una dirección o carpeta conocida con el programa predeterminado.
fn open_external(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (operation, path) = (crate::wide("open"), crate::wide(target));
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bmp_header_describes_the_image() {
        let bmp = bmp565(2, 2, &[0; 8]);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 66);
        assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), -2, "de arriba abajo");
        assert_eq!(bmp.len(), 66 + 8);
    }

    #[test]
    fn rows_are_padded_to_four_bytes() {
        let bmp = bmp565(3, 1, &[1; 6]);
        assert_eq!(bmp.len(), 66 + 8);
    }

    #[test]
    fn query_values_are_decoded() {
        assert_eq!(unescape("S%C3%A3o%20Paulo"), "São Paulo");
        assert_eq!(unescape("100%"), "100%");
        assert_eq!(unescape("%zz"), "%zz");
    }

    #[test]
    fn only_known_pages_are_served() {
        assert!(asset("/index.html").is_some());
        assert!(asset("/../Cargo.toml").is_none());
    }
}
