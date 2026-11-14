use crate::db::TryConn;
use crate::state::SharedState;
use axum::Json;
use axum::http::{HeaderMap, StatusCode, header};
use serde::Serialize;

pub type ApiError = (StatusCode, Json<ErrorBody>);

pub type DbConn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

pub fn db_unavailable(detail: &str) -> ApiError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorBody {
            error: format!("数据库暂时不可用: {detail}"),
        }),
    )
}

pub fn db_conn(state: &SharedState) -> Result<DbConn, ApiError> {
    state.db.try_conn().map_err(|e| {
        tracing::warn!(error = %e, "database pool unavailable");
        db_unavailable(&e.detail)
    })
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
    let conn = db_conn(&state)?;
    if state
        .env_api_token
        .as_ref()
        .is_some_and(|t| crate::crypto::ct_eq(t.as_bytes(), token.as_bytes()))
    {
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

fn is_ascii_safe(name: &str) -> bool {
    name.bytes()
        .all(|b| (0x20..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}

fn percent_encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.as_bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b' ');
        if keep {
            if *b == b' ' {
                out.push_str("%20");
            } else {
                out.push(*b as char);
            }
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn attachment_disposition(name: &str) -> String {
    let fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if is_ascii_safe(name) {
        format!(
            "attachment; filename=\"{name}\"; filename*=UTF-8''{}",
            percent_encode(name)
        )
    } else {
        format!(
            "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
            percent_encode(name)
        )
    }
}

pub fn binary_attachment(name: &str, bytes: Vec<u8>) -> Result<axum::response::Response, ApiError> {
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, attachment_disposition(name))
        .body(axum::body::Body::from(bytes))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
        })
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
