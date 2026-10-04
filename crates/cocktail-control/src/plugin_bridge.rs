use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use extism::{Manifest, PTR, Plugin, PluginBuilder, UserData, Wasm, host_fn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::instance::InstanceEvent;
use crate::state::AppState;

const HOST_LABEL: &str = "wasm://in-process";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub ui: Option<Value>,
    #[serde(default)]
    pub tick_ms: Option<u64>,
}

fn default_version() -> String {
    "0.1.0".into()
}

#[derive(Default)]
pub struct PluginRegistry {
    slots: HashMap<String, PluginSlot>,
}

struct PluginSlot {
    manifest: PluginManifest,
    directory: PathBuf,
    data_dir: PathBuf,
    enabled: bool,
    running: bool,
    error: Option<String>,
    instance: Arc<std::sync::Mutex<Option<Plugin>>>,
}

#[derive(Clone)]
struct HostCtx {
    state: Weak<AppState>,
    plugin_id: String,
    permissions: Vec<String>,
    data_dir: PathBuf,
    rt: tokio::runtime::Handle,
}

impl PluginRegistry {
    pub fn catalog_values(&self) -> Vec<Value> {
        let mut items: Vec<_> = self.slots.values().map(|s| s.catalog_json()).collect();
        items.sort_by(|a, b| {
            a.get("id")
                .and_then(|v| v.as_str())
                .cmp(&b.get("id").and_then(|v| v.as_str()))
        });
        items
    }
}

impl PluginSlot {
    fn catalog_json(&self) -> Value {
        json!({
            "id": self.manifest.id,
            "name": self.manifest.name,
            "version": self.manifest.version,
            "description": self.manifest.description,
            "permissions": self.manifest.permissions,
            "ui": self.manifest.ui,
            "enabled": self.enabled,
            "running": self.running,
            "error": self.error,
            "directory": self.directory.to_string_lossy(),
            "runtime": "wasm",
        })
    }
}

pub fn default_host_url() -> String {
    std::env::var("COCKTAIL_PLUGIN_HOST")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| HOST_LABEL.into())
}

pub fn resolve_token() -> String {
    if let Ok(t) = std::env::var("COCKTAIL_PLUGIN_TOKEN") {
        if !t.is_empty() {
            return t;
        }
    }
    let path = PathBuf::from("data/.plugin-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let token = format!("pt_{}", uuid::Uuid::new_v4());
    let _ = std::fs::create_dir_all("data");
    let _ = std::fs::write(&path, &token);
    token
}

pub fn spawn_event_forwarder(state: &Arc<AppState>) {
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let mut rx = state.events.subscribe();
        let verbose = std::env::var("COCKTAIL_PLUGIN_EVENTS")
            .ok()
            .is_some_and(|v| v == "all" || v == "1");
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let skip = !verbose
                        && matches!(
                            event,
                            InstanceEvent::Log { .. }
                                | InstanceEvent::Metric { .. }
                                | InstanceEvent::DownloadProgress { .. }
                        );
                    if skip {
                        continue;
                    }
                    if let Err(e) = dispatch_event(&state, &event).await {
                        tracing::debug!(error = %e, "wasm plugin event drop");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

pub fn maybe_autostart(state: &Arc<AppState>) {
    let flag = std::env::var("COCKTAIL_PLUGIN_AUTOSTART").unwrap_or_else(|_| "1".into());
    if flag == "0" || flag.eq_ignore_ascii_case("false") {
        return;
    }
    let state = Arc::clone(state);
    tokio::spawn(async move {
        if let Err(e) = reload_inner(&state).await {
            tracing::warn!(error = %e, "wasm plugin host failed to load");
        }
        spawn_ticks(state);
    });
}

fn lock_plugins(state: &AppState) -> std::sync::MutexGuard<'_, PluginRegistry> {
    state.plugins.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock_instance(
    mutex: &std::sync::Mutex<Option<Plugin>>,
) -> std::sync::MutexGuard<'_, Option<Plugin>> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn join_host<T>(
    rt: &tokio::runtime::Handle,
    timeout: Duration,
    fut: impl std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
) -> anyhow::Result<T>
where
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    rt.spawn(async move {
        let _ = tx.send(fut.await);
    });
    rx.recv_timeout(timeout)
        .map_err(|_| anyhow::anyhow!("plugin host call timed out"))?
}

fn multipart_body(filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----CocktailPluginBoundary";
    let safe: String = filename
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\r' | '\n' | '\\') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{safe}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn invoke_plane(
    rt: &tokio::runtime::Handle,
    state: &AppState,
    method: &str,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> anyhow::Result<(u16, String)> {
    let router = state
        .plane
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("control plane router not attached"))?;
    let token = state.plugin_token.clone();
    let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);
    let uri = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(&uri)
        .header("Authorization", format!("Bearer {token}"));
    if !body.is_empty() || method != Method::GET {
        builder = builder.header("Content-Type", content_type);
    }
    let req = builder.body(Body::from(body))?;
    join_host(rt, Duration::from_secs(60), async move {
        let res = router
            .oneshot(req)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let status = res.status().as_u16();
        let bytes = axum::body::to_bytes(res.into_body(), 32 * 1024 * 1024)
            .await
            .unwrap_or_default();
        Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
    })
}

