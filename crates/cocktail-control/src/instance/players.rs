//! Player join/leave, history, whitelist.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use regex::Regex;

use super::model::PlayerInfo;
use crate::db;
use crate::state::AppState;

fn sessions() -> &'static Mutex<HashMap<String, DateTime<Utc>>> {
    static M: OnceLock<Mutex<HashMap<String, DateTime<Utc>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(instance_id: &str, name: &str) -> String {
    format!("{instance_id}\0{}", name.to_ascii_lowercase())
}

pub fn parse_online_players(line: &str) -> Option<Vec<String>> {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("players online") {
        return None;
    }
    let idx = line.rfind(':')?;
    let rest = line[idx + 1..].trim();
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let names: Vec<String> = rest
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(names)
}

pub async fn ingest_line(state: &AppState, instance_id: &str, line: &str) {
    static JOIN: OnceLock<Regex> = OnceLock::new();
    static LEFT: OnceLock<Regex> = OnceLock::new();
    static UUID: OnceLock<Regex> = OnceLock::new();
    static LOGIN: OnceLock<Regex> = OnceLock::new();
    static PING: OnceLock<Regex> = OnceLock::new();

    let join = JOIN.get_or_init(|| Regex::new(r"(?i)(\S+) joined the game").unwrap());
    let left = LEFT.get_or_init(|| Regex::new(r"(?i)(\S+) left the game").unwrap());
    let uuid = UUID.get_or_init(|| {
        Regex::new(r"(?i)UUID of player (\S+) is ([0-9a-fA-F-]{32,36})").unwrap()
    });
    let login = LOGIN.get_or_init(|| {
        Regex::new(r"(?i)(\S+)\[/([0-9a-fA-F.:]+)(?::\d+)?\] logged in").unwrap()
    });
    let ping = PING.get_or_init(|| Regex::new(r"(?i)(\S+)'s ping:\s*([0-9]+)").unwrap());

    let now = Utc::now().to_rfc3339();
    let conn = state.db.lock().await;

    if let Some(c) = uuid.captures(line) {
        let _ = db::upsert_player(
            &conn,
            instance_id,
            &c[1],
            Some(&c[2]),
            None,
            None,
            None,
            &now,
            false,
            0,
        );
    }
    if let Some(c) = login.captures(line) {
        let name = &c[1];
        let ip = &c[2];
        if let Ok(mut g) = sessions().lock() {
            g.insert(key(instance_id, name), Utc::now());
        }
        let _ = db::upsert_player(
            &conn,
            instance_id,
            name,
            None,
            Some(ip),
            None,
            None,
            &now,
            false,
            0,
        );
    }
    if let Some(c) = join.captures(line) {
        let name = &c[1];
        if name.eq_ignore_ascii_case("UUID") {
            return;
        }
        if let Ok(mut g) = sessions().lock() {
            g.insert(key(instance_id, name), Utc::now());
        }
        let _ = db::upsert_player(&conn, instance_id, name, None, None, None, None, &now, false, 0);
    }
    if let Some(c) = left.captures(line) {
        let name = &c[1];
        let extra = if let Ok(mut g) = sessions().lock() {
            g.remove(&key(instance_id, name))
                .map(|t| Utc::now().signed_duration_since(t).num_seconds().max(0) as u64)
                .unwrap_or(0)
        } else {
            0
        };
        let _ = db::upsert_player(
            &conn,
            instance_id,
            name,
            None,
            None,
            None,
            None,
            &now,
            true,
            extra,
        );
    }
    if let Some(c) = ping.captures(line) {
        if let Ok(ms) = c[2].parse::<f32>() {
            let _ = db::upsert_player(
                &conn,
                instance_id,
                &c[1],
                None,
                None,
                None,
                Some(ms),
                &now,
                false,
                0,
            );
        }
    }
}

pub async fn list_enriched(
    state: &AppState,
    instance_id: &str,
    online_names: &[String],
) -> Vec<PlayerInfo> {
    let profiles = {
        let conn = state.db.lock().await;
        db::list_players(&conn, instance_id).unwrap_or_default()
    };
    let mut by_name: HashMap<String, db::PlayerRow> = HashMap::new();
    for p in profiles {
        by_name.insert(p.name.to_ascii_lowercase(), p);
    }
    let sess = sessions().lock().ok();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in online_names {
        seen.insert(name.to_ascii_lowercase());
        let row = by_name.get(&name.to_ascii_lowercase());
        let session_secs = sess
            .as_ref()
            .and_then(|m| m.get(&key(instance_id, name)))
            .map(|t| Utc::now().signed_duration_since(*t).num_seconds().max(0) as u64)
            .unwrap_or(0);
        out.push(PlayerInfo {
            name: name.clone(),
            uuid: row.and_then(|r| r.uuid.clone()),
            online: true,
            ping_ms: row.and_then(|r| r.last_ping_ms),
            world: row.and_then(|r| r.last_world.clone()),
            session_secs,
            total_secs: row.map(|r| r.total_secs).unwrap_or(0) + session_secs,
            first_seen: row.map(|r| r.first_seen.clone()),
            last_seen: row.map(|r| r.last_seen.clone()),
            ip: row.and_then(|r| r.last_ip.clone()),
        });
    }
    out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    out
}

pub async fn history(state: &AppState, instance_id: &str) -> Vec<PlayerInfo> {
    let online: std::collections::HashSet<String> = {
        let g = state.instances.read().await;
        g.get(instance_id)
            .map(|i| {
                i.last_players
                    .iter()
                    .map(|n| n.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default()
    };
    let rows = {
        let conn = state.db.lock().await;
        db::list_players(&conn, instance_id).unwrap_or_default()
    };
    rows.into_iter()
        .map(|r| {
            let on = online.contains(&r.name.to_ascii_lowercase());
            PlayerInfo {
                name: r.name,
                uuid: r.uuid,
                online: on,
                ping_ms: r.last_ping_ms,
                world: r.last_world,
                session_secs: 0,
                total_secs: r.total_secs,
                first_seen: Some(r.first_seen),
                last_seen: Some(r.last_seen),
                ip: r.last_ip,
            }
        })
        .collect()
}

pub fn read_whitelist(workdir: &str) -> Vec<String> {
    let path = Path::new(workdir).join("whitelist.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}
