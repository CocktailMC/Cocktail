use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::api::handlers::common::{ErrorBody, current_admin, db_conn};
use crate::db;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct BanQuery {
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub all: Option<bool>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct BanList {
    pub bans: Vec<db::BanRow>,
    pub active: usize,
}

#[derive(Deserialize)]
pub struct CreateBanBody {
    pub target: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub hours: Option<i64>,
    #[serde(default)]
    pub instances: Option<Vec<String>>,
}

const MAX_TARGET_LEN: usize = 64;
const MAX_REASON_LEN: usize = 512;
const MAX_PLAYER_LEN: usize = 64;
const MAX_SUBJECT_LEN: usize = 200;
const MAX_BODY_LEN: usize = 8000;
const MAX_NOTE_LEN: usize = 2000;

fn check_len(field: &str, value: &str, max: usize) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if value.chars().count() > max {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: format!("{field} 过长，最多 {max} 个字符"),
            }),
        ));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: format!("{field} 包含非法控制字符"),
            }),
        ));
    }
    Ok(())
}

pub async fn list_bans(
    State(state): State<SharedState>,
    Query(q): Query<BanQuery>,
) -> Result<Json<BanList>, (StatusCode, Json<ErrorBody>)> {
    let conn = db_conn(&state)?;
    let include_all = q.all.unwrap_or(false);
    let bans = match q.instance.as_deref() {
        Some(inst) if !include_all => db::active_bans_for(&conn, inst).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
        })?,
        _ => db::list_bans_limited(&conn, include_all, q.limit.unwrap_or(2000)).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
        })?,
    };
    let active = bans.iter().filter(|b| b.active).count();
    Ok(Json(BanList { bans, active }))
}

pub async fn create_ban(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateBanBody>,
) -> Result<Json<db::BanRow>, (StatusCode, Json<ErrorBody>)> {
    let admin = current_admin(&state, &headers).await?;
    let target = body.target.trim();
    if target.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "封禁目标不能为空".into(),
            }),
        ));
    }
    check_len("封禁目标", target, MAX_TARGET_LEN)?;
    check_len("封禁原因", &body.reason, MAX_REASON_LEN)?;
    let kind = body.kind.unwrap_or_else(|| "player".into());
    if !matches!(kind.as_str(), "player" | "ip" | "uuid") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "封禁类型仅支持 player / ip / uuid".into(),
            }),
        ));
    }
    let expires = match body.expires_at.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => {
            let parsed = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorBody {
                        error: "到期时间格式非法，需 RFC3339".into(),
                    }),
                )
            })?;
            let utc = parsed.with_timezone(&chrono::Utc);
            if utc <= chrono::Utc::now() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorBody {
                        error: "到期时间必须晚于当前时间".into(),
                    }),
                ));
            }
            Some(utc.to_rfc3339())
        }
        _ => body
            .hours
            .filter(|h| *h > 0)
            .map(|h| (chrono::Utc::now() + chrono::Duration::hours(h)).to_rfc3339()),
    };
    let instances = body.instances.unwrap_or_default();
    let id = crate::crypto::random_token(16);
    let conn = db_conn(&state)?;
    db::insert_ban(
        &conn,
        &id,
        target,
        &kind,
        &body.reason,
        &admin.username,
        expires.as_deref(),
        &instances,
        "panel",
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let _ = db::record_action(
        &conn,
        &admin.username,
        "",
        "ban",
        target,
        &body.reason,
        "ok",
    );
    crate::util::audit(
        "community.ban",
        None,
        serde_json::json!({ "target": target, "kind": kind, "hours": body.hours }),
        "api",
    );
    let row = db::list_bans(&conn, true)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
        })?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "封禁写入失败".into(),
            }),
        ))?;
    Ok(Json(row))
}