fn spawn_ticks(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut elapsed: HashMap<String, u64> = HashMap::new();
        loop {
            interval.tick().await;
            let ticks: Vec<(String, u64)> = {
                let reg = lock_plugins(&state);
                reg.slots
                    .values()
                    .filter(|s| s.running)
                    .filter_map(|s| s.manifest.tick_ms.map(|ms| (s.manifest.id.clone(), ms)))
                    .collect()
            };
            for (id, ms) in ticks {
                let acc = elapsed.entry(id.clone()).or_insert(0);
                *acc += 1000;
                if *acc >= ms.max(1000) {
                    *acc = 0;
                    if let Err(e) = call_export(&state, &id, "tick", "{}").await {
                        tracing::debug!(plugin = %id, error = %e, "plugin tick");
                    }
                }
            }
        }
    });
}

pub async fn catalog(state: &AppState) -> anyhow::Result<Vec<Value>> {
    Ok(lock_plugins(state).catalog_values())
}

pub async fn reload_arc(state: &Arc<AppState>) -> anyhow::Result<Value> {
    reload_inner(state).await?;
    let n = lock_plugins(state).slots.len();
    Ok(json!({ "ok": true, "plugins": n, "runtime": HOST_LABEL }))
}

pub async fn set_enabled(state: &Arc<AppState>, id: &str, enabled: bool) -> anyhow::Result<Value> {
    {
        let mut saved = load_enabled_map();
        saved.insert(id.to_string(), enabled);
        save_enabled_map(&saved);
    }
    let state = Arc::clone(state);
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        let mut reg = lock_plugins(&state);
        let slot = reg
            .slots
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("plugin not found"))?;
        slot.enabled = enabled;
        if enabled {
            start_slot(&state, slot)?;
        } else {
            stop_slot(slot);
        }
        Ok(json!({ "ok": true, "id": id, "enabled": enabled }))
    })
    .await?
}

pub async fn health_snapshot(state: &AppState) -> (bool, usize) {
    let running = lock_plugins(state)
        .slots
        .values()
        .filter(|s| s.running)
        .count();
    (true, running)
}

