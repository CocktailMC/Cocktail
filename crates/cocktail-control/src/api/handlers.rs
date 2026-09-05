use axum::body::Body;
use axum::extract::multipart::Multipart;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

use crate::instance::{
    self, BulkActionRequest, CommandRequest, CreateInstanceRequest, CreateScheduleRequest,
    EulaRequest, HangarVersionsQuery, InstallHangarRequest, InstallModrinthRequest,
    InstallRequest, InstallSpigetRequest, InstanceEvent, PlayerActionRequest, PropertiesUpdate,
    UpdateInstanceRequest, WriteFileRequest,
};
use crate::instance::{ModrinthVersionsQuery, SearchQuery};
use crate::platform;
use crate::state::SharedState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub name: &'static str,
    pub version: &'static str,
    pub release: &'static str,
    pub status: &'static str,
    pub auth_required: bool,
    pub setup_required: bool,
    pub panel_name: String,
    pub admin_username: Option<String>,
    pub os: String,
    pub arch: String,
    pub family: String,
    pub hostname: String,
    pub distro_id: String,
    pub distro_name: String,
    pub distro_version: String,
    pub kernel: String,
    pub wsl: bool,
}

pub async fn health(State(state): State<SharedState>) -> Json<HealthResponse> {
    let p = platform::detect();
    let conn = state.db.lock().await;
    let setup_required = crate::auth::setup_required(&conn).unwrap_or(true);
    let panel_name = crate::db::panel(&conn)
        .map(|r| r.panel_name)
        .unwrap_or_else(|_| "Cocktail Manager".into());
    let admin_username = crate::db::superadmin(&conn)
        .ok()
        .flatten()
        .map(|a| a.username);
    drop(conn);
    Json(HealthResponse {
        name: "cocktail-control",
        version: env!("CARGO_PKG_VERSION"),
        release: "26Q3",
        status: "ok",
        auth_required: !setup_required || state.env_api_token.is_some(),
        setup_required,
        panel_name,
        admin_username,
        os: p.os,
        arch: p.arch,
        family: p.family,
        hostname: p.hostname,
        distro_id: p.distro_id,
        distro_name: p.distro_name,
        distro_version: p.distro_version,
        kernel: p.kernel,
        wsl: p.wsl,
    })
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub panel_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub token: String,
    pub username: String,
    pub panel_name: String,
    pub role: String,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub username: String,
    pub role: String,
    pub panel_name: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub panel_name: String,
    pub webhook_url: Option<String>,
    pub env_webhook_set: bool,
    pub env_api_token_set: bool,
    pub admin_username: String,
    pub admin_created_at: String,
    pub bind: String,
    pub db_path: &'static str,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub panel_name: Option<String>,
    pub webhook_url: Option<String>,
    pub username: Option<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string)
}

pub async fn setup(
    State(state): State<SharedState>,
    Json(body): Json<SetupRequest>,
) -> impl IntoResponse {
    let username = match crate::auth::validate_username(&body.username) {
        Ok(u) => u,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    if let Err(e) = crate::auth::validate_password(&body.password) {
        return bad_request(e.to_string()).into_response();
    }
    let hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };

    let conn = state.db.lock().await;
    match crate::auth::setup_required(&conn) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::CONFLICT,
                Json(ErrorBody {
                    error: "最高管理员已初始化".into(),
                }),
            )
                .into_response();
        }
        Err(e) => return bad_request(e.to_string()).into_response(),
    }

    if let Some(name) = body.panel_name.as_deref() {
        if let Err(e) = crate::db::update_panel(&conn, Some(name), None) {
            return bad_request(e.to_string()).into_response();
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let admin = match crate::db::insert_superadmin(&conn, &username, &hash, &now) {
        Ok(a) => a,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    let token = match crate::auth::create_session(&conn, &admin) {
        Ok(t) => t,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    let panel_name = crate::db::panel(&conn)
        .map(|p| p.panel_name)
        .unwrap_or_else(|_| "Cocktail Manager".into());
    crate::util::audit(
        "auth.setup",
        None,
        serde_json::json!({ "username": admin.username }),
        "setup",
    );
    Json(SessionResponse {
        token,
        username: admin.username,
        panel_name,
        role: admin.role,
    })
    .into_response()
}

pub async fn login(
    State(state): State<SharedState>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let Some(admin) = crate::db::superadmin(&conn).ok().flatten() else {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "需要先完成最高管理员初始化".into(),
            }),
        )
            .into_response();
    };
    if !admin.username.eq_ignore_ascii_case(body.username.trim())
        || !crate::auth::verify_password(&body.password, &admin.password_hash)
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "用户名或密码不正确".into(),
            }),
        )
            .into_response();
    }
    let token = match crate::auth::create_session(&conn, &admin) {
        Ok(t) => t,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    let panel_name = crate::db::panel(&conn)
        .map(|p| p.panel_name)
        .unwrap_or_else(|_| "Cocktail Manager".into());
    crate::util::audit(
        "auth.login",
        None,
        serde_json::json!({ "username": admin.username }),
        "login",
    );
    Json(SessionResponse {
        token,
        username: admin.username,
        panel_name,
        role: admin.role,
    })
    .into_response()
}

