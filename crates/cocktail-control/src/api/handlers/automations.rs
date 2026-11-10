use crate::api::handlers::{ErrorBody, bad_request, db_conn};
use crate::state::SharedState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AutoListQuery {
    pub instance_id: Option<String>,
}

pub async fn list_panel_events(State(state): State<SharedState>) -> impl IntoResponse {
    Json(crate::automations::list_events(&state).await)
}

pub async fn list_automations(
    State(state): State<SharedState>,
    Query(q): Query<AutoListQuery>,
) -> Response {
    let conn = match db_conn(&state) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let rows = crate::db::list_automations(&conn, q.instance_id.as_deref()).unwrap_or_default();
    Json(rows).into_response()
}

pub async fn create_automation(
    State(state): State<SharedState>,
    Json(body): Json<crate::automations::CreateAutomation>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    crate::automations::create(&state, body)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn delete_automation(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let conn = db_conn(&state)?;
    crate::db::delete_automation(&conn, &id).map_err(|e| bad_request(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::super::testutil::request;
    use crate::state::test_state;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn automation_crud_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;

        // empty list
        let (s, v) = request(&state, "GET", "/api/v1/automations", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 0);

        // invalid condition rejected
        let (s, _) = request(
            &state,
            "POST",
            "/api/v1/automations",
            None,
            Some(serde_json::json!({
                "name": "bad",
                "condition": "no_such_cond",
                "actions": ["restart"],
            })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        // create valid
        let (s, v) = request(
            &state,
            "POST",
            "/api/v1/automations",
            None,
            Some(serde_json::json!({
                "name": "low-tps-restart",
                "condition": "tps_below",
                "threshold": 10.0,
                "actions": ["restart"],
            })),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED);
        let id = v["id"].as_str().unwrap().to_string();
        assert_eq!(v["name"], "low-tps-restart");

        // list has it
        let (s, v) = request(&state, "GET", "/api/v1/automations", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);

        // delete
        let (s, _) = request(
            &state,
            "DELETE",
            &format!("/api/v1/automations/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let (s, v) = request(&state, "GET", "/api/v1/automations", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 0);
    }
}
