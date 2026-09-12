//! Guest helpers for Cocktail WASM plugins.
//!
//! Host functions live in the Extism default namespace. Plugins export
//! `start`, `http_handle`, optional `on_event` / `tick` / `stop`.

use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[host_fn]
extern "ExtismHost" {
    fn cocktail_log(level: String, msg: String);
    fn cocktail_control(req: String) -> String;
    fn cocktail_kv_get(key: String) -> String;
    fn cocktail_kv_set(key: String, val: String);
    fn cocktail_http(req: String) -> String;
    fn cocktail_fs(req: String) -> String;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpReq {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: serde_json::Value,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResp {
    pub status: u16,
    #[serde(default = "json_ct")]
    pub content_type: String,
    pub body: String,
}

fn json_ct() -> String {
    "application/json".into()
}

impl HttpResp {
    pub fn json(status: u16, value: impl Serialize) -> Self {
        Self {
            status,
            content_type: json_ct(),
            body: serde_json::to_string(&value).unwrap_or_else(|_| "{}".into()),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8".into(),
            body: body.into(),
        }
    }

    pub fn ok_json(value: impl Serialize) -> Self {
        Self::json(200, value)
    }

    pub fn bad(msg: impl Into<String>) -> Self {
        Self::json(400, serde_json::json!({ "error": msg.into() }))
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::json(404, serde_json::json!({ "error": msg.into() }))
    }
}

pub fn log_info(msg: impl AsRef<str>) {
    let _ = unsafe { cocktail_log("info".into(), msg.as_ref().to_string()) };
}

pub fn log_warn(msg: impl AsRef<str>) {
    let _ = unsafe { cocktail_log("warn".into(), msg.as_ref().to_string()) };
}

pub fn log_error(msg: impl AsRef<str>) {
    let _ = unsafe { cocktail_log("error".into(), msg.as_ref().to_string()) };
}

pub fn kv_get(key: &str) -> Option<String> {
    let raw = unsafe { cocktail_kv_get(key.to_string()) }.ok()?;
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

pub fn kv_set(key: &str, val: &str) {
    let _ = unsafe { cocktail_kv_set(key.to_string(), val.to_string()) };
}

pub fn kv_get_json<T: for<'de> Deserialize<'de>>(key: &str) -> Option<T> {
    kv_get(key).and_then(|s| serde_json::from_str(&s).ok())
}

pub fn kv_set_json(key: &str, val: &impl Serialize) {
    if let Ok(s) = serde_json::to_string(val) {
        kv_set(key, &s);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlReq {
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResp {
    pub status: u16,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub ok: bool,
}

pub fn control(method: &str, path: &str, body: Option<&str>) -> Result<ControlResp, String> {
    let req = ControlReq {
        method: method.into(),
        path: path.into(),
        body: body.map(|s| s.to_string()),
        content_type: body.map(|_| "application/json".into()),
        body_b64: None,
        filename: None,
    };
    let payload = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    let raw = unsafe { cocktail_control(payload) }.map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn control_json<T: for<'de> Deserialize<'de>>(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<T, String> {
    let resp = control(method, path, body)?;
    if !resp.ok && resp.status >= 400 {
        return Err(format!("control {} {}: {}", method, path, resp.body));
    }
    serde_json::from_str(&resp.body).map_err(|e| e.to_string())
}

pub fn control_upload(instance_id: &str, dest: &str, filename: &str, bytes: &[u8]) -> Result<(), String> {
    let req = ControlReq {
        method: "POST".into(),
        path: format!(
            "/api/v1/instances/{}/files/upload?path={}",
            instance_id,
            urlencoding(dest)
        ),
        body: None,
        content_type: Some("application/octet-stream".into()),
        body_b64: Some(b64(bytes)),
        filename: Some(filename.into()),
    };
    let payload = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    let raw = unsafe { cocktail_control(payload) }.map_err(|e| e.to_string())?;
    let resp: ControlResp = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if resp.status >= 400 {
        Err(resp.body)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostHttpReq {
    pub url: String,
    #[serde(default = "get_method")]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<Vec<(String, String)>>,
}

fn get_method() -> String {
    "GET".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostHttpResp {
    pub status: u16,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub body_b64: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

pub fn http(req: &HostHttpReq) -> Result<HostHttpResp, String> {
    let payload = serde_json::to_string(req).map_err(|e| e.to_string())?;
    let raw = unsafe { cocktail_http(payload) }.map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsReq {
    pub op: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsResp {
    pub ok: bool,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub exists: bool,
    #[serde(default)]
    pub size: u64,
}

pub fn fs(req: &FsReq) -> Result<FsResp, String> {
    let payload = serde_json::to_string(req).map_err(|e| e.to_string())?;
    let raw = unsafe { cocktail_fs(payload) }.map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn path_tail<'a>(path: &'a str, prefix: &str) -> &'a str {
    let p = path.trim_start_matches('/');
    let pre = prefix.trim_start_matches('/');
    p.strip_prefix(pre)
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or("")
}

pub fn json_body<T: for<'de> Deserialize<'de>>(req: &HttpReq) -> Result<T, HttpResp> {
    serde_json::from_str(&req.body).map_err(|e| HttpResp::bad(e.to_string()))
}

pub fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
