use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::Connection;
use uuid::Uuid;

use crate::db::{self, AdminRow};

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
    if raw.len() < 8 {
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

pub fn permissions(role: &str) -> Vec<&'static str> {
    match role {
        "observer" => vec!["view"],
        "support" => vec!["view", "start", "stop", "logs", "players", "backups"],
        "developer" => vec!["view", "logs", "files", "plugins"],
        "admin" => vec![
            "view", "start", "stop", "logs", "files", "players", "backups", "settings",
            "automations",
        ],
        _ => vec![
            "view", "start", "stop", "logs", "files", "players", "backups", "settings",
            "automations", "users",
        ],
    }
}

pub fn can(role: &str, perm: &str) -> bool {
    permissions(role).iter().any(|p| *p == perm)
}

pub fn setup_required(conn: &Connection) -> anyhow::Result<bool> {
    Ok(db::superadmin(conn)?.is_none())
}

pub fn create_session(conn: &Connection, admin: &AdminRow) -> anyhow::Result<String> {
    let token = new_session_token();
    let now = chrono::Utc::now().to_rfc3339();
    db::insert_session(conn, &token, admin.id, &now)?;
    Ok(token)
}
