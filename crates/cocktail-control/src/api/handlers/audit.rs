use axum::Json;
use axum::extract::Query;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub action: Option<String>,
    pub instance_id: Option<String>,
    pub actor: Option<String>,
    pub q: Option<String>,
}

fn default_audit_limit() -> usize {
    80
}

#[derive(Serialize)]
pub struct AuditListResponse {
    pub items: Vec<crate::util::AuditRecord>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

pub async fn list_audit(Query(q): Query<AuditQuery>) -> Json<AuditListResponse> {
    let limit = q.limit.clamp(1, 200);
    let (items, total) = crate::util::list_audit(
        limit,
        q.offset,
        q.action.as_deref(),
        q.instance_id.as_deref(),
        q.actor.as_deref(),
        q.q.as_deref(),
    );
    Json(AuditListResponse {
        items,
        total,
        limit,
        offset: q.offset,
    })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::api;
    use crate::state::test_state;

    #[tokio::test]
    async fn audit_list_returns_200() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let req = Request::builder()
            .uri("/api/v1/audit?limit=5")
            .body(Body::empty())
            .unwrap();
        let resp = api::router().with_state(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["items"].is_array());
        assert!(v["total"].is_number());
        assert_eq!(v["limit"], 5);
    }
}
