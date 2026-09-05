//! Cocktail Manager control plane — v0.1 (26Q3)

mod api;
mod auth;
mod db;
mod instance;
mod platform;
mod state;
mod util;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::{AppState, SharedState};

/// Matches files::MAX_UPLOAD_BYTES (512 MiB) for jar / world uploads.
const MAX_BODY_BYTES: usize = 512 * 1024 * 1024;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("cocktail_control=info,tower_http=info")
        }))
        .init();

    let state = Arc::new(AppState::new());
    state.spawn_event_applier();
    state.spawn_scheduler();

    let api = api::router()
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware,
        ))
        .with_state(Arc::clone(&state));

    let mut app = Router::new().merge(api).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    )
    .layer(TraceLayer::new_for_http());

    if let Some(web_root) = resolve_web_root() {
        tracing::info!(path = %web_root.display(), "serving admin UI");
        let index = web_root.join("index.html");
        let static_files = ServeDir::new(&web_root)
            .append_index_html_on_directories(true)
            .not_found_service(ServeFile::new(index));
        app = app.fallback_service(static_files);
    } else {
        tracing::warn!(
            "admin UI not found — set COCKTAIL_WEB_ROOT or place files in ./web or ./admin/dist"
        );
    }

    let addr: SocketAddr = std::env::var("COCKTAIL_BIND")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "0.0.0.0:11011".parse().unwrap());
    tracing::info!(%addr, "Cocktail Manager control plane v0.1 listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn resolve_web_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("COCKTAIL_WEB_ROOT") {
        let path = PathBuf::from(p);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }
    for candidate in ["web", "admin/dist"] {
        let path = PathBuf::from(candidate);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }
    // Next to the executable (packaged installs).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["web", "../share/cocktail/web"] {
                let path = dir.join(name);
                if path.join("index.html").is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

use axum::Json;
use serde_json::json;

async fn auth_middleware(
    State(state): State<SharedState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if matches!(
        path.as_str(),
        "/api/v1/health" | "/api/v1/setup" | "/api/v1/auth/login"
    ) {
        return next.run(req).await;
    }

    let conn = state.db.lock().await;
    let needs_setup = crate::auth::setup_required(&conn).unwrap_or(true);
    drop(conn);
    if needs_setup {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "需要先完成最高管理员初始化",
                "code": "setup_required"
            })),
        )
            .into_response();
    }

    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let query_token = req.uri().query().and_then(|q| {
        q.split('&').find_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            if kv.next() == Some("token") {
                kv.next().map(|t| {
                    percent_decode(t)
                })
            } else {
                None
            }
        })
    });
    let token = bearer.or(query_token);

    if let Some(token) = token {
        if state.bearer_ok(&token).await {
            return next.run(req).await;
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "未授权：请登录或提供有效 Token" })),
    )
        .into_response()
}

fn percent_decode(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
