use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use crate::api::handlers::{ErrorBody, bad_request, map_result, not_found};
use crate::instance::{
    self, BulkActionRequest, CommandRequest, CreateInstanceRequest, EulaRequest, InstanceEvent,
    InstanceStatus, PropertiesUpdate, UpdateInstanceRequest,
};
use crate::state::SharedState;
use crate::util;

pub async fn list_instances(State(state): State<SharedState>) -> impl IntoResponse {
    Json(instance::list_instances(&state).await)
}

pub async fn get_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::get_instance(&state, &id)
        .await
        .map(Json)
        .ok_or_else(|| not_found("instance not found"))
}

pub async fn create_instance(
    State(state): State<SharedState>,
    Json(req): Json<CreateInstanceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if req.name.trim().is_empty() {
        return Err(bad_request("name is required"));
    }
    instance::create_instance(&state, req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn update_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateInstanceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::update_instance(&state, &id, req).await)
}

pub async fn accept_eula(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<EulaRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::accept_eula(&state, &id, req).await)
}

pub async fn start_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::start_instance(&state, &id).await)
}

pub async fn stop_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::stop_instance(&state, &id).await)
}

pub async fn restart_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::restart_instance(&state, &id).await)
}

pub async fn delete_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::delete_instance(&state, &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(not_found(e.to_string())),
    }
}

pub async fn send_command(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::send_command(&state, &id, req).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn recent_logs(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if instance::get_instance(&state, &id).await.is_none() {
        return Err(not_found("instance not found"));
    }
    Ok(Json(state.recent_logs(&id).await))
}

pub async fn metric_history(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if instance::get_instance(&state, &id).await.is_none() {
        return Err(not_found("instance not found"));
    }
    Ok(Json(state.recent_metrics(&id).await))
}

pub async fn get_properties(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::get_properties(&state, &id).await)
}

pub async fn set_properties(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<PropertiesUpdate>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_properties(&state, &id, &req.entries).await)
}

pub async fn fleet_summary(State(state): State<SharedState>) -> impl IntoResponse {
    Json(instance::fleet_summary(&state).await)
}

pub async fn clone_instance(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<instance::CloneInstanceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::clone_instance(&state, &id, req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn preflight(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::preflight_report(&state, &id).await)
}

pub async fn version_compare(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::version_compare(&state, &id).await)
}

pub async fn rescan_version(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::rescan_version(&state, &id).await)
}

pub async fn bulk_action(
    State(state): State<SharedState>,
    Json(req): Json<BulkActionRequest>,
) -> impl IntoResponse {
    Json(instance::bulk_action(&state, req).await)
}

pub async fn docker_status() -> impl IntoResponse {
    Json(instance::docker_engine_status().await)
}

pub async fn docker_images() -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::docker_list_images()
        .await
        .map(Json)
        .map_err(|e| bad_request(e.to_string()))
}

#[derive(serde::Deserialize)]
pub struct PullImageBody {
    pub image: String,
}

pub async fn docker_pull(
    Json(body): Json<PullImageBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::docker_pull_image(&body.image)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn list_java() -> impl IntoResponse {
    Json(crate::java::inventory().await)
}

pub async fn install_java(
    Json(req): Json<crate::java::InstallJavaRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let image = crate::java::ImageType::parse(req.image_type.as_deref().unwrap_or("jre"))
        .map_err(|e| bad_request(e.to_string()))?;
    crate::java::install(req.major, image)
        .await
        .map(Json)
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn ensure_java(
    Json(req): Json<crate::java::EnsureJavaRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    crate::java::ensure_api(req)
        .await
        .map(Json)
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn delete_java(
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    crate::java::remove(&id)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| bad_request(e.to_string()))
}

pub async fn events_ws(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_socket(socket, state))
}

pub async fn logs_ws(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_socket(socket, state, id))
}

