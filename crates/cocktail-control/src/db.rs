




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
            created_at TEXT NOT NULL,
            totp_secret TEXT,
            totp_enabled INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            admin_id INTEGER NOT NULL REFERENCES admins(id) ON DELETE CASCADE,
            created_at TEXT NOT NULL,
            expires_at TEXT,
            csrf_token TEXT
        );

        CREATE TABLE IF NOT EXISTS login_attempts (
            username TEXT PRIMARY KEY COLLATE NOCASE,
            fails INTEGER NOT NULL DEFAULT 0,
            first_at TEXT NOT NULL,
            locked_until TEXT
        );

        CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            token_hash TEXT,
            hostname TEXT,
            os TEXT,
            arch TEXT,
            last_seen TEXT,
            created_at TEXT NOT NULL,
            protocol_version INTEGER NOT NULL DEFAULT 1,
            last_seq INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS instances (
            id TEXT PRIMARY KEY,
            node_id TEXT NOT NULL,
            payload TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS netops_rules (
            id TEXT PRIMARY KEY,
            cidr TEXT NOT NULL,
            verdict TEXT NOT NULL,
            proto TEXT NOT NULL,
            port INTEGER,
            instance_id TEXT,
            ttl_secs INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            comment TEXT,
            game_ban INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            applied INTEGER NOT NULL DEFAULT 0,
            apply_error TEXT
        );

        CREATE TABLE IF NOT EXISTS player_profiles (
            instance_id TEXT NOT NULL,
            name TEXT NOT NULL COLLATE NOCASE,
            uuid TEXT,
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            last_left TEXT,
            total_secs INTEGER NOT NULL DEFAULT 0,
            last_world TEXT,
            last_ping_ms REAL,
            last_ip TEXT,
            PRIMARY KEY (instance_id, name)
        );

        CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at TEXT NOT NULL,
            action TEXT NOT NULL,
            instance_id TEXT,
            actor TEXT NOT NULL,
            detail TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_audit_at ON audit_log(at DESC);

        CREATE TABLE IF NOT EXISTS automations (
            id TEXT PRIMARY KEY,
            instance_id TEXT,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            condition TEXT NOT NULL,
            threshold REAL NOT NULL DEFAULT 0,
            duration_secs INTEGER NOT NULL DEFAULT 0,
            actions TEXT NOT NULL,
            last_fired TEXT,
            created_at TEXT NOT NULL
        );
        "#,
    )?;
    for (name, decl) in [
        ("qq_app_id", "TEXT"),
        ("qq_app_secret", "TEXT"),
        ("qq_group_openid", "TEXT"),
        ("qq_user_openid", "TEXT"),
        ("qq_sandbox", "INTEGER NOT NULL DEFAULT 0"),
        ("qq_alerts", "INTEGER NOT NULL DEFAULT 1"),
        ("qq_status_secs", "INTEGER NOT NULL DEFAULT 0"),
        ("net_alert_rx_bps", "REAL NOT NULL DEFAULT 0"),
    ] {
        ensure_column(conn, "panel_settings", name, decl)?;
    }
    for (name, decl) in [
        ("expires_at", "TEXT"),
        ("csrf_token", "TEXT"),
    ] {
        ensure_column(conn, "sessions", name, decl)?;
    }
    for (name, decl) in [
        ("totp_secret", "TEXT"),
        ("totp_enabled", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        ensure_column(conn, "admins", name, decl)?;
    }
    for (name, decl) in [
        ("protocol_version", "INTEGER NOT NULL DEFAULT 1"),
        ("last_seq", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        ensure_column(conn, "nodes", name, decl)?;
    }
    let _ = conn.execute("DELETE FROM sessions WHERE expires_at IS NULL", []);
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, name: &str, decl: &str) -> anyhow::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|col| col == name);
    drop(stmt);
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {name} {decl}"),
            [],
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct NodeRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub token_hash: Option<String>,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub last_seen: Option<String>,
    pub created_at: String,
    pub protocol_version: i64,
    pub last_seq: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NodeView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub last_seen: Option<String>,
    pub created_at: String,
    pub online: bool,
    #[serde(default)]
    pub cpu_pct: f32,
    #[serde(default)]
    pub memory_mib: f32,
    #[serde(default)]
    pub rx_bps: f32,
    #[serde(default)]
    pub tx_bps: f32,
    #[serde(default)]
    pub instance_count: usize,
    #[serde(default = "default_proto_v")]
    pub protocol_version: i64,
    #[serde(default)]
    pub last_seq: i64,
    #[serde(default)]
    pub nics: Vec<crate::proto::NicStat>,
    #[serde(default)]
    pub tcp_estab: u32,
    #[serde(default)]
    pub tcp_syn_recv: u32,
    #[serde(default)]
    pub tcp_time_wait: u32,
}