pub async fn logout(State(state): State<SharedState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = bearer_from_headers(&headers) {
        let conn = state.db.lock().await;
        let _ = crate::db::delete_session(&conn, &token);
    }
    StatusCode::NO_CONTENT
}

pub async fn me(State(state): State<SharedState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(token) = bearer_from_headers(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
            .into_response();
    };
    let conn = state.db.lock().await;
    let admin = if state
        .env_api_token
        .as_ref()
        .is_some_and(|t| t == &token)
    {
        crate::db::superadmin(&conn).ok().flatten()
    } else {
        crate::db::session_admin(&conn, &token).ok().flatten()
    };
    let Some(admin) = admin else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
            .into_response();
    };
    let panel_name = crate::db::panel(&conn)
        .map(|p| p.panel_name)
        .unwrap_or_else(|_| "Cocktail Manager".into());
    Json(MeResponse {
        username: admin.username,
        role: admin.role,
        panel_name,
        created_at: admin.created_at,
    })
    .into_response()
}

pub async fn get_settings(State(state): State<SharedState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let Ok(panel) = crate::db::panel(&conn) else {
        return bad_request("无法读取面板设置").into_response();
    };
    let Some(admin) = crate::db::superadmin(&conn).ok().flatten() else {
        return bad_request("尚未初始化最高管理员").into_response();
    };
    Json(SettingsResponse {
        panel_name: panel.panel_name,
        webhook_url: panel.webhook_url,
        env_webhook_set: state.env_webhook_url.is_some(),
        env_api_token_set: state.env_api_token.is_some(),
        admin_username: admin.username,
        admin_created_at: admin.created_at,
        bind: state.bind.clone(),
        db_path: crate::db::DB_PATH,
    })
    .into_response()
}

pub async fn update_settings(
    State(state): State<SharedState>,
    Json(body): Json<UpdateSettingsRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let webhook = body.webhook_url.as_deref().map(Some);
    if let Err(e) = crate::db::update_panel(&conn, body.panel_name.as_deref(), webhook) {
        return bad_request(e.to_string()).into_response();
    }
    if let Some(new_name) = body.username.as_deref() {
        match crate::auth::validate_username(new_name) {
            Ok(name) => {
                if let Some(admin) = crate::db::superadmin(&conn).ok().flatten() {
                    if let Err(e) = crate::db::update_admin(&conn, admin.id, Some(&name), None) {
                        return bad_request(e.to_string()).into_response();
                    }
                }
            }
            Err(e) => return bad_request(e.to_string()).into_response(),
        }
    }
    crate::util::audit(
        "settings.update",
        None,
        serde_json::json!({}),
        "api",
    );
    drop(conn);
    get_settings(State(state)).await.into_response()
}

pub async fn change_password(
    State(state): State<SharedState>,
    Json(body): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    if let Err(e) = crate::auth::validate_password(&body.new_password) {
        return bad_request(e.to_string()).into_response();
    }
    let conn = state.db.lock().await;
    let Some(admin) = crate::db::superadmin(&conn).ok().flatten() else {
        return bad_request("尚未初始化最高管理员").into_response();
    };
    if !crate::auth::verify_password(&body.current_password, &admin.password_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "当前密码不正确".into(),
            }),
        )
            .into_response();
    }
    let hash = match crate::auth::hash_password(&body.new_password) {
        Ok(h) => h,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    if let Err(e) = crate::db::update_admin(&conn, admin.id, None, Some(&hash)) {
        return bad_request(e.to_string()).into_response();
    }
    crate::util::audit("auth.password", None, serde_json::json!({}), "api");
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Serialize)]
pub struct ErrorBody {
    error: String,
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

#[derive(Deserialize)]
pub struct UploadQuery {
    path: String,
}

pub async fn list_instances(State(state): State<SharedState>) -> impl IntoResponse {
    Json(instance::list_instances(&state).await)
}

pub async fn get_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::get_instance(&state, &id)
        .await
        .map(Json)
        .ok_or_else(|| not_found("instance not found"))
}

pub async fn create_instance(
    State(state): State<SharedState>,
    Json(req): Json<CreateInstanceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if req.name.trim().is_empty() {
        return Err(bad_request("name is required"));
    }
    instance::create_instance(&state, req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn update_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateInstanceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::update_instance(&state, &id, req).await)
}

pub async fn accept_eula(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<EulaRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::accept_eula(&state, &id, req).await)
}

pub async fn start_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::start_instance(&state, &id).await)
}