async fn handle_events_socket(socket: WebSocket, state: SharedState) {
    let mut rx = state.events.subscribe();
    let (mut tx, mut inbound) = socket.split();
    let send_loop = async {
        loop {
            match rx.recv().await {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(payload) => {
                        if tx.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    let recv_loop = async {
        while let Some(Ok(msg)) = inbound.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    };
    tokio::select! {
        _ = send_loop => {},
        _ = recv_loop => {},
    }
}

async fn handle_logs_socket(socket: WebSocket, state: SharedState, id: String) {
    let recent = state.recent_logs(&id).await;
    let mut rx = state.events.subscribe();
    let (mut tx, mut inbound) = socket.split();
    for line in recent {
        if let Ok(payload) = serde_json::to_string(&line) {
            if tx.send(Message::Text(payload.into())).await.is_err() {
                return;
            }
        }
    }
    let send_loop = async {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let InstanceEvent::Log { instance_id, line } = &event else {
                        continue;
                    };
                    if instance_id != &id {
                        continue;
                    }
                    match serde_json::to_string(line) {
                        Ok(payload) => {
                            if tx.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    let recv_loop = async {
        while let Some(Ok(msg)) = inbound.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    };
    tokio::select! {
        _ = send_loop => {},
        _ = recv_loop => {},
    }
}

pub async fn get_instance_spec(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    instance::spec_yaml(&state, &id)
        .await
        .map(|yaml| {
            (
                [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
                yaml,
            )
        })
        .map_err(|e| not_found(e.to_string()))
}

pub async fn apply_instance_spec(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    body: String,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::apply_spec_body(&state, &id, &body).await)
}

#[derive(Deserialize)]
pub struct RconExecBody {
    pub command: String,
}

pub async fn rcon_exec(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RconExecBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let view = instance::get_instance(&state, &id)
        .await
        .ok_or_else(|| not_found("instance not found"))?;
    if view.status != InstanceStatus::Running {
        return Err(bad_request("instance not running"));
    }
    let workdir = view.spec.workdir.clone();
    let cmd = body.command.clone();
    let result = tokio::task::spawn_blocking(move || crate::rcon::try_rcon(&workdir, &cmd))
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    match result {
        Some(resp) => Ok(Json(serde_json::json!({ "ok": true, "response": resp }))),
        None => {
            instance::send_command(
                &state,
                &id,
                instance::CommandRequest {
                    command: body.command,
                },
            )
            .await
            .map_err(|e| bad_request(e.to_string()))?;
            Ok(Json(
                serde_json::json!({ "ok": false, "response": "rcon not enabled, sent via stdin" }),
            ))
        }
    }
}

#[derive(Deserialize)]
pub struct RconStatusBody {
    #[serde(default)]
    pub enable: bool,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

pub async fn rcon_status(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let view = instance::get_instance(&state, &id)
        .await
        .ok_or_else(|| not_found("instance not found"))?;
    let workdir = view.spec.workdir.clone();
    let cfg = tokio::task::spawn_blocking(move || crate::rcon::extract_rcon_config(&workdir))
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "enabled": cfg.is_some(),
        "port": cfg.as_ref().map(|c| c.port),
        "host": cfg.as_ref().map(|c| c.host.clone()),
    })))
}

pub async fn rcon_setup(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RconStatusBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let view = instance::get_instance(&state, &id)
        .await
        .ok_or_else(|| not_found("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        return Err(bad_request(
            "stop the instance before changing rcon settings",
        ));
    }
    let workdir = view.spec.workdir.clone();
    let enable = body.enable;
    let password = body
        .password
        .unwrap_or_else(|| format!("{}", uuid::Uuid::new_v4().simple()));
    let port = body.port.unwrap_or(25575);
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let path = std::path::Path::new(&workdir).join("server.properties");
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
        lines.retain(|l| {
            !l.starts_with("enable-rcon=")
                && !l.starts_with("rcon.port=")
                && !l.starts_with("rcon.password=")
        });
        lines.push(format!(
            "enable-rcon={}",
            if enable { "true" } else { "false" }
        ));
        lines.push(format!("rcon.port={port}"));
        lines.push(format!("rcon.password={password}"));
        let mut out = lines.join("\n");
        out.push('\n');
        std::fs::write(path, out)?;
        Ok(())
    })
    .await
    .map_err(|e| bad_request(e.to_string()))?
    .map_err(|e| bad_request(e.to_string()))?;
    util::audit(
        "rcon.setup",
        Some(&id),
        serde_json::json!({ "enabled": enable, "port": port }),
        "api",
    );
    Ok(Json(
        serde_json::json!({ "ok": true, "enabled": enable, "port": port }),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::testutil::request;
    use crate::state::test_state;
    use axum::http::StatusCode;

    async fn create_test_instance(state: &crate::state::SharedState, name: &str) -> String {
        let (status, v) = request(
            state,
            "POST",
            "/api/v1/instances",
            None,
            Some(serde_json::json!({ "name": name, "port": 25599 })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "create {name}");
        v["id"].as_str().expect("id").to_string()
    }

    #[tokio::test]
    async fn instance_crud_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;

        // empty list
        let (s, v) = request(&state, "GET", "/api/v1/instances", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 0);

        // empty name rejected
        let (s, _) = request(
            &state,
            "POST",
            "/api/v1/instances",
            None,
            Some(serde_json::json!({ "name": "" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        // create → get → delete → gone
        let id = create_test_instance(&state, "TestSrv").await;
        let (s, v) = request(
            &state,
            "GET",
            &format!("/api/v1/instances/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["spec"]["name"], "TestSrv");

        let (s, _) = request(
            &state,
            "DELETE",
            &format!("/api/v1/instances/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let (s, _) = request(
            &state,
            "GET",
            &format!("/api/v1/instances/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn missing_instance_error_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let missing = "no-such-instance";

        let (s, _) = request(
            &state,
            "GET",
            &format!("/api/v1/instances/{missing}"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        let (s, _) = request(
            &state,
            "GET",
            &format!("/api/v1/instances/{missing}/logs"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        let (s, _) = request(
            &state,
            "POST",
            &format!("/api/v1/instances/{missing}/start"),
            None,
            None,
        )
        .await;
        // start on a missing instance must not spawn anything; error path only
        assert_ne!(s, StatusCode::OK);
    }
}
