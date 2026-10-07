pub mod agent_runtime;
mod api;
mod auth;
mod automations;
pub mod backup;
mod cluster;
mod community;
pub mod crypto;
mod db;
mod diagnostics;
mod hardening;
mod hostnet;
mod http;
mod i18n;
mod identity;
mod init_client;
mod instance;
mod java;
mod metrics;
mod netops;
mod openapi;
mod ops;
mod ops_extra;
mod platform;
mod plugin_bridge;
mod plugins;
mod proto;
mod qqbot;
mod rbac;
mod rcon;
pub mod secrets;
mod sevenz;
mod state;
mod stdin_bridge;
mod storage;
mod totp;
mod util;
mod wincompat;
mod winnet;
mod workflow;

pub use stdin_bridge::run_stdin_bridge;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::{AppState, SharedState};

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// 通用本地服务管理器：托管 cocktail-init 等本地子进程。
pub(crate) static SERVICE_SUPERVISOR: OnceLock<Arc<supervisor::ServiceSupervisor>> =
    OnceLock::new();

/// 托管服务名：cocktail-init。
pub(crate) const INIT_SERVICE: &str = "cocktail-init";

/// 调一个 cocktail-init RPC。失败（init 未启动 / IPC 失败 / RPC error / 正在重启）
/// 时返回 Err，调用方决定是否 fallback 到本地实现。
///
/// TODO 阶段 3：对未启用的模块提供本地 fallback 而不是直接报错。
pub(crate) async fn init_call<P: serde::Serialize>(
    method: &str,
    params: P,
) -> anyhow::Result<serde_json::Value> {
    let sup = SERVICE_SUPERVISOR.get().ok_or_else(|| {
        anyhow::anyhow!("cocktail-init subprocess not available (SERVICE_SUPERVISOR unset)")
    })?;
    let v = sup
        .ipc_call(INIT_SERVICE, method, params)
        .await
        .map_err(|e| anyhow::anyhow!("init RPC {method} failed: {e}"))?;
    Ok(v)
}

/// 注册并启动 cocktail-init 服务（Transport::Ipc），随后握手拉取 master key。
/// 失败不致命：secrets::master_key() fallback 到本地 load_or_create()，
/// 兼容单进程老部署与 init 二进制缺失场景。
async fn try_spawn_init() -> anyhow::Result<()> {
    use crate::supervisor::{
        PermissionBoundary, ResourceLimits, RestartPolicy, ServiceSpec, Transport,
    };

    let program = init_client::resolve_init_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "cocktail-init binary not found (checked COCKTAIL_INIT_PATH, current exe dir, PATH)"
        )
    })?;

    // 权限边界：可执行文件只能来自当前 exe 目录或 COCKTAIL_INIT_PATH 的父目录。
    let mut allowed_bin_roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            allowed_bin_roots.push(dir.to_path_buf());
        }
    }
    if let Ok(p) = std::env::var("COCKTAIL_INIT_PATH") {
        if let Some(parent) = PathBuf::from(p).parent() {
            allowed_bin_roots.push(parent.to_path_buf());
        }
    }

    let spec = ServiceSpec {
        name: INIT_SERVICE.to_string(),
        program,
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        transport: Transport::Ipc,
        depends_on: Vec::new(),
        restart: RestartPolicy::OnFailure {
            max_attempts: 5,
            backoff_ms: 1000,
            max_backoff_ms: 30000,
        },
        limits: ResourceLimits::default(),
        boundary: PermissionBoundary {
            allowed_bin_roots,
            allowed_workdir_roots: Vec::new(),
            // 剔除敏感变量，避免子进程继承 control 的密钥
            env_denylist: vec![
                "COCKTAIL_MASTER_KEY".to_string(),
                "COCKTAIL_INIT_KEY".to_string(),
            ],
        },
        log_dir: None,
        log_max_bytes: 8 * 1024 * 1024,
        log_keep: 3,
        stop_timeout: Duration::from_secs(15),
    };

    let sup = supervisor::ServiceSupervisor::new();
    sup.register(spec)
        .await
        .map_err(|e| anyhow::anyhow!("register cocktail-init: {e}"))?;
    // 存入全局，供 init_call 与 watcher / monitor 任务使用（Weak 升级）。
    let _ = SERVICE_SUPERVISOR.set(Arc::clone(&sup));
    sup.start(INIT_SERVICE)
        .await
        .map_err(|e| anyhow::anyhow!("start cocktail-init: {e}"))?;

    // 握手 master key 并注入 secrets。失败仅记日志（init 仍可用，secrets fallback）。
    match sup.ipc_get_master_key(INIT_SERVICE).await {
        Ok(key) if key.len() == 32 => {
            tracing::info!(
                source = "init-rpc",
                "master key loaded from cocktail-init subprocess"
            );
            crate::secrets::set_init_key(key);
        }
        Ok(other) => {
            tracing::warn!(
                len = other.len(),
                "cocktail-init returned master key with unexpected length; fallback to local file"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "cocktail-init get_master_key failed; fallback to local file");
        }
    }
    Ok(())
}

/// graceful shutdown：按依赖反向序停止所有托管服务（cocktail-init 关闭 stdin
/// 触发 EOF 自然退出）。由 run_plane 的 signal handler 在 axum::serve 退出后调用。
async fn shutdown_init() {
    if let Some(sup) = SERVICE_SUPERVISOR.get() {
        sup.shutdown_all().await;
    }
}

/// 等待 shutdown 信号（Ctrl+C / SIGTERM）。返回 () 后 axum::serve
/// 进入 graceful shutdown：停止接受新连接，等待 in-flight 请求完成。
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .unwrap_or_else(|e| tracing::warn!(error = %e, "ctrl_c signal handler error"));
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to install SIGTERM handler");
                // 没有 SIGTERM 时永远等不到，让 ctrl_c 主导
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C, initiating graceful shutdown"),
        _ = terminate => tracing::info!("received SIGTERM, initiating graceful shutdown"),
    }
}

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

    println!(
        "\n✓ 用户 [{}] 的密码已重置,可使用新密码登录控制面",
        admin.username
    );
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
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("cocktail_control=info,tower_http=info")),
        )
        .init();

    let state = Arc::new(AppState::new());
    crate::util::migrate_audit_jsonl();

    // 尝试 fork+exec cocktail-init 子进程并握手拉取 master key。
    // 失败不致命：secrets::master_key() 会 fallback 到本地 load_or_create()，
    // 兼容单进程老部署与 init 二进制缺失场景。
    if let Err(e) = try_spawn_init().await {
        tracing::warn!(error = %e, "cocktail-init subprocess unavailable; secrets will fallback to local file");
    }

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
    if !addr.ip().is_loopback() {
        tracing::warn!(
            %addr,
            "控制面板监听非环回地址。HTTP 明文 + 默认凭据传输不安全：\
             请在前端部署 TLS 反代（如 Caddy/Nginx），或显式设置 COCKTAIL_BIND=127.0.0.1:11011"
        );
    }
    tracing::info!(%addr, "Cocktail Manager control plane v0.1 listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    // 通知 init 子进程退出（关闭 stdin pipe，init 端 reader EOF 自然退出）
    shutdown_init().await;
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

    let conn = state.db.get().expect("db pool");
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
    let is_machine = state.env_api_token.as_ref().is_some_and(|t| t == &token)
        || (!state.plugin_token.is_empty() && state.plugin_token == token);
    let (role, csrf, actor) = if is_machine {
        let owner = {
            let conn = state.db.get().expect("db pool");
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
            let conn = state.db.get().expect("db pool");
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