fn default_proto_v() -> i64 {
    1
}

impl NodeRow {
    pub fn into_view(self, online: bool) -> NodeView {
        NodeView {
            id: self.id,
            name: self.name,
            kind: self.kind,
            hostname: self.hostname,
            os: self.os,
            arch: self.arch,
            last_seen: self.last_seen,
            created_at: self.created_at,
            online,
            cpu_pct: 0.0,
            memory_mib: 0.0,
            rx_bps: 0.0,
            tx_bps: 0.0,
            instance_count: 0,
            protocol_version: self.protocol_version,
            last_seq: self.last_seq,
            nics: Vec::new(),
            tcp_estab: 0,
            tcp_syn_recv: 0,
            tcp_time_wait: 0,
        }
    }
}

fn map_node(r: &rusqlite::Row<'_>) -> rusqlite::Result<NodeRow> {
    Ok(NodeRow {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        token_hash: r.get(3)?,
        hostname: r.get(4)?,
        os: r.get(5)?,
        arch: r.get(6)?,
        last_seen: r.get(7)?,
        created_at: r.get(8)?,
        protocol_version: r.get(9)?,
        last_seq: r.get(10)?,
    })
}

pub fn ensure_local_node(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO nodes (id, name, kind, token_hash, hostname, os, arch, last_seen, created_at)
         VALUES ('local', '本机控制面', 'local', NULL, NULL, NULL, NULL, NULL, datetime('now'))",
        [],
    )?;
    Ok(())
}

pub fn list_nodes(conn: &Connection) -> anyhow::Result<Vec<NodeRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, token_hash, hostname, os, arch, last_seen, created_at, protocol_version, last_seq FROM nodes ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], map_node)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_node(conn: &Connection, id: &str) -> anyhow::Result<Option<NodeRow>> {
    conn.query_row(
        "SELECT id, name, kind, token_hash, hostname, os, arch, last_seen, created_at, protocol_version, last_seq FROM nodes WHERE id = ?1",
        params![id],
        map_node,
    )
    .optional()
    .map_err(Into::into)
}

pub fn insert_agent_node(
    conn: &Connection,
    id: &str,
    name: &str,
    token_hash: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO nodes (id, name, kind, token_hash, created_at) VALUES (?1, ?2, 'agent', ?3, datetime('now'))",
        params![id, name, token_hash],
    )?;
    Ok(())
}

pub fn delete_node(conn: &Connection, id: &str) -> anyhow::Result<()> {
    let n = conn.execute("DELETE FROM nodes WHERE id = ?1 AND kind = 'agent'", params![id])?;
    if n == 0 {
        anyhow::bail!("节点不存在");
    }
    Ok(())
}

pub fn touch_node(
    conn: &Connection,
    id: &str,
    hostname: Option<&str>,
    os: Option<&str>,
    arch: Option<&str>,
) -> anyhow::Result<()> {
    if hostname.is_some() || os.is_some() || arch.is_some() {
        conn.execute(
            "UPDATE nodes SET last_seen = datetime('now'),
                hostname = COALESCE(?2, hostname),
                os = COALESCE(?3, os),
                arch = COALESCE(?4, arch)
             WHERE id = ?1",
            params![id, hostname, os, arch],
        )?;
    } else {
        conn.execute(
            "UPDATE nodes SET last_seen = datetime('now') WHERE id = ?1",
            params![id],
        )?;
    }
    Ok(())
}

