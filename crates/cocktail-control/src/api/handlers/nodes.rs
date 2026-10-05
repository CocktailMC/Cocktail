use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::api::handlers::{ErrorBody, bad_request};
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct CreateNodeBody {
    pub name: String,
}

#[derive(Serialize)]
pub struct CreatedNode {
    pub node: crate::db::NodeView,
    pub token: String,
}

pub async fn list_nodes(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    crate::cluster::list_views(&state)
        .await
        .map(Json)
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn create_node(
    State(state): State<SharedState>,
    Json(body): Json<CreateNodeBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    crate::cluster::create_node(&state, &body.name)
        .await
        .map(|(node, token)| (StatusCode::CREATED, Json(CreatedNode { node, token })))
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn delete_node(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match crate::cluster::delete_node(&state, &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(bad_request(e.to_string())),
    }
}
