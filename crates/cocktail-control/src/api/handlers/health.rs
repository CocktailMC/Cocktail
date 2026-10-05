use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::platform;
use crate::state::SharedState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub name: &'static str,
    pub version: &'static str,
    pub release: &'static str,
    pub status: &'static str,
    pub auth_required: bool,
    pub setup_required: bool,
    pub panel_name: String,
    pub admin_username: Option<String>,
    pub os: String,
    pub arch: String,
    pub family: String,
    pub hostname: String,
    pub distro_id: String,
    pub distro_name: String,
    pub distro_version: String,
    pub kernel: String,
    pub wsl: bool,
    pub plugin_host: String,
    pub plugin_host_ok: bool,
    pub plugins: usize,
}

pub async fn health(State(state): State<SharedState>) -> Json<HealthResponse> {
    let p = platform::detect();
    let conn = state.db.get().expect("db pool");
    let setup_required = crate::auth::setup_required(&conn).unwrap_or(true);
    let panel_name = crate::db::panel(&conn)
        .map(|r| r.panel_name)
        .unwrap_or_else(|_| "Cocktail Manager".into());
    let admin_username = crate::db::superadmin(&conn)
        .ok()
        .flatten()
        .map(|a| a.username);
    drop(conn);
    let (plugin_host_ok, plugins) = crate::plugin_bridge::health_snapshot(&state).await;
    Json(HealthResponse {
        name: "cocktail-control",
        version: "26Q4.11.DP",
        release: "26Q4",
        status: "ok",
        auth_required: !setup_required || state.env_api_token.is_some(),
        setup_required,
        panel_name,
        admin_username,
        os: p.os,
        arch: p.arch,
        family: p.family,
        hostname: p.hostname,
        distro_id: p.distro_id,
        distro_name: p.distro_name,
        distro_version: p.distro_version,
        kernel: p.kernel,
        wsl: p.wsl,
        plugin_host: state.plugin_host.clone(),
        plugin_host_ok,
        plugins,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{request, setup_and_login};
    use crate::state::test_state;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn health_reports_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let (status, v) = request(&state, "GET", "/api/v1/health", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["name"], "cocktail-control");
        assert_eq!(v["version"], "26Q4.11.DP");
        assert_eq!(v["release"], "26Q4");
        assert_eq!(v["status"], "ok");
        assert!(v["setup_required"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn health_after_setup_requires_auth() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let _token = setup_and_login(&state).await;
        let (_, v) = request(&state, "GET", "/api/v1/health", None, None).await;
        assert_eq!(v["setup_required"], false);
        assert_eq!(v["auth_required"], true);
    }
}