pub fn replace_instances(
    conn: &Connection,
    instances: &[crate::instance::Instance],
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM instances", [])?;
    {
        let mut stmt =
            tx.prepare("INSERT INTO instances (id, node_id, payload) VALUES (?1, ?2, ?3)")?;
        for inst in instances {
            let payload = serde_json::to_string(inst)?;
            let node_id = if inst.spec.node_id.is_empty() {
                "local"
            } else {
                inst.spec.node_id.as_str()
            };
            stmt.execute(params![inst.id, node_id, payload])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn load_instances(conn: &Connection) -> anyhow::Result<Vec<crate::instance::Instance>> {
    let mut stmt = conn.prepare("SELECT payload FROM instances")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let raw = row?;
        let mut inst: crate::instance::Instance = serde_json::from_str(&raw)?;
        inst.process = None;
        inst.last_metrics = None;
        inst.last_players.clear();
        if inst.spec.node_id.is_empty() {
            inst.spec.node_id = "local".into();
        }
        out.push(inst);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct PanelRow {
    pub panel_name: String,
    pub webhook_url: Option<String>,
    pub qq_app_id: String,
    pub qq_app_secret: String,
    pub qq_group_openid: String,
    pub qq_user_openid: String,
    pub qq_sandbox: bool,
    pub qq_alerts: bool,
    pub qq_status_secs: u64,
    pub net_alert_rx_bps: f32,
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
        "SELECT panel_name, webhook_url, qq_app_id, qq_app_secret, qq_group_openid,
                qq_user_openid, qq_sandbox, qq_alerts, qq_status_secs, net_alert_rx_bps
         FROM panel_settings WHERE id = 1",
        [],
        |r| {
            Ok(PanelRow {
                panel_name: r.get(0)?,
                webhook_url: r.get(1)?,
                qq_app_id: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                qq_app_secret: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                qq_group_openid: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                qq_user_openid: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                qq_sandbox: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                qq_alerts: r.get::<_, Option<i64>>(7)?.unwrap_or(1) != 0,
                qq_status_secs: r.get::<_, Option<i64>>(8)?.unwrap_or(0).max(0) as u64,
                net_alert_rx_bps: r.get::<_, Option<f64>>(9)?.unwrap_or(0.0) as f32,
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
    write_panel(conn, &current)?;
    Ok(current)
}

fn write_panel(conn: &Connection, row: &PanelRow) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE panel_settings SET
            panel_name = ?1, webhook_url = ?2,
            qq_app_id = ?3, qq_app_secret = ?4, qq_group_openid = ?5, qq_user_openid = ?6,
            qq_sandbox = ?7, qq_alerts = ?8, qq_status_secs = ?9, net_alert_rx_bps = ?10
         WHERE id = 1",
        params![
            row.panel_name,
            row.webhook_url,
            null_if_empty(&row.qq_app_id),
            null_if_empty(&row.qq_app_secret),
            null_if_empty(&row.qq_group_openid),
            null_if_empty(&row.qq_user_openid),
            row.qq_sandbox as i64,
            row.qq_alerts as i64,
            row.qq_status_secs as i64,
            row.net_alert_rx_bps as f64,
        ],
    )?;
    Ok(())
}

fn null_if_empty(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

#[derive(Default)]
pub struct PanelPatch {
    pub panel_name: Option<String>,
    pub webhook_url: Option<Option<String>>,
    pub qq_app_id: Option<String>,
    pub qq_app_secret: Option<String>,
    pub qq_group_openid: Option<String>,
    pub qq_user_openid: Option<String>,
    pub qq_sandbox: Option<bool>,
    pub qq_alerts: Option<bool>,
    pub qq_status_secs: Option<u64>,
    pub net_alert_rx_bps: Option<f32>,
}

pub fn patch_panel(conn: &Connection, patch: PanelPatch) -> anyhow::Result<PanelRow> {
    let mut current = panel(conn)?;
    if let Some(name) = patch.panel_name.as_deref() {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            current.panel_name = trimmed.to_string();
        }
    }
    if let Some(url) = patch.webhook_url {
        current.webhook_url = url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    if let Some(v) = patch.qq_app_id {
        current.qq_app_id = v.trim().to_string();
    }
    if let Some(v) = patch.qq_app_secret {
        let t = v.trim();
        if !t.is_empty() && t != "********" {
            current.qq_app_secret = t.to_string();
        }
    }
    if let Some(v) = patch.qq_group_openid {
        current.qq_group_openid = v.trim().to_string();
    }
    if let Some(v) = patch.qq_user_openid {
        current.qq_user_openid = v.trim().to_string();
    }
    if let Some(v) = patch.qq_sandbox {
        current.qq_sandbox = v;
    }
    if let Some(v) = patch.qq_alerts {
        current.qq_alerts = v;
    }
    if let Some(v) = patch.qq_status_secs {
        current.qq_status_secs = if v == 0 { 0 } else { v.max(60) };
    }
    if let Some(v) = patch.net_alert_rx_bps {
        current.net_alert_rx_bps = v.max(0.0);
    }
    write_panel(conn, &current)?;
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
        map_admin,
    )
    .map_err(Into::into)
}

pub fn update_admin_role(conn: &Connection, id: i64, role: &str) -> anyhow::Result<AdminRow> {
    let target = admin_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("用户不存在"))?;
    if target.role == "superadmin" && role != "superadmin" {
        let owners: i64 = conn.query_row(
            "SELECT COUNT(*) FROM admins WHERE role = 'superadmin'",
            [],
            |r| r.get(0),
        )?;
        if owners <= 1 {
            anyhow::bail!("至少保留一名 Owner");
        }
    }
    conn.execute(
        "UPDATE admins SET role = ?1 WHERE id = ?2",
        params![role, id],
    )?;
    admin_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("用户不存在"))
}

pub fn insert_session(
    conn: &Connection,
    token: &str,
    admin_id: i64,
    created_at: &str,
    expires_at: &str,
    csrf_token: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO sessions (token, admin_id, created_at, expires_at, csrf_token)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![token, admin_id, created_at, expires_at, csrf_token],
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub admin: AdminRow,
    pub csrf_token: String,
    pub expires_at: String,
}

pub fn session_lookup(conn: &Connection, token: &str) -> anyhow::Result<Option<SessionRow>> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.query_row(
        "SELECT a.id, a.username, a.password_hash, a.role, a.created_at,
                COALESCE(s.csrf_token, ''), COALESCE(s.expires_at, '')
         FROM sessions s JOIN admins a ON a.id = s.admin_id
         WHERE s.token = ?1 AND (s.expires_at IS NULL OR s.expires_at > ?2)",
        params![token, now],
        |r| {
            Ok(SessionRow {
                admin: AdminRow {
                    id: r.get(0)?,
                    username: r.get(1)?,
                    password_hash: r.get(2)?,
                    role: r.get(3)?,
                    created_at: r.get(4)?,
                },
                csrf_token: r.get(5)?,
                expires_at: r.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn session_admin(conn: &Connection, token: &str) -> anyhow::Result<Option<AdminRow>> {
    Ok(session_lookup(conn, token)?.map(|s| s.admin))
}

pub fn delete_session(conn: &Connection, token: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
    Ok(())
}

pub fn delete_sessions_for_admin(conn: &Connection, admin_id: i64) -> anyhow::Result<usize> {
    let n = conn.execute("DELETE FROM sessions WHERE admin_id = ?1", params![admin_id])?;
    Ok(n)
}

pub fn purge_expired_sessions(conn: &Connection) -> anyhow::Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn.execute(
        "DELETE FROM sessions WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        params![now],
    )?;
    Ok(n)
}

pub fn admin_by_username(conn: &Connection, username: &str) -> anyhow::Result<Option<AdminRow>> {
    conn.query_row(
        "SELECT id, username, password_hash, role, created_at FROM admins
         WHERE username = ?1 COLLATE NOCASE",
        params![username.trim()],
        map_admin,
    )
    .optional()
    .map_err(Into::into)
}

pub fn admin_by_id(conn: &Connection, id: i64) -> anyhow::Result<Option<AdminRow>> {
    conn.query_row(
        "SELECT id, username, password_hash, role, created_at FROM admins WHERE id = ?1",
        params![id],
        map_admin,
    )
    .optional()
    .map_err(Into::into)
}

fn map_admin(r: &rusqlite::Row<'_>) -> rusqlite::Result<AdminRow> {
    Ok(AdminRow {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        role: r.get(3)?,
        created_at: r.get(4)?,
    })
}

pub fn login_attempts(conn: &Connection, username: &str) -> anyhow::Result<(u32, Option<String>)> {
    conn.query_row(
        "SELECT fails, locked_until FROM login_attempts WHERE username = ?1 COLLATE NOCASE",
        params![username.trim()],
        |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, Option<String>>(1)?)),
    )
    .optional()
    .map(|v| v.unwrap_or((0, None)))
    .map_err(Into::into)
}

pub fn record_login_failure(
    conn: &Connection,
    username: &str,
    window_secs: i64,
    lock_secs: i64,
) -> anyhow::Result<u32> {
    let name = username.trim();
    let now = chrono::Utc::now();
    let now_s = now.to_rfc3339();
    let (fails, first_at, locked_until) = conn
        .query_row(
            "SELECT fails, first_at, locked_until FROM login_attempts WHERE username = ?1 COLLATE NOCASE",
            params![name],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .unwrap_or((0, now_s.clone(), None));
    let stale = chrono::DateTime::parse_from_rfc3339(&first_at)
        .map(|t| (now - t.with_timezone(&chrono::Utc)).num_seconds() >= window_secs)
        .unwrap_or(true);
    let base = if stale { 0 } else { fails };
    let first = if stale { now_s.clone() } else { first_at };
    let next = base + 1;
    let lock = if next >= crate::auth::LOGIN_MAX_FAILS as i64 {
        Some((now + chrono::Duration::seconds(lock_secs)).to_rfc3339())
    } else {
        locked_until
    };
    conn.execute(
        "INSERT INTO login_attempts (username, fails, first_at, locked_until)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(username) DO UPDATE SET
            fails = excluded.fails,
            first_at = excluded.first_at,
            locked_until = excluded.locked_until",
        params![name, next, first, lock],
    )?;
    Ok(next as u32)
}

pub fn clear_login_failures(conn: &Connection, username: &str) -> anyhow::Result<()> {
    conn.execute(
        "DELETE FROM login_attempts WHERE username = ?1 COLLATE NOCASE",
        params![username.trim()],
    )?;
    Ok(())
}

pub fn login_locked(conn: &Connection, username: &str) -> anyhow::Result<bool> {
    let (_, locked_until) = login_attempts(conn, username)?;
    let Some(until) = locked_until else {
        return Ok(false);
    };
    Ok(chrono::DateTime::parse_from_rfc3339(&until)
        .map(|t| t.with_timezone(&chrono::Utc) > chrono::Utc::now())
        .unwrap_or(false))
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NetopsRule {
    pub id: String,
    pub cidr: String,
    pub verdict: String,
    pub proto: String,
    pub port: Option<u16>,
    pub instance_id: Option<String>,
    pub ttl_secs: u64,
    pub expires_at: Option<String>,
    pub comment: Option<String>,
    pub game_ban: bool,
    pub created_at: String,
    pub applied: bool,
    pub apply_error: Option<String>,
}

pub fn list_netops(conn: &Connection) -> anyhow::Result<Vec<NetopsRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, cidr, verdict, proto, port, instance_id, ttl_secs, expires_at,
                comment, game_ban, created_at, applied, apply_error
         FROM netops_rules ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(NetopsRule {
            id: r.get(0)?,
            cidr: r.get(1)?,
            verdict: r.get(2)?,
            proto: r.get(3)?,
            port: r.get::<_, Option<i64>>(4)?.map(|p| p as u16),
            instance_id: r.get(5)?,
            ttl_secs: r.get::<_, i64>(6)? as u64,
            expires_at: r.get(7)?,
            comment: r.get(8)?,
            game_ban: r.get::<_, i64>(9)? != 0,
            created_at: r.get(10)?,
            applied: r.get::<_, i64>(11)? != 0,
            apply_error: r.get(12)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn insert_netops(conn: &Connection, rule: &NetopsRule) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO netops_rules (
            id, cidr, verdict, proto, port, instance_id, ttl_secs, expires_at,
            comment, game_ban, created_at, applied, apply_error
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            rule.id,
            rule.cidr,
            rule.verdict,
            rule.proto,
            rule.port.map(|p| p as i64),
            rule.instance_id,
            rule.ttl_secs as i64,
            rule.expires_at,
            rule.comment,
            rule.game_ban as i64,
            rule.created_at,
            rule.applied as i64,
            rule.apply_error,
        ],
    )?;
    Ok(())
}

pub fn delete_netops(conn: &Connection, id: &str) -> anyhow::Result<Option<NetopsRule>> {
    let found = list_netops(conn)?.into_iter().find(|r| r.id == id);
    conn.execute("DELETE FROM netops_rules WHERE id = ?1", params![id])?;
    Ok(found)
}

pub fn expire_netops(conn: &Connection, now: &str) -> anyhow::Result<usize> {
    let n = conn.execute(
        "DELETE FROM netops_rules WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        params![now],
    )?;
    Ok(n)
}

pub fn mark_netops_applied(
    conn: &Connection,
    applied: bool,
    error: Option<&str>,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE netops_rules SET applied = ?1, apply_error = ?2",
        params![applied as i64, error],
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct PlayerRow {
    pub name: String,
    pub uuid: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub total_secs: u64,
    pub last_world: Option<String>,
    pub last_ping_ms: Option<f32>,
    pub last_ip: Option<String>,
}

pub fn upsert_player(
    conn: &Connection,
    instance_id: &str,
    name: &str,
    uuid: Option<&str>,
    ip: Option<&str>,
    world: Option<&str>,
    ping: Option<f32>,
    now: &str,
    left: bool,
    add_secs: u64,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO player_profiles (
            instance_id, name, uuid, first_seen, last_seen, last_left, total_secs,
            last_world, last_ping_ms, last_ip
         ) VALUES (?1,?2,?3,?4,?4,?5,0,?6,?7,?8)
         ON CONFLICT(instance_id, name) DO UPDATE SET
            uuid = COALESCE(excluded.uuid, player_profiles.uuid),
            last_ip = COALESCE(excluded.last_ip, player_profiles.last_ip),
            last_world = COALESCE(excluded.last_world, player_profiles.last_world),
            last_ping_ms = COALESCE(excluded.last_ping_ms, player_profiles.last_ping_ms),
            last_seen = excluded.last_seen,
            last_left = CASE WHEN ?9 THEN excluded.last_seen ELSE player_profiles.last_left END,
            total_secs = player_profiles.total_secs + ?10",
        params![
            instance_id,
            name,
            uuid,
            now,
            if left { Some(now) } else { None::<&str> },
            world,
            ping,
            ip,
            left as i64,
            add_secs as i64,
        ],
    )?;
    Ok(())
}

pub fn list_players(conn: &Connection, instance_id: &str) -> anyhow::Result<Vec<PlayerRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, uuid, first_seen, last_seen, total_secs, last_world, last_ping_ms, last_ip
         FROM player_profiles WHERE instance_id = ?1 ORDER BY last_seen DESC",
    )?;
    let rows = stmt.query_map(params![instance_id], |r| {
        Ok(PlayerRow {
            name: r.get(0)?,
            uuid: r.get(1)?,
            first_seen: r.get(2)?,
            last_seen: r.get(3)?,
            total_secs: r.get::<_, i64>(4)? as u64,
            last_world: r.get(5)?,
            last_ping_ms: r.get(6)?,
            last_ip: r.get(7)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AutomationRow {
    pub id: String,
    pub instance_id: Option<String>,
    pub name: String,
    pub enabled: bool,
    pub condition: String,
    pub threshold: f32,
    pub duration_secs: u64,
    pub actions: Vec<String>,
    pub last_fired: Option<String>,
    pub created_at: String,
}

pub fn list_automations(
    conn: &Connection,
    instance_id: Option<&str>,
) -> anyhow::Result<Vec<AutomationRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, instance_id, name, enabled, condition, threshold, duration_secs, actions, last_fired, created_at
         FROM automations ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let actions: String = r.get(7)?;
        Ok(AutomationRow {
            id: r.get(0)?,
            instance_id: r.get(1)?,
            name: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            condition: r.get(4)?,
            threshold: r.get::<_, f64>(5)? as f32,
            duration_secs: r.get::<_, i64>(6)? as u64,
            actions: serde_json::from_str(&actions).unwrap_or_default(),
            last_fired: r.get(8)?,
            created_at: r.get(9)?,
        })
    })?;
    let all: Vec<_> = rows.filter_map(Result::ok).collect();
    Ok(match instance_id {
        Some(id) => all
            .into_iter()
            .filter(|a| a.instance_id.as_deref() == Some(id) || a.instance_id.is_none())
            .collect(),
        None => all,
    })
}

pub fn insert_automation(conn: &Connection, row: &AutomationRow) -> anyhow::Result<()> {
    let actions = serde_json::to_string(&row.actions)?;
    conn.execute(
        "INSERT INTO automations (id, instance_id, name, enabled, condition, threshold, duration_secs, actions, last_fired, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            row.id,
            row.instance_id,
            row.name,
            row.enabled as i64,
            row.condition,
            row.threshold as f64,
            row.duration_secs as i64,
            actions,
            row.last_fired,
            row.created_at,
        ],
    )?;
    Ok(())
}

pub fn delete_automation(conn: &Connection, id: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM automations WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn mark_automation_fired(conn: &Connection, id: &str, at: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE automations SET last_fired = ?1 WHERE id = ?2",
        params![at, id],
    )?;
    Ok(())
}

pub fn list_admins(conn: &Connection) -> anyhow::Result<Vec<AdminRow>> {
    let mut stmt =
        conn.prepare("SELECT id, username, password_hash, role, created_at FROM admins ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(AdminRow {
            id: r.get(0)?,
            username: r.get(1)?,
            password_hash: r.get(2)?,
            role: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn insert_admin(
    conn: &Connection,
    username: &str,
    password_hash: &str,
    role: &str,
    created_at: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO admins (username, password_hash, role, created_at) VALUES (?1,?2,?3,?4)",
        params![username, password_hash, role, created_at],
    )?;
    Ok(())
}

pub fn delete_admin(conn: &Connection, id: i64) -> anyhow::Result<()> {
    let target = admin_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("用户不存在"))?;
    if target.role == "superadmin" {
        let owners: i64 = conn.query_row(
            "SELECT COUNT(*) FROM admins WHERE role = 'superadmin'",
            [],
            |r| r.get(0),
        )?;
        if owners <= 1 {
            anyhow::bail!("至少保留一名 Owner");
        }
    }
    let n = conn.execute("DELETE FROM admins WHERE id = ?1", params![id])?;
    if n == 0 {
        anyhow::bail!("用户不存在");
    }
    Ok(())
}


#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AuditRow {
    pub at: String,
    pub action: String,
    pub instance_id: Option<String>,
    pub actor: String,
    pub detail: serde_json::Value,
}

pub fn insert_audit(conn: &Connection, row: &AuditRow) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (at, action, instance_id, actor, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            row.at,
            row.action,
            row.instance_id,
            row.actor,
            serde_json::to_string(&row.detail)?
        ],
    )?;
    Ok(())
}

pub fn query_audit(
    conn: &Connection,
    limit: usize,
    offset: usize,
    action: Option<&str>,
    instance_id: Option<&str>,
    actor: Option<&str>,
    q: Option<&str>,
) -> anyhow::Result<(Vec<AuditRow>, usize)> {
    let mut sql = String::from("SELECT at, action, instance_id, actor, detail FROM audit_log WHERE 1=1");
    let mut args: Vec<String> = Vec::new();
    if let Some(a) = action {
        sql.push_str(" AND (action = ? OR action LIKE ?)");
        args.push(a.to_string());
        args.push(format!("{a}.%"));
    }
    if let Some(i) = instance_id {
        sql.push_str(" AND instance_id = ?");
        args.push(i.to_string());
    }
    if let Some(a) = actor {
        sql.push_str(" AND actor = ? COLLATE NOCASE");
        args.push(a.to_string());
    }
    if let Some(q) = q {
        sql.push_str(" AND (action LIKE ? OR actor LIKE ? OR detail LIKE ?)");
        let like = format!("%{q}%");
        args.push(like.clone());
        args.push(like.clone());
        args.push(like);
    }
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM ({sql})"),
        rusqlite::params_from_iter(args.iter()),
        |r| r.get(0),
    )?;
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    let mut all = args.clone();
    all.push(limit.to_string());
    all.push(offset.to_string());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(all.iter()), |r| {
        let detail: String = r.get(4)?;
        Ok(AuditRow {
            at: r.get(0)?,
            action: r.get(1)?,
            instance_id: r.get(2)?,
            actor: r.get(3)?,
            detail: serde_json::from_str(&detail).unwrap_or(serde_json::Value::Null),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok((out, total as usize))
}

pub fn prune_audit(conn: &Connection, keep: usize) -> anyhow::Result<usize> {
    let n = conn.execute(
        "DELETE FROM audit_log WHERE id NOT IN (SELECT id FROM audit_log ORDER BY id DESC LIMIT ?1)",
        params![keep as i64],
    )?;
    Ok(n)
}


pub fn admin_totp_secret(conn: &Connection, id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<String> = conn
        .query_row(
            "SELECT totp_secret FROM admins WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    Ok(row)
}

pub fn set_admin_totp(conn: &Connection, id: i64, secret: Option<&str>) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE admins SET totp_secret = ?1, totp_enabled = ?2 WHERE id = ?3",
        params![secret, secret.is_some() as i64, id],
    )?;
    Ok(())
}

pub fn admin_2fa_enabled(conn: &Connection, id: i64) -> anyhow::Result<bool> {
    conn.query_row(
        "SELECT totp_enabled FROM admins WHERE id = ?1",
        params![id],
        |r| Ok(r.get::<_, i64>(0)? != 0),
    )
    .optional()
    .map(|v| v.unwrap_or(false))
    .map_err(Into::into)
}

pub fn set_node_seq(conn: &Connection, id: &str, seq: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE nodes SET last_seq = ?1 WHERE id = ?2",
        params![seq, id],
    )?;
    Ok(())
}

pub fn node_last_seq(conn: &Connection, id: &str) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT last_seq FROM nodes WHERE id = ?1",
        params![id],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map(|v| v.unwrap_or(0))
    .map_err(Into::into)
}


pub fn set_node_protocol(conn: &Connection, id: &str, version: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE nodes SET protocol_version = ?1 WHERE id = ?2",
        params![version, id],
    )?;
    Ok(())
}

pub fn node_protocol_version(conn: &Connection, id: &str) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT protocol_version FROM nodes WHERE id = ?1",
        params![id],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map(|v| v.unwrap_or(1))
    .map_err(Into::into)
}
