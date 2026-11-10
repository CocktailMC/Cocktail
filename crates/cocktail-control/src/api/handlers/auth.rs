use crate::api::handlers::{ErrorBody, bad_request, bearer_from_headers, current_admin, db_conn};
use crate::state::SharedState;
use crate::util;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub totp_code: Option<u32>,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub token: String,
    pub csrf_token: String,
    pub username: String,
    pub panel_name: String,
    pub role: String,
    pub permissions: Vec<String>,
    pub expires_at: String,
}
#[derive(Serialize)]
pub struct MeResponse {
    pub username: String,
    pub role: String,
    pub panel_name: String,
    pub created_at: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

fn session_payload(
    token: String,
    csrf: String,
    expires_at: String,
    admin: &crate::db::AdminRow,
    panel_name: String,
) -> SessionResponse {
    SessionResponse {
        token,
        csrf_token: csrf,
        username: admin.username.clone(),
        panel_name,
        role: admin.role.clone(),
        permissions: crate::auth::permissions(&admin.role)
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        expires_at,
    }
}

fn panel_name_of(conn: &rusqlite::Connection) -> String {
    crate::db::panel(conn)
        .map(|p| p.panel_name)
        .unwrap_or_else(|_| "Cocktail Manager".into())
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

    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
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
    let (token, csrf) = match crate::auth::create_session(&conn, &admin) {
        Ok(t) => t,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    let expires_at =
        (chrono::Utc::now() + chrono::Duration::hours(crate::auth::SESSION_TTL_HOURS)).to_rfc3339();
    let panel_name = panel_name_of(&conn);
    crate::util::audit(
        "auth.setup",
        None,
        serde_json::json!({ "username": admin.username }),
        "setup",
    );
    Json(session_payload(token, csrf, expires_at, &admin, panel_name)).into_response()
}

pub async fn login(
    State(state): State<SharedState>,
    connect_info: axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let username = body.username.trim().to_string();
    // 直接使用 TCP 对端 IP 作为限流键，不再信任 X-Forwarded-For，
    // 否则攻击者可以通过伪造该头绕过登录限速。
    // 如需在反代后保留真实客户端 IP，请在反代层做 SNAT 而非依赖应用层头。
    let peer = connect_info.0.ip().to_string();
    let rate_key = format!("{peer}|{}", username.to_ascii_lowercase());

    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if crate::auth::setup_required(&conn).unwrap_or(true) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "需要先完成最高管理员初始化".into(),
            }),
        )
            .into_response();
    }
    if !crate::auth::login_allowed(&rate_key)
        || crate::db::login_locked(&conn, &username).unwrap_or(false)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorBody {
                error: "登录失败次数过多，请稍后再试".into(),
            }),
        )
            .into_response();
    }
    let found = crate::db::admin_by_username(&conn, &username)
        .ok()
        .flatten();
    let ok = found
        .as_ref()
        .map(|a| crate::auth::verify_password(&body.password, &a.password_hash))
        .unwrap_or(false);
    if !ok {
        crate::auth::login_failed(&rate_key);
        let _ = crate::db::record_login_failure(
            &conn,
            &username,
            crate::auth::LOGIN_WINDOW.as_secs() as i64,
            crate::auth::LOGIN_LOCK.as_secs() as i64,
        );
        crate::util::audit(
            "auth.login_failed",
            None,
            serde_json::json!({ "username": username, "peer": peer }),
            "login",
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "用户名或密码不正确".into(),
            }),
        )
            .into_response();
    }
    let admin = found.expect("checked");
    let totp_enabled = crate::db::admin_2fa_enabled(&conn, admin.id).unwrap_or(false);
    let totp_secret = crate::db::admin_totp_secret(&conn, admin.id)
        .ok()
        .flatten()
        .unwrap_or_default();
    if totp_enabled && !totp_secret.is_empty() {
        match body.totp_code {
            Some(code) if crate::totp::verify_code(&totp_secret, code) => {}
            Some(_) => {
                crate::auth::login_failed(&rate_key);
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorBody {
                        error: "2FA 验证码不正确".into(),
                    }),
                )
                    .into_response();
            }
            None => {
                crate::auth::login_failed(&rate_key);
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorBody {
                        error: "需要 2FA 验证码".into(),
                    }),
                )
                    .into_response();
            }
        }
    }
    let _ = crate::db::clear_login_failures(&conn, &admin.username);
    crate::auth::login_succeeded(&rate_key);
    let (token, csrf) = match crate::auth::create_session(&conn, &admin) {
        Ok(t) => t,
        Err(e) => return bad_request(e.to_string()).into_response(),
    };
    let expires_at =
        (chrono::Utc::now() + chrono::Duration::hours(crate::auth::SESSION_TTL_HOURS)).to_rfc3339();
    let panel_name = panel_name_of(&conn);
    crate::util::audit(
        "auth.login",
        None,
        serde_json::json!({ "username": admin.username, "role": admin.role }),
        &admin.username,
    );
    Json(session_payload(token, csrf, expires_at, &admin, panel_name)).into_response()
}

