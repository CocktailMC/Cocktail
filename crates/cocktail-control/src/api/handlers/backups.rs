use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::api::handlers::{ErrorBody, bad_request, map_result, not_found};
use crate::instance;
use crate::state::SharedState;

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

pub async fn backup_preview(
    State(state): State<SharedState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::backup_preview(&state, &id, &backup_id).await)
}
