use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::api::handlers::{ErrorBody, bad_request, map_result, not_found};
use crate::state::SharedState;

#[derive(Serialize)]
pub struct HostNetworkResponse {
    pub live: Option<crate::hostnet::HostNetSample>,
    pub history: Vec<crate::hostnet::HostNetSample>,
}

pub async fn host_network(State(state): State<SharedState>) -> impl IntoResponse {
    let live = state.ops.latest.read().await.clone();
    let history: Vec<_> = state.ops.history.read().await.iter().cloned().collect();
    Json(HostNetworkResponse { live, history })
}

pub async fn list_netops(State(state): State<SharedState>) -> impl IntoResponse {
    Json(crate::netops::status(&state).await)
}

pub async fn create_netops(
    State(state): State<SharedState>,
    Json(req): Json<crate::netops::CreateNetopsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(crate::netops::create(&state, req).await)
}

pub async fn delete_netops(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match crate::netops::delete(&state, &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn kick_netops(
    State(state): State<SharedState>,
    Json(req): Json<crate::netops::KickRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(
        crate::netops::kick(&state, req)
            .await
            .map(|_| serde_json::json!({ "ok": true })),
    )
}

pub async fn resync_netops(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(crate::netops::resync(&state).await)
}

#[derive(Deserialize)]
pub struct QqTestRequest {
    pub message: Option<String>,
}

pub async fn qq_test(
    State(state): State<SharedState>,
    Json(body): Json<QqTestRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let text = body
        .message
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Cocktail 测试：QQ 机器人接入正常。".into());
    match crate::ops::send_now(&state, &text).await {
        Ok(()) => Ok(Json(serde_json::json!({ "ok": true }))),
        Err(e) => Err(bad_request(e.to_string())),
    }
}