pub async fn proxy(
    state: &Arc<AppState>,
    plugin_id: &str,
    rest: &str,
    method: Method,
    _headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = if rest.trim_start_matches('/').is_empty() {
        "/".to_string()
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        format!("/{rest}")
    };
    let req = json!({
        "method": method.as_str(),
        "path": path,
        "body": String::from_utf8_lossy(&body),
    });
    match call_export(state, plugin_id, "http_handle", &req.to_string()).await {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(v) => {
                let status = v.get("status").and_then(|s| s.as_u64()).unwrap_or(200) as u16;
                let ct = v
                    .get("content_type")
                    .and_then(|s| s.as_str())
                    .unwrap_or("application/json");
                let body = v.get("body").and_then(|s| s.as_str()).unwrap_or(&raw);
                let mut response = Response::new(Body::from(body.to_string()));
                *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
                if let Ok(hv) = ct.parse() {
                    response
                        .headers_mut()
                        .insert(axum::http::header::CONTENT_TYPE, hv);
                }
                response
            }
            Err(_) => {
                let mut response = Response::new(Body::from(raw));
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    "application/json".parse().unwrap(),
                );
                response
            }
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[allow(dead_code)]
pub async fn post_event(state: &AppState, event: &InstanceEvent) -> anyhow::Result<()> {
    let _ = (state, event);
    Ok(())
}

async fn dispatch_event(state: &Arc<AppState>, event: &InstanceEvent) -> anyhow::Result<()> {
    let payload = serde_json::to_string(event)?;
    let ids: Vec<String> = {
        let reg = lock_plugins(state);
        reg.slots
            .values()
            .filter(|s| {
                s.running
                    && s.manifest
                        .permissions
                        .iter()
                        .any(|p| p == "events.subscribe")
            })
            .map(|s| s.manifest.id.clone())
            .collect()
    };
    for id in ids {
        let _ = call_export(state, &id, "on_event", &payload).await;
    }
    Ok(())
}

async fn call_export(
    state: &Arc<AppState>,
    id: &str,
    export: &str,
    input: &str,
) -> anyhow::Result<String> {
    let state = Arc::clone(state);
    let id = id.to_string();
    let export = export.to_string();
    let input = input.to_string();
    tokio::task::spawn_blocking(move || {
        let instance = {
            let reg = lock_plugins(&state);
            let slot = reg
                .slots
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("plugin {id} not found"))?;
            if !slot.running {
                anyhow::bail!("plugin {id} is not running");
            }
            Arc::clone(&slot.instance)
        };
        let mut guard = lock_instance(&instance);
        let plugin = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("plugin {id} wasm not loaded"))?;
        if !plugin.function_exists(&export) {
            anyhow::bail!("export {export} missing");
        }
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            plugin.call::<&str, String>(&export, &input)
        }));
        match called {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => {
                *guard = None;
                drop(guard);
                let mut reg = lock_plugins(&state);
                if let Some(slot) = reg.slots.get_mut(&id) {
                    slot.running = false;
                    slot.error = Some(format!("wasm panic in {export}"));
                }
                anyhow::bail!("plugin {id} panicked in {export}");
            }
        }
    })
    .await?
}

async fn reload_inner(state: &Arc<AppState>) -> anyhow::Result<()> {
    let saved = load_enabled_map();
    let discovered = discover_plugins();
    let mut next = PluginRegistry::default();
    for (dir, manifest) in discovered {
        let enabled = saved.get(&manifest.id).copied().unwrap_or(true);
        let data_dir = PathBuf::from("data")
            .join("extensions")
            .join(&manifest.id)
            .join("data");
        let _ = std::fs::create_dir_all(&data_dir);
        let mut slot = PluginSlot {
            manifest,
            directory: dir,
            data_dir,
            enabled,
            running: false,
            error: None,
            instance: Arc::new(std::sync::Mutex::new(None)),
        };
        if enabled {
            let st = Arc::clone(state);
            slot = tokio::task::spawn_blocking(move || {
                if let Err(e) = start_slot(&st, &mut slot) {
                    slot.error = Some(e.to_string());
                    tracing::warn!(plugin = %slot.manifest.id, error = %e, "plugin start failed");
                }
                slot
            })
            .await?;
        }
        next.slots.insert(slot.manifest.id.clone(), slot);
    }
    let n = next.slots.len();
    *lock_plugins(state) = next;
    tracing::info!(count = n, "wasm plugin host loaded");
    Ok(())
}

