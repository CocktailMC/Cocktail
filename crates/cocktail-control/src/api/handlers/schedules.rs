use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::api::handlers::{ErrorBody, bad_request, not_found};
use crate::instance::{self, CreateScheduleRequest};
use crate::state::SharedState;

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
