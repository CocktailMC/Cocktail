use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::api::handlers::{ErrorBody, bad_request};
use crate::state::SharedState;

#[derive(Serialize)]
pub struct ExtensionsList {
    pub host: String,
    pub online: bool,
    pub error: Option<String>,
    pub items: Vec<serde_json::Value>,
}

pub async fn list_extensions(State(state): State<SharedState>) -> impl IntoResponse {
    match crate::plugin_bridge::catalog(&state).await {
        Ok(items) => Json(ExtensionsList {
            host: state.plugin_host.clone(),
            online: true,
            error: None,
            items,
        }),
        Err(e) => Json(ExtensionsList {
            host: state.plugin_host.clone(),
            online: false,
            error: Some(e.to_string()),
            items: Vec::new(),
        }),
    }
}

pub async fn reload_extensions(
    State(state): State<SharedState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    crate::plugin_bridge::reload_arc(&state)
        .await
        .map(Json)
        .map_err(|e| bad_request(e.to_string()))
}

#[derive(Deserialize)]
pub struct ExtensionEnabledBody {
    pub enabled: bool,
}

pub async fn set_extension_enabled(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<ExtensionEnabledBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    crate::plugin_bridge::set_enabled(&state, &id, body.enabled)
        .await
        .map(Json)
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn proxy_extension_root(
    State(state): State<SharedState>,
    Path(plugin_id): Path<String>,
    req: axum::http::Request<Body>,
) -> Response {
    proxy_extension_inner(state, plugin_id, String::new(), req).await
}

pub async fn proxy_extension(
    State(state): State<SharedState>,
    Path((plugin_id, rest)): Path<(String, String)>,
    req: axum::http::Request<Body>,
) -> Response {
    proxy_extension_inner(state, plugin_id, rest, req).await
}

async fn proxy_extension_inner(
    state: SharedState,
    plugin_id: String,
    rest: String,
    req: axum::http::Request<Body>,
) -> Response {
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap_or_default();
    crate::plugin_bridge::proxy(&state, &plugin_id, &rest, method, headers, body).await
}