fn start_slot(state: &Arc<AppState>, slot: &mut PluginSlot) -> anyhow::Result<()> {
    stop_slot(slot);
    let wasm_path = find_wasm(&slot.directory)
        .ok_or_else(|| anyhow::anyhow!("no plugin.wasm in {}", slot.directory.display()))?;
    let ctx = HostCtx {
        state: Arc::downgrade(state),
        plugin_id: slot.manifest.id.clone(),
        permissions: slot.manifest.permissions.clone(),
        data_dir: slot.data_dir.clone(),
        rt: tokio::runtime::Handle::current(),
    };
    let user = UserData::new(ctx);
    let manifest = Manifest::new([Wasm::file(&wasm_path)]);
    let mut plugin = PluginBuilder::new(manifest)
        .with_wasi(true)
        .with_function("cocktail_log", [PTR, PTR], [], user.clone(), cocktail_log)
        .with_function(
            "cocktail_control",
            [PTR],
            [PTR],
            user.clone(),
            cocktail_control,
        )
        .with_function(
            "cocktail_kv_get",
            [PTR],
            [PTR],
            user.clone(),
            cocktail_kv_get,
        )
        .with_function(
            "cocktail_kv_set",
            [PTR, PTR],
            [],
            user.clone(),
            cocktail_kv_set,
        )
        .with_function("cocktail_http", [PTR], [PTR], user.clone(), cocktail_http)
        .with_function("cocktail_fs", [PTR], [PTR], user.clone(), cocktail_fs)
        .build()?;
    if plugin.function_exists("start") {
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            plugin.call::<&str, String>("start", "")
        }));
        match called {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => anyhow::bail!("plugin {} panicked in start", slot.manifest.id),
        }
    }
    *lock_instance(&slot.instance) = Some(plugin);
    slot.running = true;
    slot.error = None;
    tracing::info!(id = %slot.manifest.id, wasm = %wasm_path.display(), "wasm plugin started");
    Ok(())
}

fn stop_slot(slot: &mut PluginSlot) {
    let mut guard = lock_instance(&slot.instance);
    if let Some(mut plugin) = guard.take() {
        if plugin.function_exists("stop") {
            let _ = plugin.call::<&str, String>("stop", "");
        }
    }
    drop(guard);
    slot.running = false;
}

fn find_wasm(dir: &Path) -> Option<PathBuf> {
    for name in ["plugin.wasm", "plugin.core.wasm"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("wasm") {
            Some(p)
        } else {
            None
        }
    })
}

fn discover_plugins() -> Vec<(PathBuf, PluginManifest)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in plugin_search_dirs() {
        if !root.is_dir() {
            continue;
        }
        if let Some(item) = load_dir(&root) {
            if seen.insert(item.1.id.clone()) {
                out.push(item);
            }
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&root) {
            for ent in rd.flatten() {
                let dir = ent.path();
                if !dir.is_dir() {
                    continue;
                }
                if let Some(item) = load_dir(&dir) {
                    if seen.insert(item.1.id.clone()) {
                        out.push(item);
                    }
                }
            }
        }
    }
    out
}

fn load_dir(dir: &Path) -> Option<(PathBuf, PluginManifest)> {
    let path = dir.join("plugin.json");
    if !path.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let manifest: PluginManifest = serde_json::from_str(&text).ok()?;
    if manifest.id.is_empty() {
        return None;
    }
    Some((dir.to_path_buf(), manifest))
}

fn plugin_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(env) = std::env::var("COCKTAIL_PLUGIN_DIR") {
        if !env.is_empty() {
            dirs.push(PathBuf::from(env));
        }
    }
    dirs.push(PathBuf::from("data/extensions"));
    dirs.push(PathBuf::from("dist/plugins"));
    dirs.push(PathBuf::from("crates/plugins"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("plugins"));
        }
    }
    dirs
}

fn enabled_path() -> PathBuf {
    PathBuf::from("data/plugin-state.json")
}

