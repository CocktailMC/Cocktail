use regex::Regex;

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
    let base = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .trim()
        .to_ascii_lowercase();
    base == "java" || base == "java.exe" || base.starts_with("java")
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

    let tps_from = TPS_FROM
        .get_or_init(|| Regex::new(r"(?i)TPS from last[^:]+:\s*([0-9]+(?:\.[0-9]+)?)").unwrap());
    let tps_eq =
        TPS_EQ.get_or_init(|| Regex::new(r"(?i)\bTPS[=:\s]+([0-9]+(?:\.[0-9]+)?)").unwrap());
    let mspt = MSPT.get_or_init(|| {
        Regex::new(r"(?i)(?:MSPT|mean tick(?: time)?|tick time)[=:\s]+([0-9]+(?:\.[0-9]+)?)")
            .unwrap()
    });
    let list = LIST.get_or_init(|| {
        Regex::new(r"(?i)There are ([0-9]+) of a max(?:imum)? of ([0-9]+)").unwrap()
    });
    let players = PLAYERS.get_or_init(|| Regex::new(r"(?i)\bplayers[=:\s]+([0-9]+)").unwrap());
    let ent = ENT.get_or_init(|| {
        Regex::new(r"(?i)(?:living )?entit(?:y|ies)(?: count)?[=:\s]+([0-9]+)").unwrap()
    });
    let chunk =
        CHUNK.get_or_init(|| Regex::new(r"(?i)chunks?(?: loaded)?[=:\s]+([0-9]+)").unwrap());
    let heap = HEAP.get_or_init(|| {
        Regex::new(r"(?i)heap(?: memory)?[:\s]+([0-9]+(?:\.[0-9]+)?)\s*([MG])i?B?(?:\s*/\s*([0-9]+(?:\.[0-9]+)?)\s*([MG])i?B?)?").unwrap()
    });
    let gc =
        GC.get_or_init(|| Regex::new(r"(?i)\[(?:full )?gc|pause \(g1|garbage.?collect").unwrap());

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

pub fn minecraft_ready(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if let Some(rest) = lower.split("done (").nth(1) {
        if rest.contains("s)!") {
            return true;
        }
    }
    false
}
