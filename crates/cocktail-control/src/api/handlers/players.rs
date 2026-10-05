use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::api::handlers::{ErrorBody, bad_request, map_result, not_found};
use crate::instance::{self, InstanceStatus, PlayerActionRequest, PlayerInfo};
use crate::state::SharedState;
use crate::util;

#[derive(Deserialize)]
pub struct PlayersQuery {
    #[serde(default)]
    pub probe: bool,
}

pub async fn list_players(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PlayersQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    if q.probe {
        map_result(instance::probe_players(&state, &id).await)
    } else {
        map_result(instance::list_players(&state, &id).await)
    }
}

pub async fn player_action(
    State(state): State<SharedState>,
    Path((id, name, action)): Path<(String, String, String)>,
    body: Option<Json<PlayerActionRequest>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let reason = body.and_then(|Json(b)| b.reason);
    match instance::player_action(&state, &id, &name, &action, reason).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

#[derive(Serialize)]
pub struct PlayerDetailResponse {
    pub info: PlayerInfo,
    pub rcon_capabilities: Vec<String>,
    pub whitelist: Vec<String>,
    pub ops: Vec<String>,
    pub banned_players: Vec<String>,
    pub banned_ips: Vec<String>,
}

pub async fn player_detail(
    State(state): State<SharedState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let view = instance::get_instance(&state, &id)
        .await
        .ok_or_else(|| not_found("instance not found"))?;
    let workdir = view.spec.workdir.clone();
    let player_name = name.clone();
    let detail = tokio::task::spawn_blocking(move || -> anyhow::Result<PlayerDetailResponse> {
        let conn = state.db.get().expect("db pool");
        let rows = crate::db::list_players(&conn, &id)?;
        drop(conn);
        let info = rows
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(&player_name))
            .map(|r| PlayerInfo {
                name: r.name,
                uuid: r.uuid,
                online: false,
                ping_ms: r.last_ping_ms,
                world: r.last_world,
                session_secs: 0,
                total_secs: r.total_secs,
                first_seen: Some(r.first_seen),
                last_seen: Some(r.last_seen),
                ip: r.last_ip,
            })
            .unwrap_or(PlayerInfo {
                name: player_name.clone(),
                uuid: None,
                online: false,
                ping_ms: None,
                world: None,
                session_secs: 0,
                total_secs: 0,
                first_seen: None,
                last_seen: None,
                ip: None,
            });
        let whitelist = crate::instance::players::read_whitelist(&workdir);
        let ops = read_ops_list(&workdir);
        let banned_players = read_banned_players(&workdir);
        let banned_ips = read_banned_ips(&workdir);
        let mut caps = Vec::new();
        if crate::rcon::extract_rcon_config(&workdir).is_some() {
            caps.extend_from_slice(
                &[
                    "kick", "ban", "pardon", "op", "deop", "gamemode", "give", "teleport",
                    "effect", "kill", "clear",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            );
        }
        Ok(PlayerDetailResponse {
            info,
            rcon_capabilities: caps,
            whitelist,
            ops,
            banned_players,
            banned_ips,
        })
    })
    .await
    .map_err(|e| bad_request(e.to_string()))?
    .map_err(|e| bad_request(e.to_string()))?;
    Ok(Json(detail))
}

fn read_ops_list(workdir: &str) -> Vec<String> {
    let path = std::path::Path::new(workdir).join("ops.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    x.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_banned_players(workdir: &str) -> Vec<String> {
    let path = std::path::Path::new(workdir).join("banned-players.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    x.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_banned_ips(workdir: &str) -> Vec<String> {
    let path = std::path::Path::new(workdir).join("banned-ips.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("ip").and_then(|n| n.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Deserialize)]
pub struct PlayerRconActionBody {
    pub command: String,
}

pub async fn player_rcon_action(
    State(state): State<SharedState>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<PlayerRconActionBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let view = instance::get_instance(&state, &id)
        .await
        .ok_or_else(|| not_found("instance not found"))?;
    if view.status != InstanceStatus::Running {
        return Err(bad_request("instance not running"));
    }
    let workdir = view.spec.workdir.clone();
    let cmd = format!("{} {}", body.command, name);
    let result = tokio::task::spawn_blocking(move || crate::rcon::try_rcon(&workdir, &cmd))
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    util::audit(
        "player.rcon",
        Some(&id),
        serde_json::json!({ "player": name, "command": body.command }),
        "api",
    );
    match result {
        Some(resp) => Ok(Json(serde_json::json!({ "ok": true, "response": resp }))),
        None => Err(bad_request("rcon not enabled for this instance")),
    }
}

pub async fn player_history(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::player_history(&state, &id).await)
}
