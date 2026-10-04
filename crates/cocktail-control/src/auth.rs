use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use rusqlite::Connection;
use uuid::Uuid;

use crate::db::{self, AdminRow};

pub const SESSION_TTL_HOURS: i64 = 12 * 24;
pub const LOGIN_MAX_FAILS: u32 = 5;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(600);
pub const LOGIN_LOCK: Duration = Duration::from_secs(900);
const LOGIN_MAX_KEYS: usize = 4096;

#[derive(Clone, Copy)]
struct FailState {
    count: u32,
    first: Instant,
    locked_until: Option<Instant>,
}

fn fail_table() -> &'static Mutex<HashMap<String, FailState>> {
    static TABLE: OnceLock<Mutex<HashMap<String, FailState>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn login_allowed(key: &str) -> bool {
    let now = Instant::now();
    let mut table = match fail_table().lock() {
        Ok(t) => t,
        Err(_) => return true,
    };
    table.retain(|_, s| {
        now.saturating_duration_since(s.first) < LOGIN_WINDOW
            || s.locked_until.is_some_and(|until| until > now)
    });
    match table.get(key) {
        Some(state) => {
            if let Some(until) = state.locked_until {
                if until > now {
                    return false;
                }
            }
            if now.saturating_duration_since(state.first) >= LOGIN_WINDOW {
                table.remove(key);
                return true;
            }
            state.count < LOGIN_MAX_FAILS
        }
        None => true,
    }
}

pub fn login_failed(key: &str) {
    let now = Instant::now();
    let mut table = match fail_table().lock() {
        Ok(t) => t,
        Err(_) => return,
    };
    if table.len() >= LOGIN_MAX_KEYS {
        table.clear();
    }
    let entry = table.entry(key.to_string()).or_insert(FailState {
        count: 0,
        first: now,
        locked_until: None,
    });
    if now.saturating_duration_since(entry.first) >= LOGIN_WINDOW {
        entry.count = 0;
        entry.first = now;
        entry.locked_until = None;
    }
    entry.count += 1;
    if entry.count >= LOGIN_MAX_FAILS {
        entry.locked_until = Some(now + LOGIN_LOCK);
    }
}

pub fn login_succeeded(key: &str) {
    if let Ok(mut table) = fail_table().lock() {
        table.remove(key);
    }
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash password: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn validate_username(raw: &str) -> anyhow::Result<String> {
    let name = raw.trim();
    if name.len() < 3 || name.len() > 32 {
        anyhow::bail!("用户名长度须为 3–32 个字符");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!("用户名仅允许字母、数字、下划线和短横线");
    }
    Ok(name.to_string())
}

pub fn validate_password(raw: &str) -> anyhow::Result<()> {
    if raw.chars().count() < 8 {
        anyhow::bail!("密码至少 8 位");
    }
    if raw.len() > 128 {
        anyhow::bail!("密码过长");
    }
    Ok(())
}

pub fn new_session_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub fn new_csrf_token() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn permissions(role: &str) -> Vec<&'static str> {
    match role {
        "observer" => vec!["view"],
        "support" => vec![
            "view", "start", "stop", "console", "logs", "players", "players.detail",
            "players.kick", "players.ban", "players.pardon", "players.whitelist",
            "rcon", "backups", "netops.view",
        ],
        "developer" => vec![
            "view", "console", "logs", "files", "files.read", "files.write",
            "plugins", "plugins.install", "start", "stop", "rcon",
        ],
        "admin" => vec![
            "view", "start", "stop", "console", "logs", "files", "files.read",
            "files.write", "plugins", "plugins.install", "players", "players.detail",
            "players.kick", "players.ban", "players.pardon", "players.op",
            "players.deop", "players.whitelist", "players.gamemode", "players.teleport",
            "players.give", "players.effect", "players.kill", "players.clear",
            "rcon", "rcon.setup", "backups", "settings", "automations",
            "netops.view", "netops.write", "nodes",
        ],
        _ => vec![
            "view", "start", "stop", "console", "logs", "files", "files.read",
            "files.write", "plugins", "plugins.install", "players", "players.detail",
            "players.kick", "players.ban", "players.pardon", "players.op",
            "players.deop", "players.whitelist", "players.gamemode", "players.teleport",
            "players.give", "players.effect", "players.kill", "players.clear",
            "rcon", "rcon.setup", "backups", "settings", "automations",
            "netops.view", "netops.write", "nodes", "nodes.manage", "users",
            "2fa.manage",
        ],
    }
}

pub fn can(role: &str, perm: &str) -> bool {
    if role == "superadmin" {
        return true;
    }
    if perm.is_empty() || perm == "view" {
        return true;
    }
    permissions(role).iter().any(|p| *p == perm)
}

pub fn setup_required(conn: &Connection) -> anyhow::Result<bool> {
    Ok(db::superadmin(conn)?.is_none())
}

pub fn create_session(conn: &Connection, admin: &AdminRow) -> anyhow::Result<(String, String)> {
    let token = new_session_token();
    let csrf = new_csrf_token();
    let now = chrono::Utc::now().to_rfc3339();
    let expires = (chrono::Utc::now() + chrono::Duration::hours(SESSION_TTL_HOURS)).to_rfc3339();
    db::insert_session(conn, &token, admin.id, &now, &expires, &csrf)?;
    Ok((token, csrf))
}