fn load_enabled_map() -> HashMap<String, bool> {
    let Ok(text) = std::fs::read_to_string(enabled_path()) else {
        return HashMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_enabled_map(map: &HashMap<String, bool>) {
    let _ = std::fs::create_dir_all("data");
    let _ = std::fs::write(
        enabled_path(),
        serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".into()),
    );
}

host_fn!(cocktail_log(user_data: HostCtx; level: String, msg: String) {
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    match level.as_str() {
        "warn" => tracing::warn!(plugin = %ctx.plugin_id, "{msg}"),
        "error" => tracing::error!(plugin = %ctx.plugin_id, "{msg}"),
        _ => tracing::info!(plugin = %ctx.plugin_id, "{msg}"),
    }
    Ok(())
});

host_fn!(cocktail_kv_get(user_data: HostCtx; key: String) -> String {
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    let path = kv_path(&ctx.data_dir, &key)?;
    Ok(std::fs::read_to_string(path).unwrap_or_default())
});

host_fn!(cocktail_kv_set(user_data: HostCtx; key: String, val: String) {
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    let path = kv_path(&ctx.data_dir, &key)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, val)?;
    Ok(())
});

host_fn!(cocktail_control(user_data: HostCtx; req: String) -> String {
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    let parsed: Value = serde_json::from_str(&req)?;
    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
    let path = parsed.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    deny_control(&ctx.permissions, method, path)?;
    let Some(state) = ctx.state.upgrade() else {
        anyhow::bail!("control plane gone");
    };
    let body = parsed.get("body").and_then(|v| v.as_str()).map(|s| s.to_string());
    let body_b64 = parsed.get("body_b64").and_then(|v| v.as_str()).map(|s| s.to_string());
    let filename = parsed
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("upload.bin")
        .to_string();
    let mut content_type = parsed
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("application/json")
        .to_string();
    let method = method.to_string();
    let path = path.to_string();
    let rt = ctx.rt.clone();
    drop(ctx);
    let bytes = if let Some(b64) = body_b64 {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD.decode(b64)?;
        let (ct, encoded) = multipart_body(&filename, &raw);
        content_type = ct;
        encoded
    } else {
        body.unwrap_or_default().into_bytes()
    };
    let (status, text) = invoke_plane(&rt, &state, &method, &path, &content_type, bytes)?;
    Ok(json!({ "status": status, "ok": status < 400, "body": text }).to_string())
});

host_fn!(cocktail_http(user_data: HostCtx; req: String) -> String {
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    if !ctx.permissions.iter().any(|p| p == "net.fetch") {
        anyhow::bail!("missing permission net.fetch");
    }
    let parsed: Value = serde_json::from_str(&req)?;
    let url = parsed.get("url").and_then(|v| v.as_str()).unwrap_or("");
    deny_url(url)?;
    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
    let Some(state) = ctx.state.upgrade() else {
        anyhow::bail!("control plane gone");
    };
    let mut builder = state.http.request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET),
        url,
    );
    if let Some(headers) = parsed.get("headers").and_then(|v| v.as_array()) {
        for h in headers {
            if let (Some(k), Some(val)) = (
                h.get(0).and_then(|x| x.as_str()),
                h.get(1).and_then(|x| x.as_str()),
            ) {
                builder = builder.header(k, val);
            }
        }
    }
    if let Some(form) = parsed.get("form").and_then(|v| v.as_array()) {
        let mut pairs = Vec::new();
        for h in form {
            if let (Some(k), Some(val)) = (
                h.get(0).and_then(|x| x.as_str()),
                h.get(1).and_then(|x| x.as_str()),
            ) {
                pairs.push((k.to_string(), val.to_string()));
            }
        }
        builder = builder.form(&pairs);
    } else if let Some(body) = parsed.get("body").and_then(|v| v.as_str()) {
        builder = builder.body(body.to_string());
    }
    let rt = ctx.rt.clone();
    drop(ctx);
    join_host(&rt, Duration::from_secs(120), async move {
        let res = builder.timeout(Duration::from_secs(120)).send().await?;
        let status = res.status().as_u16();
        let headers: Vec<(String, String)> = res
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let bytes = res.bytes().await.unwrap_or_default();
        use base64::Engine;
        let body_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let body = String::from_utf8(bytes.to_vec()).unwrap_or_default();
        Ok(json!({
            "status": status,
            "body": body,
            "body_b64": body_b64,
            "headers": headers,
        })
        .to_string())
    })
});

