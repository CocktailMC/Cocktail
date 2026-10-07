use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use cocktail_shared::runtime_util::{
    ParsedGameStats, inject_jvm_memory, is_java_command, merge_game_stats, minecraft_ready,
    parse_game_stats,
};

pub fn parse_command_line(line: &str) -> anyhow::Result<(String, Vec<String>)> {
    let parts = split_command_line(line);
    let mut iter = parts.into_iter();
    let command = iter
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("启动命令为空"))?;
    Ok((command, iter.collect()))
}

fn split_command_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn java_jar_startup(jar_rel: &str) -> (String, Vec<String>) {
    let jar = jar_rel
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    ("java".into(), vec!["-jar".into(), jar, "nogui".into()])
}

pub fn set_property_file(path: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    let mut lines: Vec<String> = if path.exists() {
        fs::read_to_string(path)?
            .lines()
            .map(|l| l.to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut found = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = trimmed.split_once('=') {
            if k.trim() == key {
                *line = format!("{key}={value}");
                found = true;
                break;
            }
        }
    }
    if !found {
        lines.push(format!("{key}={value}"));
    }
    fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

pub fn read_properties(path: &Path) -> anyhow::Result<Vec<(String, String)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(out)
}

pub fn write_properties(path: &Path, entries: &[(String, String)]) -> anyhow::Result<()> {
    let mut body = String::from("# Managed by Cocktail Manager\n");
    for (k, v) in entries {
        body.push_str(&format!("{k}={v}\n"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body)?;
    Ok(())
}

pub fn write_eula(workdir: &str, accepted: bool) -> anyhow::Result<()> {
    let path = Path::new(workdir).join("eula.txt");
    let val = if accepted { "true" } else { "false" };
    fs::write(
        path,
        format!(
            "# By changing the setting below to TRUE you are indicating your agreement to Mojang EULA.\n\
             # https://aka.ms/MinecraftEULA\n\
             eula={val}\n"
        ),
    )?;
    Ok(())
}

pub fn eula_is_accepted(workdir: &str) -> bool {
    let path = Path::new(workdir).join("eula.txt");
    fs::read_to_string(path)
        .map(|s| {
            s.lines().any(|l| {
                let t = l.trim();
                t.eq_ignore_ascii_case("eula=true")
            })
        })
        .unwrap_or(false)
}

pub fn health_report(
    status: &str,
    tps: Option<f32>,
    mspt: Option<f32>,
    mem_used: f32,
    mem_max: f32,
    net_alerts: usize,
) -> (u8, Vec<String>) {
    let mut score = 100i32;
    let mut reasons = Vec::new();
    if status == "crashed" {
        return (5, vec!["进程崩溃".into()]);
    }
    if status != "running" {
        reasons.push("服务器未运行".into());
        score -= 40;
    } else {
        match tps {
            Some(t) if t >= 18.0 => reasons.push("TPS 正常".into()),
            Some(t) if t >= 15.0 => {
                reasons.push(format!("TPS {t:.1} 偏低"));
                score -= 15;
            }
            Some(t) => {
                reasons.push(format!("TPS {t:.1} 严重偏低"));
                score -= 35;
            }
            None => reasons.push("尚未采到 TPS（需 Paper/Spark 输出或定时 tps）".into()),
        }
        if let Some(m) = mspt {
            if m > 50.0 {
                reasons.push(format!("MSPT {m:.0}ms 过高"));
                score -= 20;
            } else {
                reasons.push("MSPT 正常".into());
            }
        }
        if mem_max > 0.0 && mem_used / mem_max > 0.9 {
            reasons.push("内存偏高".into());
            score -= 12;
        } else if mem_max > 0.0 {
            reasons.push("内存正常".into());
        }
        if net_alerts > 0 {
            reasons.push("网络有告警".into());
            score -= 10;
        } else if status == "running" {
            reasons.push("网络正常".into());
        }
    }
    (score.clamp(0, 100) as u8, reasons)
}

pub fn append_instance_log(instance_id: &str, stream: &str, line: &str) {
    let path = Path::new("data")
        .join("logs")
        .join(format!("{instance_id}.log"));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} [{}] {}", Utc::now().to_rfc3339(), stream, line);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub at: String,
    pub action: String,
    pub instance_id: Option<String>,
    #[serde(default)]
    pub detail: serde_json::Value,
    pub actor: String,
}

const AUDIT_MAX_READ: usize = 8000;
pub const AUDIT_MAX_ROWS: usize = 50_000;

fn audit_conn() -> Option<&'static std::sync::Mutex<rusqlite::Connection>> {
    static CONN: std::sync::OnceLock<Option<std::sync::Mutex<rusqlite::Connection>>> =
        std::sync::OnceLock::new();
    CONN.get_or_init(|| crate::db::open().ok().map(std::sync::Mutex::new))
        .as_ref()
}

pub fn migrate_audit_jsonl() {
    let path = Path::new("data").join("audit.jsonl");
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Some(conn) = audit_conn() else {
        return;
    };
    let Ok(guard) = conn.lock() else {
        return;
    };
    let count: i64 = guard
        .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
        .unwrap_or(0);
    if count > 0 {
        return;
    }
    let mut moved = 0usize;
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(rec) = serde_json::from_str::<AuditRecord>(line) else {
            continue;
        };
        let row = crate::db::AuditRow {
            at: rec.at,
            action: rec.action,
            instance_id: rec.instance_id,
            actor: rec.actor,
            detail: rec.detail,
        };
        if crate::db::insert_audit(&guard, &row).is_ok() {
            moved += 1;
        }
    }
    let _ = crate::db::prune_audit(&guard, AUDIT_MAX_ROWS);
    if moved > 0 {
        let _ = fs::rename(&path, path.with_extension("jsonl.migrated"));
        tracing::info!(rows = moved, "audit.jsonl migrated into sqlite");
    }
}