pub async fn stop_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::stop_instance(&state, &id).await)
}

pub async fn restart_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::restart_instance(&state, &id).await)
}

pub async fn delete_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::delete_instance(&state, &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(not_found(e.to_string())),
    }
}

pub async fn send_command(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::send_command(&state, &id, req).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn recent_logs(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if instance::get_instance(&state, &id).await.is_none() {
        return Err(not_found("instance not found"));
    }
    Ok(Json(state.recent_logs(&id).await))
}

pub async fn list_files(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_files(&state, &id, &q.path).await)
}

pub async fn read_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::read_file(&state, &id, &q.path).await)
}

pub async fn write_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<WriteFileRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::write_file(&state, &id, &req.path, &req.content).await)
}

pub async fn delete_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::delete_file(&state, &id, &q.path).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn download_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let (path, bytes) = instance::read_bytes(&state, &id, &q.path)
        .await
        .map_err(|e| {
            if e.to_string().contains("not found") {
                not_found(e.to_string())
            } else {
                bad_request(e.to_string())
            }
        })?;
    let filename = path.rsplit('/').next().unwrap_or("download");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(bytes))
        .unwrap())
}

pub async fn upload_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<UploadQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        if field.name() == Some("file") || bytes.is_none() {
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(e.to_string()))?
                    .to_vec(),
            );
        }
    }
    let bytes = bytes.ok_or_else(|| bad_request("missing file field"))?;
    map_result(instance::write_bytes(&state, &id, &q.path, &bytes).await)
}

#[derive(Deserialize)]
pub struct MkdirBody {
    pub path: String,
}

pub async fn mkdir(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<MkdirBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::mkdir(&state, &id, &body.path).await)
}

#[derive(Deserialize)]
pub struct InstallJarQuery {
    #[serde(default = "default_server_jar")]
    pub path: String,
    #[serde(default = "default_custom_core")]
    pub core: String,
    #[serde(default = "default_true_bool")]
    pub accept_eula: bool,
}

fn default_server_jar() -> String {
    "server.jar".into()
}

fn default_custom_core() -> String {
    "custom".into()
}

fn default_true_bool() -> bool {
    true
}

pub async fn install_jar(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<InstallJarQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        if field.name() == Some("file") || bytes.is_none() {
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(e.to_string()))?
                    .to_vec(),
            );
        }
    }
    let bytes = bytes.ok_or_else(|| bad_request("missing file field"))?;
    map_result(
        instance::install_local_jar(
            &state,
            &id,
            &q.path,
            &bytes,
            Some(q.core),
            q.accept_eula,
        )
        .await,
    )
}

#[derive(Deserialize)]
pub struct StartupJarBody {
    pub path: String,
}

pub async fn set_startup_jar(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<StartupJarBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_startup_jar(&state, &id, &body.path).await)
}

pub async fn list_backups(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_backups(&state, &id).await)
}

pub async fn create_backup(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::create_backup(&state, &id).await)
}

pub async fn delete_backup(
    State(state): State<SharedState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::delete_backup(&state, &id, &backup_id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn restore_backup(
    State(state): State<SharedState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::restore_backup(&state, &id, &backup_id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn get_properties(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::get_properties(&state, &id).await)
}

pub async fn set_properties(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<PropertiesUpdate>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_properties(&state, &id, &req.entries).await)
}

pub async fn list_plugins(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_plugins(&state, &id).await)
}

pub async fn enable_plugin(
    State(state): State<SharedState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_plugin_enabled(&state, &id, &name, true).await)
}

pub async fn disable_plugin(
    State(state): State<SharedState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_plugin_enabled(&state, &id, &name, false).await)
}

pub async fn list_core_versions(
    Path(core): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_versions(&core).await)
}

pub async fn install_core(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_core(&state, &id, req).await)
}

pub async fn modrinth_search(
    Query(q): Query<SearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::modrinth::search(&q).await)
}

pub async fn modrinth_versions(
    Path(id): Path<String>,
    Query(q): Query<ModrinthVersionsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::modrinth::list_versions(&id, &q).await)
}

pub async fn modrinth_install(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallModrinthRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_modrinth(&state, &id, req).await)
}

#[derive(Deserialize)]
pub struct HangarSearchQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_20")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_paper")]
    pub platform: String,
}

fn default_20() -> u32 {
    20
}

fn default_paper() -> String {
    "PAPER".into()
}

pub async fn hangar_search(
    Query(q): Query<HangarSearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(
        instance::hangar::search(&instance::hangar::SearchQuery {
            query: q.query,
            limit: q.limit,
            offset: q.offset,
            platform: q.platform,
        })
        .await,
    )
}

pub async fn hangar_versions(
    Path(slug): Path<String>,
    Query(q): Query<HangarVersionsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::hangar::list_versions(&slug, &q).await)
}

