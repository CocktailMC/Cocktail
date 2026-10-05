mod audit;
mod auth;
mod automations;
mod backups;
mod common;
mod extensions;
mod files;
mod health;
mod instances;
mod network;
mod nodes;
mod players;
mod plugin_ops;
mod schedules;
mod settings;
mod users;
mod worlds;

pub use audit::*;
pub use auth::*;
pub use automations::*;
pub use backups::*;
pub use common::*;
pub use extensions::*;
pub use files::*;
pub use health::*;
pub use instances::*;
pub use network::*;
pub use nodes::*;
pub use players::*;
pub use plugin_ops::*;
pub use schedules::*;
pub use settings::*;
pub use users::*;
pub use worlds::*;

#[cfg(test)]
pub(crate) mod testutil {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::api;
    use crate::state::SharedState;

    /// Sends a request through the real API router (without the auth
    /// middleware that `lib.rs` adds) and returns `(status, json_body)`.
    /// `auth` is passed as a `Bearer` token when provided.
    pub async fn request(
        state: &SharedState,
        method: &str,
        uri: &str,
        auth: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(t) = auth {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let req = match body {
            Some(v) => builder
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&v).unwrap()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let resp = api::router()
            .with_state(state.clone())
            .oneshot(req)
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json)
    }

    /// Sets up the super-admin and returns a fresh session token.
    pub async fn setup_and_login(state: &SharedState) -> String {
        let (status, v) = request(
            state,
            "POST",
            "/api/v1/setup",
            None,
            Some(serde_json::json!({
                "username": "rootadmin",
                "password": "S3cretPass!123",
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "setup should succeed");
        v["token"].as_str().expect("token").to_string()
    }
}
