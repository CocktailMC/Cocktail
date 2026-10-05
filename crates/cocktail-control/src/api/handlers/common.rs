use axum::Json;
use axum::http::{HeaderMap, StatusCode, header};
use serde::Serialize;

use crate::state::SharedState;

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

pub fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string)
}

pub async fn current_admin(
    state: &SharedState,
    headers: &HeaderMap,
) -> Result<crate::db::AdminRow, (StatusCode, Json<ErrorBody>)> {
    let token = bearer_from_headers(headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "未登录".into(),
            }),
        )
    })?;
    let conn = state.db.get().expect("db pool");
    if state.env_api_token.as_ref().is_some_and(|t| t == &token) {
        return crate::db::superadmin(&conn).ok().flatten().ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "未登录".into(),
                }),
            )
        });
    }
    let session = crate::db::session_lookup(&conn, &token).ok().flatten();
    drop(conn);
    let Some(session) = session else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "登录已过期，请重新登录".into(),
            }),
        ));
    };
    Ok(session.admin)
}

pub fn map_result<T: Serialize>(
    result: anyhow::Result<T>,
) -> Result<Json<T>, (StatusCode, Json<ErrorBody>)> {
    match result {
        Ok(v) => Ok(Json(v)),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub fn not_found(msg: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::NOT_FOUND, Json(ErrorBody { error: msg.into() }))
}

pub fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody { error: msg.into() }),
    )
}
