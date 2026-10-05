use axum::Json;
use axum::body::Body;
use axum::extract::multipart::Multipart;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::api::handlers::{ErrorBody, bad_request, map_result, not_found};
use crate::instance;
use crate::state::SharedState;

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

pub async fn world_download(
    State(state): State<SharedState>,
    Path((id, world)): Path<(String, String)>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let (filename, path) = instance::world_download(&state, &id, &world)
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(bytes))
        .unwrap())
}

pub async fn world_upload(
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
    match instance::world_upload(&state, &id, &world, &bytes).await {
        Ok(files) => Ok(Json(serde_json::json!({ "ok": true, "files": files }))),
        Err(e) => Err(bad_request(e.to_string())),
    }
}
