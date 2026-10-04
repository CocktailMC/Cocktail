

pub mod agent_runtime;
mod api;
mod automations;
mod auth;
mod rcon;
mod totp;
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
    crate::util::migrate_audit_jsonl();

    
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
                .allow_origin(cors_origins())
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderName::from_static("x-cocktail-csrf"),
                ])
                .allow_credentials(true),
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

fn required_perm(method: &axum::http::Method, path: &str) -> &'static str {
    let read = matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD);
    if path.starts_with("/api/v1/users") {
        return "users";
    }
    if path.starts_with("/api/v1/nodes") {
        return if read { "nodes" } else { "nodes.manage" };
    }
    if path.starts_with("/api/v1/netops") || path.starts_with("/api/v1/network") {
        return if read { "netops.view" } else { "netops.write" };
    }
    if path.starts_with("/api/v1/auth/2fa") {
        return if read { "view" } else { "2fa.manage" };
    }
    if path.starts_with("/api/v1/audit")
        || path.starts_with("/api/v1/settings")
        || path.starts_with("/api/v1/extensions")
        || path.starts_with("/api/v1/ext/")
        || path.starts_with("/api/v1/java")
        || path.starts_with("/api/v1/docker")
        || path.starts_with("/api/v1/qqbot")
    {
        return if read { "view" } else { "settings" };
    }
    if path.starts_with("/api/v1/automations") || path.starts_with("/api/v1/schedules") {
        return if read { "view" } else { "automations" };
    }
    if path.starts_with("/api/v1/fleet") {
        return if read { "view" } else { "stop" };
    }
    if path.starts_with("/api/v1/cores") {
        return if read { "view" } else { "plugins.install" };
    }
    if path.starts_with("/api/v1/instances") {
        if path.ends_with("/start") {
            return "start";
        }
        if path.ends_with("/stop") || path.ends_with("/restart") {
            return "stop";
        }
        if path.contains("/files") {
            return if read { "files.read" } else { "files.write" };
        }
        if path.contains("/plugins") {
            return if read { "view" } else { "plugins" };
        }
        if path.contains("/modrinth")
            || path.contains("/hangar")
            || path.contains("/spiget")
            || path.contains("/install")
            || path.contains("/startup-jar")
            || path.contains("/import-archive")
        {
            return if read { "view" } else { "plugins" };
        }
        if path.contains("/players/") {
            if path.ends_with("/detail") {
                return "players.detail";
            }
            if path.ends_with("/rcon") {
                return "rcon";
            }
            if path.ends_with("/kick") {
                return "players.kick";
            }
            if path.ends_with("/ban") {
                return "players.ban";
            }
            if path.ends_with("/pardon") {
                return "players.pardon";
            }
            if path.ends_with("/op") {
                return "players.op";
            }
            if path.ends_with("/deop") {
                return "players.deop";
            }
            if path.ends_with("/whitelist") || path.ends_with("/unwhitelist") {
                return "players.whitelist";
            }
            return if read { "view" } else { "players" };
        }
        if path.contains("/players") {
            return if read { "view" } else { "players" };
        }
        if path.contains("/rcon/") {
            if path.ends_with("/setup") {
                return "rcon.setup";
            }
            return "rcon";
        }
        if path.contains("/backups") || path.contains("/worlds") {
            return if read { "view" } else { "backups" };
        }
        if path.contains("/command") || path.contains("/logs") {
            return if read { "view" } else { "console" };
        }
        return if read { "view" } else { "settings" };
    }
    if read { "view" } else { "settings" }
}

fn is_mutation(method: &axum::http::Method) -> bool {
    !matches!(
        *method,
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    )
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
    let cookie_token = req
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(cookie_value);
    let query_token = if query_token_allowed(req.method(), req.uri().path()) {
        req.uri().query().and_then(|q| {
            q.split('&').find_map(|pair| {
                let mut kv = pair.splitn(2, '=');
                if kv.next() == Some("token") {
                    kv.next().map(percent_decode)
                } else {
                    None
                }
            })
        })
    } else {
        None
    };
    let token = match bearer.or(cookie_token).or(query_token) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "未授权：请登录或提供有效 Token" })),
            )
                .into_response();
        }
    };

    let method = req.method().clone();
    let is_machine = state
        .env_api_token
        .as_ref()
        .is_some_and(|t| t == &token)
        || (!state.plugin_token.is_empty() && state.plugin_token == token);
    let (role, csrf, actor) = if is_machine {
        let owner = {
            let conn = state.db.lock().await;
            crate::db::superadmin(&conn).ok().flatten()
        };
        match owner {
            Some(a) => (a.role, String::new(), "machine".to_string()),
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "机器 Token 无效" })),
                )
                    .into_response();
            }
        }
    } else {
        let session = {
            let conn = state.db.lock().await;
            crate::db::session_lookup(&conn, &token).ok().flatten()
        };
        match session {
            Some(s) => (s.admin.role, s.csrf_token, s.admin.username),
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "登录已过期，请重新登录" })),
                )
                    .into_response();
            }
        }
    };

    if is_mutation(&method) && !is_machine && !csrf.is_empty() {
        let provided = req
            .headers()
            .get("x-cocktail-csrf")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if provided != csrf {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "CSRF 校验失败，请刷新页面后重试" })),
            )
                .into_response();
        }
    }

    let perm = required_perm(&method, &path);
    if !crate::auth::can(&role, perm) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": format!("权限不足：需要 {perm} 权限（当前角色 {role}）"),
                "code": "forbidden",
                "required": perm,
            })),
        )
            .into_response();
    }

    crate::util::with_actor(actor, next.run(req)).await
}

fn cors_origins() -> tower_http::cors::AllowOrigin {
    let mut origins: Vec<axum::http::HeaderValue> = Vec::new();
    if let Ok(raw) = std::env::var("COCKTAIL_CORS_ORIGINS") {
        for part in raw.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(v) = axum::http::HeaderValue::from_str(trimmed) {
                origins.push(v);
            }
        }
    }
    for fallback in [
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://127.0.0.1:11011",
        "http://localhost:11011",
    ] {
        if let Ok(v) = axum::http::HeaderValue::from_str(fallback) {
            origins.push(v);
        }
    }
    tower_http::cors::AllowOrigin::list(origins)
}

fn query_token_allowed(method: &axum::http::Method, path: &str) -> bool {
    matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD)
        && (path.ends_with("/files/download")
            || path.ends_with("/worlds/download")
            || path.contains("/worlds/")
            || path.ends_with("/logs/ws")
            || path.ends_with("/events/ws"))
}

fn cookie_value(raw: &str) -> Option<String> {
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some(v) = pair.strip_prefix("cocktail_token=") {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
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
