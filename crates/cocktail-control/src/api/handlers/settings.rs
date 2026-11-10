use crate::api::handlers::{bad_request, current_admin, db_conn};
use crate::state::SharedState;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

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
    pub plugin_host: String,
    pub qq_app_id: String,
    pub qq_app_secret_set: bool,
    pub qq_group_openid: String,
    pub qq_user_openid: String,
    pub qq_sandbox: bool,
    pub qq_alerts: bool,
    pub qq_status_secs: u64,
    pub net_alert_rx_mbps: f32,
    pub qq_ready: bool,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub panel_name: Option<String>,
    pub webhook_url: Option<String>,
    pub username: Option<String>,
    pub qq_app_id: Option<String>,
    pub qq_app_secret: Option<String>,
    pub qq_group_openid: Option<String>,
    pub qq_user_openid: Option<String>,
    pub qq_sandbox: Option<bool>,
    pub qq_alerts: Option<bool>,
    pub qq_status_secs: Option<u64>,
    pub net_alert_rx_mbps: Option<f32>,
}

pub async fn get_settings(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let admin = match current_admin(&state, &headers).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };
    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let Ok(panel) = crate::db::panel(&conn) else {
        return bad_request("无法读取面板设置").into_response();
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
        plugin_host: state.plugin_host.clone(),
        qq_app_id: panel.qq_app_id.clone(),
        qq_app_secret_set: !panel.qq_app_secret.is_empty(),
        qq_group_openid: panel.qq_group_openid.clone(),
        qq_user_openid: panel.qq_user_openid.clone(),
        qq_sandbox: panel.qq_sandbox,
        qq_alerts: panel.qq_alerts,
        qq_status_secs: panel.qq_status_secs,
        net_alert_rx_mbps: if panel.net_alert_rx_bps > 0.0 {
            panel.net_alert_rx_bps / (1024.0 * 1024.0)
        } else {
            80.0
        },
        qq_ready: !panel.qq_app_id.is_empty()
            && !panel.qq_app_secret.is_empty()
            && (!panel.qq_group_openid.is_empty() || !panel.qq_user_openid.is_empty()),
    })
    .into_response()
}

pub async fn update_settings(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<UpdateSettingsRequest>,
) -> Response {
    let admin = match current_admin(&state, &headers).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };
    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let patch = crate::db::PanelPatch {
        panel_name: body.panel_name.clone(),
        webhook_url: body.webhook_url.as_ref().map(|s| Some(s.clone())),
        qq_app_id: body.qq_app_id.clone(),
        qq_app_secret: body.qq_app_secret.clone(),
        qq_group_openid: body.qq_group_openid.clone(),
        qq_user_openid: body.qq_user_openid.clone(),
        qq_sandbox: body.qq_sandbox,
        qq_alerts: body.qq_alerts,
        qq_status_secs: body.qq_status_secs,
        net_alert_rx_bps: body
            .net_alert_rx_mbps
            .map(|m| if m <= 0.0 { 0.0 } else { m * 1024.0 * 1024.0 }),
    };
    if let Err(e) = crate::db::patch_panel(&conn, patch) {
        return bad_request(e.to_string()).into_response();
    }
    if let Some(new_name) = body.username.as_deref() {
        match crate::auth::validate_username(new_name) {
            Ok(name) => {
                if let Err(e) = crate::db::update_admin(&conn, admin.id, Some(&name), None) {
                    return bad_request(e.to_string()).into_response();
                }
            }
            Err(e) => return bad_request(e.to_string()).into_response(),
        }
    }
    crate::util::audit(
        "settings.update",
        None,
        serde_json::json!({ "by": admin.username }),
        &admin.username,
    );
    drop(conn);
    get_settings(State(state), headers).await
}