host_fn!(cocktail_fs(user_data: HostCtx; req: String) -> String {
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    let parsed: Value = serde_json::from_str(&req)?;
    let op = parsed.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let rel = parsed.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let path = resolve_fs_path(&ctx, rel)?;
    let dest = match parsed.get("dest").and_then(|v| v.as_str()) {
        Some(d) if !d.is_empty() => Some(resolve_fs_path(&ctx, d)?),
        _ => None,
    };
    match op {
        "mkdir" => {
            std::fs::create_dir_all(&path)?;
            Ok(json!({ "ok": true, "body": path.to_string_lossy() }).to_string())
        }
        "exists" => Ok(json!({ "ok": true, "exists": path.exists() }).to_string()),
        "read_text" => {
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            Ok(json!({ "ok": true, "body": body, "exists": path.exists() }).to_string())
        }
        "write_text" => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let body = parsed.get("body").and_then(|v| v.as_str()).unwrap_or("");
            std::fs::write(&path, body)?;
            Ok(json!({ "ok": true }).to_string())
        }
        "write_bytes" => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if let Some(b64) = parsed.get("body_b64").and_then(|v| v.as_str()) {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
                std::fs::write(&path, bytes)?;
            }
            Ok(json!({ "ok": true }).to_string())
        }
        "copy_dir" => {
            let dest = dest.ok_or_else(|| anyhow::anyhow!("dest required"))?;
            copy_dir(&path, &dest)?;
            Ok(json!({ "ok": true }).to_string())
        }
        "symlink" => {
            let dest = dest.ok_or_else(|| anyhow::anyhow!("dest required"))?;
            if dest.exists() {
                return Ok(json!({ "ok": true, "body": "exists" }).to_string());
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&path, &dest)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&path, &dest)?;
            Ok(json!({ "ok": true }).to_string())
        }
        "dir_size" => {
            Ok(json!({ "ok": true, "size": dir_size(&path) }).to_string())
        }
        "list" => {
            let mut names = Vec::new();
            if path.is_dir() {
                for e in std::fs::read_dir(&path)?.flatten() {
                    names.push(e.file_name().to_string_lossy().to_string());
                }
            }
            Ok(json!({ "ok": true, "body": serde_json::to_string(&names)? }).to_string())
        }
        "remove" => {
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else if path.exists() {
                std::fs::remove_file(&path)?;
            }
            Ok(json!({ "ok": true }).to_string())
        }
        other => anyhow::bail!("unknown fs op {other}"),
    }
});

fn kv_path(data_dir: &Path, key: &str) -> anyhow::Result<PathBuf> {
    if key.contains("..") || key.contains('\\') || key.starts_with('/') {
        anyhow::bail!("invalid kv key");
    }
    Ok(data_dir.join("kv").join(key))
}

fn resolve_fs_path(ctx: &HostCtx, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = rel.replace('\\', "/");
    let path = if rel.starts_with("/data/") || rel == "/data" {
        let rest = rel.trim_start_matches("/data").trim_start_matches('/');
        ctx.data_dir.join(rest)
    } else if rel.starts_with("data/") || !rel.starts_with('/') {
        let cleaned = rel.trim_start_matches('/').trim_start_matches("data/");
        PathBuf::from("data").join(cleaned)
    } else {
        PathBuf::from(rel.trim_start_matches('/'))
    };
    let full = std::fs::canonicalize(path.parent().unwrap_or(&path)).unwrap_or(path.clone());
    let allowed_data = std::fs::canonicalize(&ctx.data_dir).unwrap_or(ctx.data_dir.clone());
    let allowed_gameops = PathBuf::from("data/gameops");
    let allowed_instances = PathBuf::from("data/instances");
    let has_files = ctx
        .permissions
        .iter()
        .any(|p| p == "controlplane.files.write" || p == "controlplane.files.read");
    let under_data = full.starts_with(&allowed_data) || path.starts_with(&ctx.data_dir);
    let under_go = has_files && path.starts_with(&allowed_gameops);
    let under_inst = has_files && path.starts_with(&allowed_instances);
    if under_data || under_go || under_inst {
        return Ok(path);
    }
    anyhow::bail!("path not allowed: {}", path.display())
}