pub async fn logout(State(state): State<SharedState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = bearer_from_headers(&headers) {
        let Ok(conn) = db_conn(&state) else {
            return StatusCode::SERVICE_UNAVAILABLE;
        };
        let _ = crate::db::delete_session(&conn, &token);
    }
    StatusCode::NO_CONTENT
}

pub async fn me(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let admin = match current_admin(&state, &headers).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };
    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let panel_name = panel_name_of(&conn);
    Json(MeResponse {
        username: admin.username.clone(),
        role: admin.role.clone(),
        panel_name,
        created_at: admin.created_at,
        permissions: crate::auth::permissions(&admin.role)
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
    })
    .into_response()
}

pub async fn change_password(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> Response {
    if let Err(e) = crate::auth::validate_password(&body.new_password) {
        return bad_request(e.to_string()).into_response();
    }
    let admin = match current_admin(&state, &headers).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };
    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
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
    let _ = crate::db::delete_sessions_for_admin(&conn, admin.id);
    if let Ok((token, csrf)) = crate::auth::create_session(&conn, &admin) {
        let expires_at = (chrono::Utc::now()
            + chrono::Duration::hours(crate::auth::SESSION_TTL_HOURS))
        .to_rfc3339();
        let panel_name = panel_name_of(&conn);
        crate::util::audit(
            "auth.password",
            None,
            serde_json::json!({ "username": admin.username }),
            &admin.username,
        );
        return Json(session_payload(token, csrf, expires_at, &admin, panel_name)).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct TotpSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
}

pub async fn totp_setup(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let token = bearer_from_headers(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let conn = db_conn(&state)?;
    let admin = if state
        .env_api_token
        .as_ref()
        .is_some_and(|t| crate::crypto::ct_eq(t.as_bytes(), token.as_bytes()))
    {
        crate::db::superadmin(&conn).ok().flatten()
    } else {
        crate::db::session_admin(&conn, &token).ok().flatten()
    };
    let admin = admin.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let secret = crate::totp::generate_secret().map_err(|e| bad_request(e.to_string()))?;
    let url = crate::totp::otpauth_url(&secret, &admin.username, "Cocktail Manager");
    crate::db::set_admin_totp_pending(&conn, admin.id, &secret)
        .map_err(|e| bad_request(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "secret": secret, "otpauth_url": url }),
    ))
}

#[derive(Deserialize)]
pub struct TotpVerifyBody {
    pub code: u32,
}

