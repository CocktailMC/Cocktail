//! Bridge to the .NET plugin host (GameOps extension plane).

use std::path::PathBuf;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tokio::process::Command;

use crate::instance::InstanceEvent;
use crate::state::AppState;

pub fn default_host_url() -> String {
    std::env::var("COCKTAIL_PLUGIN_HOST")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:11012".into())
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

pub fn spawn_event_forwarder(state: &std::sync::Arc<AppState>) {
    let state = std::sync::Arc::clone(state);
    tokio::spawn(async move {
        let mut rx = state.events.subscribe();
        let verbose = std::env::var("COCKTAIL_PLUGIN_EVENTS")
            .ok()
            .is_some_and(|v| v == "all" || v == "1");
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let skip = !verbose
                        && matches!(event, InstanceEvent::Log { .. } | InstanceEvent::Metric { .. });
                    if skip {
                        continue;
                    }
                    if let Err(e) = post_event(&state, &event).await {
                        tracing::debug!(error = %e, "plugin host event drop");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

pub fn maybe_autostart(state: &std::sync::Arc<AppState>) {
    let flag = std::env::var("COCKTAIL_PLUGIN_AUTOSTART").unwrap_or_else(|_| "1".into());
    if flag == "0" || flag.eq_ignore_ascii_case("false") {
        return;
    }
    let state = std::sync::Arc::clone(state);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if health_snapshot(&state).await.0 {
            tracing::info!("plugin host already running at {}", state.plugin_host);
            return;
        }
        let Some(dll) = find_host_dll() else {
            tracing::info!(
                "plugin host dll not found — start with: dotnet run --project dotnet/Cocktail.PluginHost"
            );
            return;
        };
        let plane = plane_url_for_host(&state.bind);
        let token = state.plugin_token.clone();
        let api_token = state.env_api_token.clone();
        let plugin_dir = std::env::var("COCKTAIL_PLUGIN_DIR").unwrap_or_else(|_| {
            if PathBuf::from("dotnet/dist/plugins").is_dir() {
                "dotnet/dist/plugins".into()
            } else {
                "data/extensions".into()
            }
        });
        tracing::info!(path = %dll.display(), "starting .NET plugin host");
        let mut cmd = Command::new("dotnet");
        cmd.arg(&dll)
            .env("COCKTAIL_PLUGIN_TOKEN", &token)
            .env("COCKTAIL_PLANE", plane)
            .env("COCKTAIL_PLUGIN_DIR", plugin_dir)
            .env("COCKTAIL_DATA", "data")
            .kill_on_drop(true);
        if let Some(t) = api_token {
            cmd.env("COCKTAIL_API_TOKEN", t);
        }
        match cmd.status().await {
            Ok(st) => tracing::warn!(code = ?st.code(), "plugin host exited"),
            Err(e) => tracing::warn!(error = %e, "failed to spawn plugin host (install .NET 8 SDK)"),
        }
    });
}

fn plane_url_for_host(bind: &str) -> String {
    if let Ok(v) = std::env::var("COCKTAIL_PLANE") {
        if !v.is_empty() {
            return v;
        }
    }
    let port = bind.rsplit(':').next().unwrap_or("11011");
    format!("http://127.0.0.1:{port}")
}

fn find_host_dll() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("COCKTAIL_PLUGIN_HOST_DLL") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    [
        "dotnet/Cocktail.PluginHost/bin/Debug/net8.0/Cocktail.PluginHost.dll",
        "dotnet/Cocktail.PluginHost/bin/Release/net8.0/Cocktail.PluginHost.dll",
        "dotnet/dist/Cocktail.PluginHost/Cocktail.PluginHost.dll",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

pub async fn post_event(state: &AppState, event: &InstanceEvent) -> anyhow::Result<()> {
    let url = format!("{}/v1/events", state.plugin_host.trim_end_matches('/'));
    let res = state
        .http
        .post(url)
        .header("X-Cocktail-Plugin", &state.plugin_token)
        .json(event)
        .timeout(Duration::from_millis(800))
        .send()
        .await?;
    if !res.status().is_success() {
        anyhow::bail!("plugin host {}", res.status());
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CatalogResponse {
    pub items: Vec<serde_json::Value>,
}

pub async fn catalog(state: &AppState) -> anyhow::Result<Vec<serde_json::Value>> {
    let url = format!("{}/v1/catalog", state.plugin_host.trim_end_matches('/'));
    let res = state
        .http
        .get(url)
        .header("X-Cocktail-Plugin", &state.plugin_token)
        .timeout(Duration::from_secs(3))
        .send()
        .await?;
    if !res.status().is_success() {
        anyhow::bail!("插件宿主不可用 ({})", res.status());
    }
    let body: CatalogResponse = res.json().await?;
    Ok(body.items)
}

pub async fn reload(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let url = format!("{}/v1/reload", state.plugin_host.trim_end_matches('/'));
    let res = state
        .http
        .post(url)
        .header("X-Cocktail-Plugin", &state.plugin_token)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    if !res.status().is_success() {
        anyhow::bail!("reload failed: {}", res.status());
    }
    Ok(res.json().await.unwrap_or_else(|_| json!({ "ok": true })))
}

pub async fn set_enabled(state: &AppState, id: &str, enabled: bool) -> anyhow::Result<serde_json::Value> {
    let url = format!(
        "{}/v1/plugins/{id}/enabled",
        state.plugin_host.trim_end_matches('/')
    );
    let res = state
        .http
        .put(url)
        .header("X-Cocktail-Plugin", &state.plugin_token)
        .json(&json!({ "enabled": enabled }))
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !res.status().is_success() {
        anyhow::bail!("set enabled failed: {}", res.status());
    }
    Ok(res.json().await.unwrap_or_else(|_| json!({ "ok": true })))
}

pub async fn health_snapshot(state: &AppState) -> (bool, usize) {
    let url = format!("{}/health", state.plugin_host.trim_end_matches('/'));
    match state.http.get(url).timeout(Duration::from_millis(400)).send().await {
        Ok(res) if res.status().is_success() => {
            let n = res
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("plugins").and_then(|p| p.as_u64()))
                .unwrap_or(0) as usize;
            (true, n)
        }
        _ => (false, 0),
    }
}

pub async fn proxy(
    state: &AppState,
    plugin_id: &str,
    rest: &str,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let rest = rest.trim_start_matches('/');
    let url = if rest.is_empty() {
        format!(
            "{}/v1/ext/{plugin_id}",
            state.plugin_host.trim_end_matches('/')
        )
    } else {
        format!(
            "{}/v1/ext/{plugin_id}/{rest}",
            state.plugin_host.trim_end_matches('/')
        )
    };
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let mut req = state.http.request(method, url);
    req = req
        .header("X-Cocktail-Plugin", &state.plugin_token)
        .timeout(Duration::from_secs(60));
    if let Some(ct) = headers.get(axum::http::header::CONTENT_TYPE) {
        req = req.header(axum::http::header::CONTENT_TYPE, ct);
    }
    if !body.is_empty() {
        req = req.body(body);
    }
    match req.send().await {
        Ok(res) => {
            let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = res
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            match res.bytes().await {
                Ok(bytes) => {
                    let mut response = Response::new(Body::from(bytes));
                    *response.status_mut() = status;
                    if let Ok(v) = content_type.parse() {
                        response.headers_mut().insert(axum::http::header::CONTENT_TYPE, v);
                    }
                    response
                }
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    axum::Json(json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({ "error": format!("插件宿主不可用: {e}") })),
        )
            .into_response(),
    }
}
