//! Online player name parsing from console output.

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