pub async fn totp_verify(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<TotpVerifyBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let token = bearer_from_headers(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let conn = db_conn(&state)?;
    let admin = if state
        .env_api_token
        .as_ref()
        .is_some_and(|t| crate::crypto::ct_eq(t.as_bytes(), token.as_bytes()))
    {
        crate::db::superadmin(&conn).ok().flatten()
    } else {
        crate::db::session_admin(&conn, &token).ok().flatten()
    };
    let admin = admin.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let secret = crate::db::admin_totp_secret(&conn, admin.id)
        .map_err(|e| bad_request(e.to_string()))?
        .unwrap_or_default();
    if secret.is_empty() {
        return Err(bad_request("2FA not set up"));
    }
    if !crate::totp::verify_code(&secret, body.code) {
        return Err(bad_request("invalid 2FA code"));
    }
    crate::db::enable_admin_totp(&conn, admin.id).map_err(|e| bad_request(e.to_string()))?;
    util::audit(
        "auth.2fa_verify",
        None,
        serde_json::json!({ "username": admin.username }),
        "api",
    );
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn totp_status(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let token = bearer_from_headers(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let conn = db_conn(&state)?;
    let admin = if state
        .env_api_token
        .as_ref()
        .is_some_and(|t| crate::crypto::ct_eq(t.as_bytes(), token.as_bytes()))
    {
        crate::db::superadmin(&conn).ok().flatten()
    } else {
        crate::db::session_admin(&conn, &token).ok().flatten()
    };
    let admin = admin.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let enabled =
        crate::db::admin_2fa_enabled(&conn, admin.id).map_err(|e| bad_request(e.to_string()))?;
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

pub async fn totp_disable(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let token = bearer_from_headers(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let conn = db_conn(&state)?;
    let admin = if state
        .env_api_token
        .as_ref()
        .is_some_and(|t| crate::crypto::ct_eq(t.as_bytes(), token.as_bytes()))
    {
        crate::db::superadmin(&conn).ok().flatten()
    } else {
        crate::db::session_admin(&conn, &token).ok().flatten()
    };
    let admin = admin.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    crate::db::set_admin_totp(&conn, admin.id, None).map_err(|e| bad_request(e.to_string()))?;
    util::audit(
        "auth.2fa_disable",
        None,
        serde_json::json!({ "username": admin.username }),
        "api",
    );
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::super::testutil::request;
    use crate::state::test_state;
    use axum::http::StatusCode;

    async fn setup_state() -> (tempfile::TempDir, crate::state::SharedState) {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        (tmp, state)
    }

    #[tokio::test]
    async fn setup_then_login_me_logout() {
        let (tmp, state) = setup_state().await;
        let _ = &tmp;
        // setup creates super-admin and returns a session
        let (s, v) = request(
            &state,
            "POST",
            "/api/v1/setup",
            None,
            Some(serde_json::json!({
                "username": "rootadmin",
                "password": "S3cretPass!123",
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let token = v["token"].as_str().unwrap().to_string();
        assert_eq!(v["role"], "superadmin");
        assert_eq!(v["username"], "rootadmin");

        // me with the token
        let (s, v) = request(&state, "GET", "/api/v1/auth/me", Some(&token), None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["username"], "rootadmin");

        // logout kills the session
        let (s, _) = request(&state, "POST", "/api/v1/auth/logout", Some(&token), None).await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let (s, _) = request(&state, "GET", "/api/v1/auth/me", Some(&token), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_with_wrong_password_rejected() {
        let (tmp, state) = setup_state().await;
        let _ = &tmp;
        let (s, _) = request(
            &state,
            "POST",
            "/api/v1/setup",
            None,
            Some(serde_json::json!({
                "username": "rootadmin",
                "password": "S3cretPass!123",
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let (s, v) = request(
            &state,
            "POST",
            "/api/v1/auth/login",
            None,
            Some(serde_json::json!({
                "username": "rootadmin",
                "password": "wrong-password",
            })),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(v["error"].as_str().is_some());
    }

    #[tokio::test]
    async fn setup_rejects_short_password() {
        let (tmp, state) = setup_state().await;
        let _ = &tmp;
        let (s, _) = request(
            &state,
            "POST",
            "/api/v1/setup",
            None,
            Some(serde_json::json!({
                "username": "rootadmin",
                "password": "123",
            })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn setup_twice_conflicts() {
        let (tmp, state) = setup_state().await;
        let _ = &tmp;
        let body = serde_json::json!({
            "username": "rootadmin",
            "password": "S3cretPass!123",
        });
        let (s, _) = request(&state, "POST", "/api/v1/setup", None, Some(body.clone())).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _) = request(&state, "POST", "/api/v1/setup", None, Some(body)).await;
        assert_eq!(s, StatusCode::CONFLICT);
    }
}