pub async fn lift_ban(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let admin = current_admin(&state, &headers).await?;
    let conn = db_conn(&state)?;
    let ok = db::deactivate_ban(&conn, &id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "封禁记录不存在".into(),
            }),
        ));
    }
    crate::util::audit(
        "community.unban",
        None,
        serde_json::json!({ "id": id, "by": admin.username }),
        "api",
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct TicketQuery {
    #[serde(default)]
    pub open: Option<bool>,
    #[serde(default)]
    pub player: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct TicketList {
    pub tickets: Vec<db::TicketRow>,
    pub open: usize,
    pub total: usize,
}

pub async fn list_tickets(
    State(state): State<SharedState>,
    Query(q): Query<TicketQuery>,
) -> Result<Json<TicketList>, (StatusCode, Json<ErrorBody>)> {
    let conn = db_conn(&state)?;
    let tickets = db::list_tickets_limited(
        &conn,
        q.open.unwrap_or(false),
        q.player.as_deref(),
        q.limit.unwrap_or(500),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let open = tickets
        .iter()
        .filter(|t| matches!(t.state.as_str(), "open" | "claimed"))
        .count();
    let total = tickets.len();
    Ok(Json(TicketList {
        tickets,
        open,
        total,
    }))
}

#[derive(Deserialize)]
pub struct TicketBody {
    pub kind: String,
    pub player: String,
    #[serde(default)]
    pub instance_id: Option<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Deserialize)]
pub struct TicketActionBody {
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn create_ticket(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<TicketBody>,
) -> Result<(StatusCode, Json<db::TicketRow>), (StatusCode, Json<ErrorBody>)> {
    let admin = current_admin(&state, &headers).await?;
    let kind = body.kind.trim().to_lowercase();
    if !matches!(kind.as_str(), "appeal" | "whitelist" | "report" | "support") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "工单类型无效".into(),
            }),
        ));
    }
    let player = body.player.trim();
    if player.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "玩家名不能为空".into(),
            }),
        ));
    }
    if body.subject.trim().is_empty() || body.body.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "主题与内容不能为空".into(),
            }),
        ));
    }
    check_len("玩家名", player, MAX_PLAYER_LEN)?;
    check_len("工单主题", body.subject.trim(), MAX_SUBJECT_LEN)?;
    check_len("工单内容", body.body.trim(), MAX_BODY_LEN)?;
    let id = crate::crypto::random_token(12);
    let conn = db_conn(&state)?;
    db::insert_ticket(
        &conn,
        &id,
        &kind,
        player,
        body.instance_id.as_deref().unwrap_or(""),
        body.subject.trim(),
        body.body.trim(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let _ = db::record_action(
        &conn,
        &admin.username,
        "",
        "ticket.open",
        player,
        &kind,
        "ok",
    );
    let row = db::get_ticket(&conn, &id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
        })?
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "工单写入失败".into(),
            }),
        ))?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn transition(
    state: &SharedState,
    headers: &axum::http::HeaderMap,
    id: &str,
    next: &str,
    note: Option<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let admin = current_admin(state, headers).await?;
    let conn = db_conn(&state)?;
    let Some(existing) = db::get_ticket(&conn, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "工单不存在".into(),
            }),
        ));
    };
    if matches!(existing.state.as_str(), "approved" | "rejected" | "closed") {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: "工单已结束".into(),
            }),
        ));
    }
    let resolution = note.clone().unwrap_or_default();
    db::update_ticket_state(&conn, id, next, Some(&admin.username), &resolution).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    if let Some(text) = note {
        let _ = db::append_ticket_note(&conn, id, &admin.username, &text);
    }
    let _ = db::record_action(
        &conn,
        &admin.username,
        &existing.instance_id,
        &format!("ticket.{next}"),
        &existing.player,
        &existing.subject,
        "ok",
    );
    Ok(StatusCode::NO_CONTENT)
}

pub async fn claim_ticket(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    transition(&state, &headers, &id, "claimed", None).await
}

pub async fn approve_ticket(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TicketActionBody>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    transition(&state, &headers, &id, "approved", body.note).await
}

pub async fn reject_ticket(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TicketActionBody>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    transition(&state, &headers, &id, "rejected", body.note).await
}

#[derive(Deserialize)]
pub struct ActionQuery {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn list_actions(
    State(state): State<SharedState>,
    Query(q): Query<ActionQuery>,
) -> Result<Json<Vec<db::ActionRow>>, (StatusCode, Json<ErrorBody>)> {
    let conn = db_conn(&state)?;
    let rows = db::list_actions(
        &conn,
        q.target.as_deref(),
        q.actor.as_deref(),
        q.limit.unwrap_or(200),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    Ok(Json(rows))
}