pub fn list_audit(
    limit: usize,
    offset: usize,
    action: Option<&str>,
    instance_id: Option<&str>,
    actor: Option<&str>,
    q: Option<&str>,
) -> (Vec<AuditRecord>, usize) {
    let limit = limit.clamp(1, 200);
    let action = action.map(str::trim).filter(|s| !s.is_empty());
    let instance_id = instance_id.map(str::trim).filter(|s| !s.is_empty());
    let actor = actor.map(str::trim).filter(|s| !s.is_empty());
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    if let Some(conn) = audit_conn() {
        if let Ok(guard) = conn.lock() {
            if let Ok((rows, total)) =
                crate::db::query_audit(&guard, limit, offset, action, instance_id, actor, q)
            {
                return (
                    rows.into_iter()
                        .map(|r| AuditRecord {
                            at: r.at,
                            action: r.action,
                            instance_id: r.instance_id,
                            detail: r.detail,
                            actor: r.actor,
                        })
                        .collect(),
                    total,
                );
            }
        }
    }
    legacy_list_audit(limit, offset, action, instance_id, actor, q)
}

fn legacy_list_audit(
    limit: usize,
    offset: usize,
    action: Option<&str>,
    instance_id: Option<&str>,
    actor: Option<&str>,
    q: Option<&str>,
) -> (Vec<AuditRecord>, usize) {
    let path = Path::new("data").join("audit.jsonl");
    let Ok(raw) = fs::read_to_string(path) else {
        return (Vec::new(), 0);
    };
    let mut rows: Vec<AuditRecord> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if rows.len() > AUDIT_MAX_READ {
        rows = rows.split_off(rows.len() - AUDIT_MAX_READ);
    }
    rows.reverse();
    let q = q.map(|s| s.to_ascii_lowercase());
    rows.retain(|r| {
        if let Some(a) = action {
            if r.action != a && !r.action.starts_with(&format!("{a}.")) {
                return false;
            }
        }
        if let Some(id) = instance_id {
            if r.instance_id.as_deref() != Some(id) {
                return false;
            }
        }
        if let Some(act) = actor {
            if !r.actor.eq_ignore_ascii_case(act) {
                return false;
            }
        }
        if let Some(needle) = q.as_deref() {
            let detail = r.detail.to_string().to_ascii_lowercase();
            let hay = format!(
                "{} {} {} {}",
                r.action,
                r.actor,
                r.instance_id.as_deref().unwrap_or(""),
                detail
            )
            .to_ascii_lowercase();
            if !hay.contains(needle) {
                return false;
            }
        }
        true
    });
    let total = rows.len();
    let page: Vec<AuditRecord> = rows.into_iter().skip(offset).take(limit).collect();
    (page, total)
}

