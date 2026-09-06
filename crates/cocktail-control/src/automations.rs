//! Condition → action rules (TPS, players, crash, CPU).

use std::time::{Duration, Instant};

use chrono::{Timelike, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::AutomationRow;
use crate::instance::{CommandRequest, InstanceStatus};
use crate::state::SharedState;

#[derive(Debug, Clone, Serialize)]
pub struct PanelEvent {
    pub id: String,
    pub at: String,
    pub level: String,
    pub instance_id: Option<String>,
    pub title: String,
    pub detail: String,
}

pub async fn emit(
    state: &SharedState,
    level: &str,
    instance_id: Option<&str>,
    title: &str,
    detail: &str,
) {
    let ev = PanelEvent {
        id: Uuid::new_v4().to_string(),
        at: Utc::now().to_rfc3339(),
        level: level.into(),
        instance_id: instance_id.map(|s| s.to_string()),
        title: title.into(),
        detail: detail.into(),
    };
    let mut g = state.feed.write().await;
    g.push_back(ev);
    while g.len() > 80 {
        g.pop_front();
    }
}

pub async fn list_events(state: &SharedState) -> Vec<PanelEvent> {
    state.feed.read().await.iter().cloned().rev().collect()
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateAutomation {
    pub instance_id: Option<String>,
    pub name: String,
    pub condition: String,
    #[serde(default)]
    pub threshold: f32,
    #[serde(default)]
    pub duration_secs: u64,
    pub actions: Vec<String>,
    #[serde(default = "def_true")]
    pub enabled: bool,
}

fn def_true() -> bool {
    true
}

pub async fn create(state: &SharedState, req: CreateAutomation) -> anyhow::Result<AutomationRow> {
    let cond = req.condition.trim().to_ascii_lowercase();
    if !matches!(
        cond.as_str(),
        "tps_below" | "players_above" | "crashed" | "cpu_above"
    ) {
        anyhow::bail!("未知条件（tps_below / players_above / crashed / cpu_above）");
    }
    if req.actions.is_empty() {
        anyhow::bail!("至少一条动作");
    }
    let row = AutomationRow {
        id: Uuid::new_v4().to_string(),
        instance_id: req.instance_id.filter(|s| !s.is_empty()),
        name: req.name.trim().to_string(),
        enabled: req.enabled,
        condition: cond,
        threshold: req.threshold,
        duration_secs: req.duration_secs.min(3600),
        actions: req.actions,
        last_fired: None,
        created_at: Utc::now().to_rfc3339(),
    };
    if row.name.is_empty() {
        anyhow::bail!("名称不能为空");
    }
    let conn = state.db.lock().await;
    crate::db::insert_automation(&conn, &row)?;
    Ok(row)
}

pub async fn tick(state: &SharedState) {
    let rules = {
        let conn = state.db.lock().await;
        crate::db::list_automations(&conn, None).unwrap_or_default()
    };
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let matched = match rule.condition.as_str() {
            "tps_below" | "players_above" | "cpu_above" => {
                eval_metric(state, &rule).await
            }
            "crashed" => false,
            _ => false,
        };
        let key = rule.id.clone();
        if matched {
            let ready = if rule.duration_secs == 0 {
                true
            } else {
                let dur = Duration::from_secs(rule.duration_secs);
                let mut hold = state.ops.auto_hold.lock().await;
                let e = hold.entry(key.clone()).or_insert_with(Instant::now);
                e.elapsed() >= dur
            };
            if ready {
                fire(state, &rule).await;
                state.ops.auto_hold.lock().await.remove(&key);
            }
        } else {
            state.ops.auto_hold.lock().await.remove(&key);
        }
    }
}

async fn eval_metric(state: &SharedState, rule: &AutomationRow) -> bool {
    let g = state.instances.read().await;
    g.values()
        .filter(|i| {
            rule.instance_id
                .as_ref()
                .map(|id| &i.id == id)
                .unwrap_or(true)
        })
        .filter(|i| i.status == InstanceStatus::Running)
        .any(|i| {
            let m = i.last_metrics.as_ref();
            match rule.condition.as_str() {
                "tps_below" => m.and_then(|x| x.tps).is_some_and(|t| t < rule.threshold),
                "players_above" => m.is_some_and(|x| x.players as f32 > rule.threshold),
                "cpu_above" => m.is_some_and(|x| x.cpu_pct > rule.threshold),
                _ => false,
            }
        })
}

pub async fn on_crash(state: &SharedState, instance_id: &str, name: &str) {
    emit(
        state,
        "warn",
        Some(instance_id),
        &format!("{name} 崩溃"),
        "进程异常退出",
    )
    .await;
    let rules = {
        let conn = state.db.lock().await;
        crate::db::list_automations(&conn, Some(instance_id)).unwrap_or_default()
    };
    for rule in rules {
        if rule.enabled && rule.condition == "crashed" {
            fire(state, &rule).await;
        }
    }
}

async fn fire(state: &SharedState, rule: &AutomationRow) {
    if let Some(prev) = &rule.last_fired {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(prev) {
            if Utc::now()
                .signed_duration_since(t.with_timezone(&Utc))
                .num_seconds()
                < 180
            {
                return;
            }
        }
    }
    let now = Utc::now().to_rfc3339();
    {
        let conn = state.db.lock().await;
        let _ = crate::db::mark_automation_fired(&conn, &rule.id, &now);
    }
    emit(
        state,
        "ok",
        rule.instance_id.as_deref(),
        &format!("自动化「{}」触发", rule.name),
        &rule.actions.join(", "),
    )
    .await;
    for act in &rule.actions {
        let a = act.trim();
        if a == "notify_qq" || a == "qq" {
            crate::ops::notify_event(
                state,
                "自动化",
                &format!("{}: {}", rule.name, rule.condition),
            )
            .await;
        } else if a == "restart" {
            if let Some(id) = &rule.instance_id {
                let _ = crate::instance::restart_instance(state, id).await;
            }
        } else if a == "start" {
            if let Some(id) = &rule.instance_id {
                let _ = crate::instance::start_instance(state, id).await;
            }
        } else if let Some(id) = a.strip_prefix("start:") {
            let _ = crate::instance::start_instance(state, id.trim()).await;
        } else if let Some(cmd) = a.strip_prefix("command:") {
            if let Some(id) = &rule.instance_id {
                let _ = crate::instance::send_command(
                    state,
                    id,
                    CommandRequest {
                        command: cmd.trim().into(),
                    },
                )
                .await;
            }
        }
    }
    crate::util::audit(
        "automation.fire",
        rule.instance_id.as_deref(),
        serde_json::json!({ "name": rule.name, "actions": rule.actions }),
        "system",
    );
}

pub async fn run_backup_hours(state: &SharedState) {
    let hour = chrono::Local::now().hour();
    let today = chrono::Local::now().date_naive().to_string();
    let insts: Vec<(String, u8, u32)> = {
        let g = state.instances.read().await;
        g.values()
            .filter_map(|i| {
                i.spec
                    .backup_hour
                    .map(|h| (i.id.clone(), h, i.spec.backup_keep.max(1)))
            })
            .collect()
    };
    for (id, h, keep) in insts {
        if h != hour as u8 {
            continue;
        }
        let stamp_key = format!("backup-hour:{id}:{today}");
        {
            let mut hold = state.ops.auto_hold.lock().await;
            if hold.contains_key(&stamp_key) {
                continue;
            }
            hold.insert(stamp_key, Instant::now());
        }
        let _ = crate::instance::create_backup(state, &id).await;
        let _ = keep;
    }
}
