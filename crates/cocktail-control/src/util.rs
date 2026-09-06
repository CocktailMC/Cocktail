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
    pub mspt: Option<f32>,
    pub players: Option<u32>,
    pub players_max: Option<u32>,
    pub entities: Option<u32>,
    pub chunks: Option<u32>,
    pub gc_delta: u32,
    pub heap_used_mib: Option<f32>,
    pub heap_max_mib: Option<f32>,
}

pub fn merge_game_stats(dst: &mut ParsedGameStats, src: &ParsedGameStats) {
    if src.tps.is_some() {
        dst.tps = src.tps;
    }
    if src.mspt.is_some() {
        dst.mspt = src.mspt;
    }
    if src.players.is_some() {
        dst.players = src.players;
    }
    if src.players_max.is_some() {
        dst.players_max = src.players_max;
    }
    if src.entities.is_some() {
        dst.entities = src.entities;
    }
    if src.chunks.is_some() {
        dst.chunks = src.chunks;
    }
    if src.gc_delta > 0 {
        dst.gc_delta = dst.gc_delta.saturating_add(src.gc_delta);
    }
    if src.heap_used_mib.is_some() {
        dst.heap_used_mib = src.heap_used_mib;
    }
    if src.heap_max_mib.is_some() {
        dst.heap_max_mib = src.heap_max_mib;
    }
}

pub fn parse_game_stats(line: &str) -> ParsedGameStats {
    static TPS_FROM: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static TPS_EQ: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static MSPT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static LIST: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static PLAYERS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static ENT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static CHUNK: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static HEAP: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static GC: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

    let tps_from = TPS_FROM.get_or_init(|| {
        Regex::new(r"(?i)TPS from last[^:]+:\s*([0-9]+(?:\.[0-9]+)?)").unwrap()
    });
    let tps_eq = TPS_EQ.get_or_init(|| {
        Regex::new(r"(?i)\bTPS[=:\s]+([0-9]+(?:\.[0-9]+)?)").unwrap()
    });
    let mspt = MSPT.get_or_init(|| {
        Regex::new(r"(?i)(?:MSPT|mean tick(?: time)?|tick time)[=:\s]+([0-9]+(?:\.[0-9]+)?)").unwrap()
    });
    let list = LIST.get_or_init(|| {
        Regex::new(r"(?i)There are ([0-9]+) of a max(?:imum)? of ([0-9]+)").unwrap()
    });
    let players = PLAYERS.get_or_init(|| {
        Regex::new(r"(?i)\bplayers[=:\s]+([0-9]+)").unwrap()
    });
    let ent = ENT.get_or_init(|| {
        Regex::new(r"(?i)(?:living )?entit(?:y|ies)(?: count)?[=:\s]+([0-9]+)").unwrap()
    });
    let chunk = CHUNK.get_or_init(|| {
        Regex::new(r"(?i)chunks?(?: loaded)?[=:\s]+([0-9]+)").unwrap()
    });
    let heap = HEAP.get_or_init(|| {
        Regex::new(r"(?i)heap(?: memory)?[:\s]+([0-9]+(?:\.[0-9]+)?)\s*([MG])i?B?(?:\s*/\s*([0-9]+(?:\.[0-9]+)?)\s*([MG])i?B?)?").unwrap()
    });
    let gc = GC.get_or_init(|| {
        Regex::new(r"(?i)\[(?:full )?gc|pause \(g1|garbage.?collect").unwrap()
    });

    let mut stats = ParsedGameStats::default();
    if let Some(c) = tps_from.captures(line).or_else(|| tps_eq.captures(line)) {
        if let Ok(v) = c[1].parse::<f32>() {
            stats.tps = Some(v.clamp(0.0, 21.0));
        }
    }
    if let Some(c) = mspt.captures(line) {
        if let Ok(v) = c[1].parse::<f32>() {
            stats.mspt = Some(v);
        }
    }
    if let Some(c) = list.captures(line) {
        stats.players = c[1].parse().ok();
        stats.players_max = c[2].parse().ok();
    } else if let Some(c) = players.captures(line) {
        stats.players = c[1].parse().ok();
    }
    if let Some(c) = ent.captures(line) {
        stats.entities = c[1].parse().ok();
    }
    if let Some(c) = chunk.captures(line) {
        stats.chunks = c[1].parse().ok();
    }
    if let Some(c) = heap.captures(line) {
        let used = c[1].parse::<f32>().ok();
        let unit = c.get(2).map(|m| m.as_str());
        stats.heap_used_mib = used.map(|n| if unit == Some("G") { n * 1024.0 } else { n });
        if let (Some(max), Some(u2)) = (c.get(3), c.get(4)) {
            if let Ok(n) = max.as_str().parse::<f32>() {
                stats.heap_max_mib = Some(if u2.as_str() == "G" { n * 1024.0 } else { n });
            }
        }
    }
    if gc.is_match(line) {
        stats.gc_delta = 1;
    }
    stats
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
