//! Cocktail Manager control plane library (shared with `cocktail-agent`).

pub mod agent_runtime;
mod api;
mod automations;
mod auth;
mod cluster;
mod db;
mod hostnet;
mod http;
mod instance;
mod java;
mod netops;
mod ops;
mod platform;
mod plugin_bridge;
mod proto;
mod qqbot;
mod sevenz;
mod state;
mod stdin_bridge;
mod util;
mod wincompat;
mod winnet;

pub use stdin_bridge::run_stdin_bridge;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::Router;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::{AppState, SharedState};

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// CLI 子命令:重置 super-admin 密码
///
/// 用法:`cocktail-control reset-password`
///
/// 适用于忘记密码、无法登录控制面的恢复场景。
/// 直接读写 SQLite,不需要控制面进程运行,不需要当前密码。
/// 注意:密码在终端会明文回显(单机本地场景风险可控)。
pub fn run_reset_password() -> anyhow::Result<()> {
    use std::io::{self, Write};

    let conn = db::open()?;
    let admin = db::superadmin(&conn)?
        .ok_or_else(|| anyhow::anyhow!("尚未初始化,请先启动控制面完成 setup"))?;

    println!("正在重置 super-admin [{}] 的密码", admin.username);
    println!("提示:密码在终端会明文显示,请确保周围无人窥屏。");

    let password = read_password_line("新密码(至少 8 位):")?;
    auth::validate_password(&password).map_err(|e| anyhow::anyhow!("密码不符合要求: {e}"))?;

    let confirm = read_password_line("再次输入新密码:")?;
    if password != confirm {
        anyhow::bail!("两次输入不一致,已取消");
    }

    let hash = auth::hash_password(&password)?;
    db::update_admin(&conn, admin.id, None, Some(&hash))?;

    util::audit(
        "auth.reset_password",
        None,
        json!({ "username": admin.username }),
        "cli",
    );

    println!("\n✓ 用户 [{}] 的密码已重置,可使用新密码登录控制面", admin.username);
    Ok(())
}

fn read_password_line(prompt: &str) -> anyhow::Result<String> {
    use std::io::{self, Write};
    print!("{prompt}");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

pub async fn run_plane() -> anyhow::Result<()> {
    crate::wincompat::enable_utf8_console();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("cocktail_control=info,tower_http=info")
        }))
        .init();

    let state = Arc::new(AppState::new());

    // Detect and initialise the container runtime (Docker or Podman).
    match instance::runtime::ContainerRuntime::detect().await {
        Ok(rt) => {
            let engine = rt.engine.as_str();
            tracing::info!(engine, "container runtime initialised");
            instance::runtime::set_runtime(rt);
        }
        Err(e) => {
            tracing::warn!(error = %e, "no container runtime detected; docker/podman instances unavailable");
        }
    }

    state.spawn_event_applier();
    instance::reattach_running(&state).await;
    state.spawn_scheduler();
    state.spawn_reconciler();
    crate::ops::spawn(&state);

    let api = api::router()
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware,
        ))
        .with_state(Arc::clone(&state));
    let _ = state.plane.set(api.clone());

    crate::plugin_bridge::spawn_event_forwarder(&state);
    crate::plugin_bridge::maybe_autostart(&state);

    let mut app = Router::new()
        .merge(api)
        .layer(
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

async fn auth_middleware(
    State(state): State<SharedState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if matches!(
        path.as_str(),
        "/api/v1/health" | "/api/v1/setup" | "/api/v1/auth/login" | "/api/v1/agent/ws"
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
                kv.next().map(percent_decode)
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
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
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
