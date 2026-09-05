//! SQLite persistence for panel settings, super-admin, and sessions.
//!
//! SQLite 4 was never released as a production engine; this uses bundled SQLite 3
//! (the current SQLite) at `data/cocktail.db`.

use std::fs;

use rusqlite::{params, Connection, OptionalExtension};

pub const DB_PATH: &str = "data/cocktail.db";

pub fn open() -> anyhow::Result<Connection> {
    fs::create_dir_all("data")?;
    let conn = Connection::open(DB_PATH)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS panel_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            panel_name TEXT NOT NULL,
            webhook_url TEXT
        );
        INSERT OR IGNORE INTO panel_settings (id, panel_name, webhook_url)
            VALUES (1, 'Cocktail Manager', NULL);

        CREATE TABLE IF NOT EXISTS admins (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'superadmin',
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            admin_id INTEGER NOT NULL REFERENCES admins(id) ON DELETE CASCADE,
            created_at TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct PanelRow {
    pub panel_name: String,
    pub webhook_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AdminRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: String,
}

pub fn panel(conn: &Connection) -> anyhow::Result<PanelRow> {
    conn.query_row(
        "SELECT panel_name, webhook_url FROM panel_settings WHERE id = 1",
        [],
        |r| {
            Ok(PanelRow {
                panel_name: r.get(0)?,
                webhook_url: r.get(1)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn update_panel(
    conn: &Connection,
    panel_name: Option<&str>,
    webhook_url: Option<Option<&str>>,
) -> anyhow::Result<PanelRow> {
    let mut current = panel(conn)?;
    if let Some(name) = panel_name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            current.panel_name = trimmed.to_string();
        }
    }
    if let Some(url) = webhook_url {
        current.webhook_url = url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    conn.execute(
        "UPDATE panel_settings SET panel_name = ?1, webhook_url = ?2 WHERE id = 1",
        params![current.panel_name, current.webhook_url],
    )?;
    Ok(current)
}

pub fn superadmin(conn: &Connection) -> anyhow::Result<Option<AdminRow>> {
    conn.query_row(
        "SELECT id, username, password_hash, role, created_at FROM admins ORDER BY id ASC LIMIT 1",
        [],
        |r| {
            Ok(AdminRow {
                id: r.get(0)?,
                username: r.get(1)?,
                password_hash: r.get(2)?,
                role: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn insert_superadmin(
    conn: &Connection,
    username: &str,
    password_hash: &str,
    created_at: &str,
) -> anyhow::Result<AdminRow> {
    conn.execute(
        "INSERT INTO admins (username, password_hash, role, created_at) VALUES (?1, ?2, 'superadmin', ?3)",
        params![username, password_hash, created_at],
    )?;
    Ok(superadmin(conn)?.expect("just inserted"))
}

pub fn update_admin(
    conn: &Connection,
    id: i64,
    username: Option<&str>,
    password_hash: Option<&str>,
) -> anyhow::Result<AdminRow> {
    if let Some(u) = username {
        conn.execute(
            "UPDATE admins SET username = ?1 WHERE id = ?2",
            params![u, id],
        )?;
    }
    if let Some(h) = password_hash {
        conn.execute(
            "UPDATE admins SET password_hash = ?1 WHERE id = ?2",
            params![h, id],
        )?;
    }
    conn.query_row(
        "SELECT id, username, password_hash, role, created_at FROM admins WHERE id = ?1",
        params![id],
        |r| {
            Ok(AdminRow {
                id: r.get(0)?,
                username: r.get(1)?,
                password_hash: r.get(2)?,
                role: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn insert_session(conn: &Connection, token: &str, admin_id: i64, created_at: &str) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO sessions (token, admin_id, created_at) VALUES (?1, ?2, ?3)",
        params![token, admin_id, created_at],
    )?;
    Ok(())
}

pub fn session_admin(conn: &Connection, token: &str) -> anyhow::Result<Option<AdminRow>> {
    conn.query_row(
        "SELECT a.id, a.username, a.password_hash, a.role, a.created_at
         FROM sessions s JOIN admins a ON a.id = s.admin_id
         WHERE s.token = ?1",
        params![token],
        |r| {
            Ok(AdminRow {
                id: r.get(0)?,
                username: r.get(1)?,
                password_hash: r.get(2)?,
                role: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn delete_session(conn: &Connection, token: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
    Ok(())
}