fn copy_dir(src: &Path, dest: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            if from.file_name().and_then(|s| s.to_str()) == Some(".snapshots") {
                continue;
            }
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    let mut n = 0u64;
    let Ok(walk) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut stack: Vec<PathBuf> = walk.flatten().map(|e| e.path()).collect();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else if let Ok(meta) = p.metadata() {
            n += meta.len();
        }
    }
    n
}

fn deny_url(url: &str) -> anyhow::Result<()> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://api.github.com/")
        || lower.starts_with("https://github.com/")
        || lower.starts_with("https://objects.githubusercontent.com/")
        || lower.starts_with("http://127.0.0.1:")
        || lower.starts_with("http://localhost:")
    {
        return Ok(());
    }
    anyhow::bail!("url not allowed")
}

fn deny_control(perms: &[String], method: &str, path: &str) -> anyhow::Result<()> {
    let p = path.split('?').next().unwrap_or(path);
    if p.starts_with("/api/v1/ext") || p.starts_with("/api/v1/extensions") {
        anyhow::bail!("plugins cannot call the extension proxy");
    }
    let m = method.to_ascii_uppercase();
    let need = if p == "/api/v1/instances" && m == "GET"
        || p.starts_with("/api/v1/instances/") && m == "GET" && !p.contains("/files")
    {
        "controlplane.instances.read"
    } else if p.starts_with("/api/v1/nodes") && m == "GET" {
        "controlplane.nodes.read"
    } else if p.contains("/files") && m == "GET" {
        return require_any(
            perms,
            &["controlplane.files.read", "controlplane.files.write"],
        );
    } else if p.contains("/files")
        || p.contains("/modrinth/install")
        || p.contains("/hangar/install")
        || p.contains("/spiget/install")
    {
        "controlplane.files.write"
    } else if p.starts_with("/api/v1/instances") {
        "controlplane.instances.write"
    } else {
        anyhow::bail!("control path not allowed: {method} {path}");
    };
    require_any(perms, &[need])
}

fn require_any(perms: &[String], need: &[&str]) -> anyhow::Result<()> {
    if need.iter().any(|n| perms.iter().any(|p| p == n)) {
        Ok(())
    } else {
        anyhow::bail!("missing permission {}", need[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_instances_allowed() {
        deny_control(
            &["controlplane.instances.read".into()],
            "GET",
            "/api/v1/instances",
        )
        .unwrap();
    }

    #[test]
    fn read_instances_denied_without_perm() {
        assert!(deny_control(&[], "GET", "/api/v1/instances").is_err());
    }

    #[test]
    fn files_query_uses_files_read() {
        deny_control(
            &["controlplane.files.read".into()],
            "GET",
            "/api/v1/instances/abc/files?path=mods",
        )
        .unwrap();
    }

    #[test]
    fn cannot_call_extension_proxy() {
        assert!(
            deny_control(
                &["controlplane.instances.read".into()],
                "GET",
                "/api/v1/ext/watchdog/summary"
            )
            .is_err()
        );
        assert!(
            deny_control(
                &["controlplane.instances.read".into()],
                "GET",
                "/api/v1/extensions"
            )
            .is_err()
        );
    }

    #[test]
    fn start_instance_needs_write() {
        assert!(
            deny_control(
                &["controlplane.instances.read".into()],
                "POST",
                "/api/v1/instances/abc/start"
            )
            .is_err()
        );
        deny_control(
            &["controlplane.instances.write".into()],
            "POST",
            "/api/v1/instances/abc/start",
        )
        .unwrap();
    }

    #[test]
    fn github_url_allowed() {
        deny_url("https://api.github.com/repos/FORGE24/ESPlus/releases").unwrap();
        assert!(deny_url("https://evil.example/").is_err());
    }
}
