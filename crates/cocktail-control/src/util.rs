//! Shared helpers: JVM flags, properties, log parsing, audit, webhook.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn inject_jvm_memory(args: &mut Vec<String>, memory_mib: u32) {
    let has_xmx = args.iter().any(|a| a.starts_with("-Xmx"));
    let has_xms = args.iter().any(|a| a.starts_with("-Xms"));
    if !has_xmx {
        args.insert(0, format!("-Xmx{memory_mib}M"));
    }
    if !has_xms {
        let xms = (memory_mib / 2).max(256).min(memory_mib);
        args.insert(0, format!("-Xms{xms}M"));
    }
}

pub fn is_java_command(command: &str) -> bool {
    let base = Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();
    base == "java" || base == "java.exe" || base.starts_with("java")
}

/// Default Minecraft server launch line for a jar relative to workdir.
pub fn java_jar_startup(jar_rel: &str) -> (String, Vec<String>) {
    let jar = jar_rel.replace('\\', "/").trim_start_matches('/').to_string();
    (
        "java".into(),
        vec!["-jar".into(), jar, "nogui".into()],
    )
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

#[derive(Debug, Default, Clone)]
pub struct ParsedGameStats {
    pub tps: Option<f32>,
    pub players: Option<u32>,
}

pub fn parse_game_stats(line: &str) -> ParsedGameStats {
    static TPS_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static PLAYERS_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static LIST_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

    let tps_re = TPS_RE.get_or_init(|| {
        Regex::new(r"(?i)TPS[=:\s]+([0-9]+(?:\.[0-9]+)?)").expect("tps regex")
    });
    let players_re = PLAYERS_RE.get_or_init(|| {
        Regex::new(r"(?i)players[=:\s]+([0-9]+)").expect("players regex")
    });
    let list_re = LIST_RE.get_or_init(|| {
        Regex::new(r"(?i)There are ([0-9]+) of a max").expect("list regex")
    });

    let mut stats = ParsedGameStats::default();
    if let Some(c) = tps_re.captures(line) {
        if let Ok(v) = c[1].parse::<f32>() {
            stats.tps = Some(v);
        }
    }
    if let Some(c) = players_re.captures(line) {
        if let Ok(v) = c[1].parse::<u32>() {
            stats.players = Some(v);
        }
    }
    if let Some(c) = list_re.captures(line) {
        if let Ok(v) = c[1].parse::<u32>() {
            stats.players = Some(v);
        }
    }
    stats
}

pub fn append_instance_log(instance_id: &str, stream: &str, line: &str) {
    let path = Path::new("data").join("logs").join(format!("{instance_id}.log"));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            f,
            "{} [{}] {}",
            Utc::now().to_rfc3339(),
            stream,
            line
        );
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

pub fn list_audit(
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

    let action = action.map(str::trim).filter(|s| !s.is_empty());
    let instance_id = instance_id.map(str::trim).filter(|s| !s.is_empty());
    let actor = actor.map(str::trim).filter(|s| !s.is_empty());
    let q = q.map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty());

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
    let limit = limit.clamp(1, 200);
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

pub fn audit(action: &str, instance_id: Option<&str>, detail: serde_json::Value, actor: &str) {
    let path = Path::new("data").join("audit.jsonl");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = AuditEntry {
        at: Utc::now().to_rfc3339(),
        action,
        instance_id,
        detail,
        actor,
    };
    if let (Ok(line), Ok(mut f)) = (
        serde_json::to_string(&entry),
        OpenOptions::new().create(true).append(true).open(path),
    ) {
        let _ = writeln!(f, "{line}");
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
    let client = reqwest::Client::new();
    if let Err(e) = client.post(url).json(&body).send().await {
        tracing::warn!(error = %e, "webhook notify failed");
    }
}
