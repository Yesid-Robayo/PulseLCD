//! Peticiones HTTP con WinHTTP, la pila de Windows: TLS y certificados del sistema, sin bibliotecas añadidas.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
    WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts,
};

/// Ninguna de las respuestas que se esperan pesa más. Una mayor se descarta sin guardarla.
const MAX_BODY: usize = 2 * 1024 * 1024;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

#[derive(Debug, PartialEq)]
struct Target {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse(url: &str) -> Option<Target> {
    let (secure, rest) = match url.split_once("://")? {
        ("https", rest) => (true, rest),
        ("http", rest) => (false, rest),
        _ => return None,
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    (!host.is_empty()).then(|| Target { secure, host: host.into(), port, path: path.into() })
}

/// GET a una dirección. Devuelve el cuerpo de la respuesta.
pub fn get(url: &str, timeout_ms: i32) -> Result<String, String> {
    let target = parse(url).ok_or("dirección no válida")?;
    unsafe {
        let session =
            Handle(WinHttpOpen(wide("PulseLCD").as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0));
        if session.0.is_null() {
            return Err("WinHTTP no disponible".into());
        }
        WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
        let connection = Handle(WinHttpConnect(session.0, wide(&target.host).as_ptr(), target.port, 0));
        if connection.0.is_null() {
            return Err("sin conexión".into());
        }
        let flags = if target.secure { WINHTTP_FLAG_SECURE } else { 0 };
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            wide("GET").as_ptr(),
            wide(&target.path).as_ptr(),
            null(),
            null(),
            null(),
            flags,
        ));
        if request.0.is_null() {
            return Err("sin conexión".into());
        }
        if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return Err("no responde".into());
        }
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                return Err("respuesta cortada".into());
            }
            if available == 0 {
                break;
            }
            if body.len() + available as usize > MAX_BODY {
                return Err("respuesta demasiado grande".into());
            }
            let start = body.len();
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            if WinHttpReadData(request.0, body[start..].as_mut_ptr() as *mut _, available, &mut read) == 0 {
                return Err("respuesta cortada".into());
            }
            body.truncate(start + read as usize);
        }
        String::from_utf8(body).map_err(|_| "respuesta no válida".to_string())
    }
}

/// Escapa un texto para usarlo en una dirección.
pub fn escape(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_split_correctly() {
        let t = parse("http://127.0.0.1:8765/v1/status").unwrap();
        assert_eq!((t.secure, t.host.as_str(), t.port, t.path.as_str()), (false, "127.0.0.1", 8765, "/v1/status"));
        let t = parse("https://api.example.com/x?a=1").unwrap();
        assert_eq!((t.secure, t.port, t.path.as_str()), (true, 443, "/x?a=1"));
        assert_eq!(parse("https://host").unwrap().path, "/");
    }

    #[test]
    fn strange_addresses_are_refused() {
        assert_eq!(parse("ftp://host/x"), None);
        assert_eq!(parse("sin esquema"), None);
        assert_eq!(parse("http://:80/"), None);
    }

    #[test]
    fn city_names_are_escaped() {
        assert_eq!(escape("São Paulo"), "S%C3%A3o%20Paulo");
    }

    #[test]
    fn a_closed_port_is_an_error_not_a_hang() {
        assert!(get("http://127.0.0.1:9/", 500).is_err());
    }
}
