use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::api::handlers::common::{ErrorBody, db_conn};
use crate::community::{AvatarSource, GeoHint, PlayerProfileSummary};
use crate::db;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct BoardQuery {
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct BoardEntry {
    pub rank: usize,
    pub name: String,
    pub uuid: Option<String>,
    pub total_secs: i64,
    pub hours: f64,
    pub first_seen: String,
    pub last_seen: String,
    pub instances: i64,
    pub avatar: String,
}

#[derive(Serialize)]
pub struct BoardResponse {
    pub entries: Vec<BoardEntry>,
    pub total: usize,
}

pub async fn leaderboard(
    State(state): State<SharedState>,
    Query(q): Query<BoardQuery>,
) -> Result<Json<BoardResponse>, (StatusCode, Json<ErrorBody>)> {
    let conn = db_conn(&state)?;
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let entries = if let Some(inst) = q.instance.as_deref() {
        db::list_profiles(&conn, Some(inst), limit)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: e.to_string(),
                    }),
                )
            })?
            .into_iter()
            .enumerate()
            .map(|(i, p)| BoardEntry {
                rank: i + 1,
                avatar: AvatarSource::minotar().url_for(&p.name, p.uuid.as_deref()),
                name: p.name,
                uuid: p.uuid,
                total_secs: p.total_secs,
                hours: p.total_secs as f64 / 3600.0,
                first_seen: p.first_seen,
                last_seen: p.last_seen,
                instances: 1,
            })
            .collect::<Vec<_>>()
    } else {
        db::aggregate_leaderboard(&conn, limit)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: e.to_string(),
                    }),
                )
            })?
            .into_iter()
            .enumerate()
            .map(|(i, (name, uuid, total, first, last, count))| BoardEntry {
                rank: i + 1,
                avatar: AvatarSource::minotar().url_for(&name, uuid.as_deref()),
                name,
                uuid,
                total_secs: total,
                hours: total as f64 / 3600.0,
                first_seen: first,
                last_seen: last,
                instances: count,
            })
            .collect::<Vec<_>>()
    };
    let total = entries.len();
    Ok(Json(BoardResponse { entries, total }))
}

#[derive(Serialize)]
pub struct ProfileView {
    pub name: String,
    pub uuid: Option<String>,
    pub avatar: String,
    pub total_secs: i64,
    pub hours: f64,
    pub sessions: usize,
    pub instances: Vec<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub ips: Vec<String>,
    pub risk_score: u8,
    pub ban_count: usize,
    pub action_count: usize,
    pub geo: Vec<GeoHint>,
}

pub async fn profile(
    State(state): State<SharedState>,
    Path(name): Path<String>,
) -> Result<Json<ProfileView>, (StatusCode, Json<ErrorBody>)> {
    let conn = db_conn(&state)?;
    let rows = db::find_profile(&conn, &name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    if rows.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("没有该玩家的记录: {name}"),
            }),
        ));
    }
    let mut summary = PlayerProfileSummary::new(&rows[0].name);
    summary.uuid = rows.iter().find_map(|r| r.uuid.clone());
    let mut instances = BTreeSet::new();
    let mut ips = BTreeSet::new();
    let mut earliest: Option<String> = None;
    let mut latest: Option<String> = None;
    for row in &rows {
        instances.insert(row.instance_id.clone());
        summary.total_secs = summary
            .total_secs
            .saturating_add(row.total_secs.max(0) as u64);
        summary.sessions += 1;
        if let Some(ip) = row.last_ip.as_deref() {
            if !ip.is_empty() {
                ips.insert(ip.to_string());
            }
        }
        if earliest
            .as_deref()
            .map(|e| row.first_seen.as_str() < e)
            .unwrap_or(true)
        {
            earliest = Some(row.first_seen.clone());
        }
        if latest
            .as_deref()
            .map(|l| row.last_seen.as_str() > l)
            .unwrap_or(true)
        {
            latest = Some(row.last_seen.clone());
        }
    }
    for ip in &ips {
        summary.ips.insert(ip.clone());
    }
    let bans = db::list_bans(&conn, false).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let ban_count = bans
        .iter()
        .filter(|b| b.target.eq_ignore_ascii_case(&name))
        .count();
    summary.ban_count = ban_count as u32;
    let actions = db::list_actions(&conn, Some(&name), None, 500).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    summary.action_count = actions.len() as u32;
    summary.first_seen = earliest
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    summary.last_seen = latest
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    let geo = ips
        .iter()
        .map(|ip| {
            if GeoHint::is_private(ip) {
                GeoHint {
                    ip: ip.clone(),
                    country: "内网".into(),
                    city: "内网".into(),
                    asn: String::new(),
                }
            } else {
                GeoHint::unknown(ip)
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(ProfileView {
        avatar: AvatarSource::minotar().url_for(&summary.name, summary.uuid.as_deref()),
        name: summary.name.clone(),
        uuid: summary.uuid.clone(),
        total_secs: summary.total_secs as i64,
        hours: summary.hours_played(),
        sessions: summary.sessions as usize,
        instances: instances.into_iter().collect(),
        first_seen: summary.first_seen.map(|d| d.to_rfc3339()),
        last_seen: summary.last_seen.map(|d| d.to_rfc3339()),
        ips: ips.into_iter().collect(),
        risk_score: summary.risk_score(),
        ban_count,
        action_count: actions.len(),
        geo,
    }))
}