pub async fn hangar_install(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallHangarRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_hangar(&state, &id, req).await)
}

#[derive(Deserialize)]
pub struct SpigetSearchQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_20")]
    pub size: u32,
    #[serde(default)]
    pub page: u32,
}

pub async fn spiget_search(
    Query(q): Query<SpigetSearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(
        instance::spiget::search(&instance::spiget::SearchQuery {
            query: q.query,
            size: q.size,
            page: q.page,
        })
        .await,
    )
}

pub async fn spiget_versions(
    Path(rid): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::spiget::list_versions(rid).await)
}

pub async fn spiget_icon(
    Path(rid): Path<i64>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    match instance::spiget::fetch_icon(rid).await {
        Ok((ctype, bytes)) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctype)
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(bytes))
            .unwrap()),
        Err(e) => Err(not_found(e.to_string())),
    }
}

pub async fn spiget_install(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallSpigetRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_spiget(&state, &id, req).await)
}

#[derive(Deserialize)]
pub struct PlayersQuery {
    /// If true, send `list` to the running server (appears in console).
    #[serde(default)]
    pub probe: bool,
}

pub async fn list_players(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PlayersQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if q.probe {
        map_result(instance::probe_players(&state, &id).await)
    } else {
        map_result(instance::list_players(&state, &id).await)
    }
}

pub async fn player_action(
    State(state): State<SharedState>,
    Path((id, name, action)): Path<(String, String, String)>,
    body: Option<Json<PlayerActionRequest>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let reason = body.and_then(|Json(b)| b.reason);
    match instance::player_action(&state, &id, &name, &action, reason).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn list_worlds(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_worlds(&state, &id).await)
}

pub async fn reset_world(
    State(state): State<SharedState>,
    Path((id, world)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::reset_world(&state, &id, &world).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn export_world(
    State(state): State<SharedState>,
    Path((id, world)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::export_world(&state, &id, &world).await)
}

pub async fn import_world(
    State(state): State<SharedState>,
    Path((id, world)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        bytes = Some(
            field
                .bytes()
                .await
                .map_err(|e| bad_request(e.to_string()))?
                .to_vec(),
        );
    }
    let bytes = bytes.ok_or_else(|| bad_request("missing file"))?;
    match instance::import_world(&state, &id, &world, &bytes).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn list_schedules(State(state): State<SharedState>) -> impl IntoResponse {
    Json(instance::list_schedules(&state).await)
}

pub async fn create_schedule(
    State(state): State<SharedState>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::create_schedule(&state, req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn delete_schedule(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::delete_schedule(&state, &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(not_found(e.to_string())),
    }
}

pub async fn fleet_summary(State(state): State<SharedState>) -> impl IntoResponse {
    Json(instance::fleet_summary(&state).await)
}

pub async fn fleet_bulk(
    State(state): State<SharedState>,
    Json(req): Json<BulkActionRequest>,
) -> impl IntoResponse {
    Json(instance::bulk_action(&state, req).await)
}

pub async fn docker_status() -> impl IntoResponse {
    Json(instance::docker_engine_status().await)
}

pub async fn events_ws(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_socket(socket, state))
}

pub async fn logs_ws(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_socket(socket, state, id))
}

async fn handle_events_socket(socket: WebSocket, state: SharedState) {
    let mut rx = state.events.subscribe();
    let (mut tx, mut inbound) = socket.split();
    let send_loop = async {
        loop {
            match rx.recv().await {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(payload) => {
                        if tx.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    let recv_loop = async {
        while let Some(Ok(msg)) = inbound.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    };
    tokio::select! {
        _ = send_loop => {},
        _ = recv_loop => {},
    }
}

async fn handle_logs_socket(socket: WebSocket, state: SharedState, id: String) {
    let recent = state.recent_logs(&id).await;
    let mut rx = state.events.subscribe();
    let (mut tx, mut inbound) = socket.split();
    for line in recent {
        if let Ok(payload) = serde_json::to_string(&line) {
            if tx.send(Message::Text(payload.into())).await.is_err() {
                return;
            }
        }
    }
    let send_loop = async {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let InstanceEvent::Log { instance_id, line } = &event else {
                        continue;
                    };
                    if instance_id != &id {
                        continue;
                    }
                    match serde_json::to_string(line) {
                        Ok(payload) => {
                            if tx.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    let recv_loop = async {
        while let Some(Ok(msg)) = inbound.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    };
    tokio::select! {
        _ = send_loop => {},
        _ = recv_loop => {},
    }
}

fn map_result<T: Serialize>(
    result: anyhow::Result<T>,
) -> Result<Json<T>, (StatusCode, Json<ErrorBody>)> {
    match result {
        Ok(v) => Ok(Json(v)),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: msg.into(),
        }),
    )
}

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: msg.into(),
        }),
    )
}