#[derive(Serialize)]
pub struct AuditEntry<'a> {
    pub at: String,
    pub action: &'a str,
    pub instance_id: Option<&'a str>,
    pub detail: serde_json::Value,
    pub actor: &'a str,
}

tokio::task_local! {
    pub static ACTOR: String;
}

pub async fn with_actor<F, T>(actor: String, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    ACTOR.scope(actor, fut).await
}

pub fn current_actor(fallback: &str) -> String {
    ACTOR
        .try_with(|a| a.clone())
        .unwrap_or_else(|_| fallback.to_string())
}

pub fn audit(action: &str, instance_id: Option<&str>, detail: serde_json::Value, actor: &str) {
    let at = Utc::now().to_rfc3339();
    let resolved = current_actor(actor);
    let entry = AuditEntry {
        at: at.clone(),
        action,
        instance_id,
        detail: detail.clone(),
        actor: resolved.as_str(),
    };
    let path = Path::new("data").join("audit.jsonl");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let (Ok(line), Ok(mut f)) = (
        serde_json::to_string(&entry),
        OpenOptions::new().create(true).append(true).open(path),
    ) {
        let _ = writeln!(f, "{line}");
    }
    if let Some(conn) = audit_conn() {
        if let Ok(guard) = conn.lock() {
            let row = crate::db::AuditRow {
                at,
                action: action.to_string(),
                instance_id: instance_id.map(str::to_string),
                actor: resolved.clone(),
                detail,
            };
            let _ = crate::db::insert_audit(&guard, &row);
            let _ = crate::db::prune_audit(&guard, AUDIT_MAX_ROWS);
        }
    }
}

pub async fn notify_webhook(url: &str, instance_id: &str, status: &str, name: &str) {
    let body = json!({
        "source": "cocktail-manager",
        "instance_id": instance_id,
        "name": name,
        "status": status,
        "at": Utc::now().to_rfc3339(),
    });
    let client = crate::http::client();
    if let Err(e) = client.post(url).json(&body).send().await {
        tracing::warn!(error = %e, "webhook notify failed");
    }
}

#[cfg(test)]
mod tests {
    use super::{inject_jvm_memory, minecraft_ready, parse_command_line};

    #[test]
    fn splits_quoted_startup() {
        let (cmd, args) = parse_command_line(r#"java -jar "My Server.jar" nogui"#).unwrap();
        assert_eq!(cmd, "java");
        assert_eq!(args, vec!["-jar", "My Server.jar", "nogui"]);
        assert!(parse_command_line("   ").is_err());
    }

    #[test]
    fn paper_done_line_is_ready() {
        assert!(minecraft_ready(
            "[00:00:01] [Server thread/INFO]: Done (12.345s)! For help, type \"help\""
        ));
        assert!(!minecraft_ready("Downloading mojang_1.21.10.jar"));
        assert!(!minecraft_ready("> tps"));
    }

    #[test]
    fn jvm_memory_injected_when_missing() {
        let mut args = vec!["-jar".to_string(), "server.jar".to_string()];
        inject_jvm_memory(&mut args, 4096);
        // -Xms is inserted first, then -Xmx lands after it
        assert_eq!(
            &args[..2],
            &["-Xms2048M".to_string(), "-Xmx4096M".to_string()]
        );
        assert!(args.contains(&"-jar".to_string()));
    }

    #[test]
    fn jvm_memory_respects_existing_flags() {
        let mut args = vec![
            "-Xmx2G".to_string(),
            "-Xms1G".to_string(),
            "-jar".to_string(),
        ];
        inject_jvm_memory(&mut args, 4096);
        // user flags win; nothing injected
        assert!(!args.iter().any(|a| a.starts_with("-Xmx4")));
        assert_eq!(args[0], "-Xmx2G");
        assert_eq!(args[1], "-Xms1G");
    }

    #[test]
    fn jvm_memory_small_heaps_clamp_xms() {
        let mut args = Vec::new();
        inject_jvm_memory(&mut args, 256);
        assert_eq!(args[0], "-Xms256M");
        // xms = max(128, 256) = 256 for tiny heaps
        assert_eq!(args[1], "-Xmx256M");
    }
}
