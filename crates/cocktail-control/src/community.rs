use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::crypto;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalBan {
    pub id: String,
    pub target: String,
    pub target_kind: String,
    pub reason: String,
    pub banned_by: String,
    pub banned_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub instances: Vec<String>,
    pub active: bool,
    pub source: String,
}

impl GlobalBan {
    pub fn new(target: &str, kind: &str, reason: &str, by: &str) -> Self {
        Self {
            id: crypto::random_token(16),
            target: target.to_string(),
            target_kind: kind.to_string(),
            reason: reason.to_string(),
            banned_by: by.to_string(),
            banned_at: Utc::now(),
            expires_at: None,
            instances: Vec::new(),
            active: true,
            source: "panel".into(),
        }
    }

    pub fn with_duration(mut self, hours: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::hours(hours));
        self
    }

    pub fn on_instances(mut self, list: &[&str]) -> Self {
        self.instances = list.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn applies_to(&self, instance: &str, now: DateTime<Utc>) -> bool {
        if !self.active {
            return false;
        }
        if self.is_expired(now) {
            return false;
        }
        self.instances.is_empty() || self.instances.iter().any(|i| i == instance)
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.map(|t| t <= now).unwrap_or(false)
    }

    pub fn remaining_hours(&self, now: DateTime<Utc>) -> Option<i64> {
        self.expires_at.map(|t| (t - now).num_hours())
    }
}

pub struct BanRegistry {
    bans: Vec<GlobalBan>,
}

impl BanRegistry {
    pub fn new() -> Self {
        Self { bans: Vec::new() }
    }

    pub fn add(&mut self, ban: GlobalBan) {
        self.bans.push(ban);
    }

    pub fn all(&self) -> &[GlobalBan] {
        &self.bans
    }

    pub fn active_for(&self, instance: &str, now: DateTime<Utc>) -> Vec<&GlobalBan> {
        self.bans
            .iter()
            .filter(|b| b.applies_to(instance, now))
            .collect()
    }

    pub fn is_banned(
        &self,
        target: &str,
        instance: &str,
        now: DateTime<Utc>,
    ) -> Option<&GlobalBan> {
        self.bans
            .iter()
            .find(|b| b.target.eq_ignore_ascii_case(target) && b.applies_to(instance, now))
    }

    pub fn lift(&mut self, id: &str) -> bool {
        match self.bans.iter_mut().find(|b| b.id == id) {
            Some(b) => {
                b.active = false;
                true
            }
            None => false,
        }
    }

    pub fn purge_expired(&mut self, now: DateTime<Utc>) -> usize {
        let before = self.bans.len();
        self.bans.retain(|b| !b.is_expired(now));
        before - self.bans.len()
    }

    pub fn sync_payload(&self, instance: &str, now: DateTime<Utc>) -> Vec<String> {
        self.active_for(instance, now)
            .iter()
            .map(|b| format!("ban {} {}", b.target, b.reason))
            .collect()
    }
}

impl Default for BanRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerAction {
    pub id: String,
    pub at: DateTime<Utc>,
    pub actor: String,
    pub instance_id: String,
    pub action: String,
    pub target: String,
    pub detail: String,
    pub result: String,
}

pub struct ActionLog {
    entries: Vec<PlayerAction>,
    cap: usize,
}

impl ActionLog {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            cap: cap.max(1),
        }
    }

    pub fn record(
        &mut self,
        actor: &str,
        instance_id: &str,
        action: &str,
        target: &str,
        detail: &str,
        result: &str,
    ) -> PlayerAction {
        let entry = PlayerAction {
            id: crypto::random_token(12),
            at: Utc::now(),
            actor: actor.to_string(),
            instance_id: instance_id.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            detail: detail.to_string(),
            result: result.to_string(),
        };
        self.entries.push(entry.clone());
        if self.entries.len() > self.cap {
            let excess = self.entries.len() - self.cap;
            self.entries.drain(0..excess);
        }
        entry
    }

    pub fn by_target(&self, target: &str) -> Vec<&PlayerAction> {
        self.entries
            .iter()
            .filter(|e| e.target.eq_ignore_ascii_case(target))
            .collect()
    }

    pub fn by_actor(&self, actor: &str) -> Vec<&PlayerAction> {
        self.entries.iter().filter(|e| e.actor == actor).collect()
    }

    pub fn by_action(&self, action: &str) -> Vec<&PlayerAction> {
        self.entries.iter().filter(|e| e.action == action).collect()
    }

    pub fn search(&self, needle: &str) -> Vec<&PlayerAction> {
        let lower = needle.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.target.to_ascii_lowercase().contains(&lower)
                    || e.actor.to_ascii_lowercase().contains(&lower)
                    || e.action.to_ascii_lowercase().contains(&lower)
                    || e.detail.to_ascii_lowercase().contains(&lower)
            })
            .collect()
    }

    pub fn all(&self) -> &[PlayerAction] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TicketKind {
    BanAppeal,
    Whitelist,
    Report,
    Support,
}

impl TicketKind {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "appeal" | "ban_appeal" => TicketKind::BanAppeal,
            "whitelist" => TicketKind::Whitelist,
            "report" => TicketKind::Report,
            "support" => TicketKind::Support,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            TicketKind::BanAppeal => "封禁申诉",
            TicketKind::Whitelist => "白名单申请",
            TicketKind::Report => "玩家举报",
            TicketKind::Support => "求助",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TicketState {
    Open,
    Claimed,
    Approved,
    Rejected,
    Closed,
}

impl TicketState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TicketState::Approved | TicketState::Rejected | TicketState::Closed
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            TicketState::Open => "待处理",
            TicketState::Claimed => "处理中",
            TicketState::Approved => "已通过",
            TicketState::Rejected => "已驳回",
            TicketState::Closed => "已关闭",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub kind: TicketKind,
    pub state: TicketState,
    pub player: String,
    pub instance_id: String,
    pub subject: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub handler: Option<String>,
    pub resolution: String,
    pub notes: Vec<(DateTime<Utc>, String, String)>,
}

impl Ticket {
    pub fn new(kind: TicketKind, player: &str, instance: &str, subject: &str, body: &str) -> Self {
        let now = Utc::now();
        Self {
            id: crypto::random_token(12),
            kind,
            state: TicketState::Open,
            player: player.to_string(),
            instance_id: instance.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            created_at: now,
            updated_at: now,
            handler: None,
            resolution: String::new(),
            notes: Vec::new(),
        }
    }

    pub fn claim(&mut self, who: &str) -> bool {
        if self.state.is_terminal() {
            return false;
        }
        self.state = TicketState::Claimed;
        self.handler = Some(who.to_string());
        self.updated_at = Utc::now();
        true
    }

    pub fn approve(&mut self, by: &str, note: &str) -> bool {
        if self.state.is_terminal() {
            return false;
        }
        self.state = TicketState::Approved;
        self.handler = Some(by.to_string());
        self.resolution = note.to_string();
        self.updated_at = Utc::now();
        true
    }

    pub fn reject(&mut self, by: &str, note: &str) -> bool {
        if self.state.is_terminal() {
            return false;
        }
        self.state = TicketState::Rejected;
        self.handler = Some(by.to_string());
        self.resolution = note.to_string();
        self.updated_at = Utc::now();
        true
    }

    pub fn add_note(&mut self, who: &str, text: &str) {
        self.notes
            .push((Utc::now(), who.to_string(), text.to_string()));
        self.updated_at = Utc::now();
    }

    pub fn age_hours(&self, now: DateTime<Utc>) -> i64 {
        (now - self.created_at).num_hours()
    }
}

pub struct TicketQueue {
    tickets: Vec<Ticket>,
}

impl TicketQueue {
    pub fn new() -> Self {
        Self {
            tickets: Vec::new(),
        }
    }

    pub fn submit(&mut self, ticket: Ticket) -> String {
        let id = ticket.id.clone();
        self.tickets.push(ticket);
        id
    }

    pub fn open(&self) -> Vec<&Ticket> {
        self.tickets
            .iter()
            .filter(|t| !t.state.is_terminal())
            .collect()
    }

    pub fn by_player(&self, player: &str) -> Vec<&Ticket> {
        self.tickets
            .iter()
            .filter(|t| t.player.eq_ignore_ascii_case(player))
            .collect()
    }

    pub fn by_kind(&self, kind: &TicketKind) -> Vec<&Ticket> {
        self.tickets.iter().filter(|t| &t.kind == kind).collect()
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Ticket> {
        self.tickets.iter_mut().find(|t| t.id == id)
    }

    pub fn stats(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for t in &self.tickets {
            *out.entry(t.state.label().to_string()).or_insert(0) += 1;
        }
        out
    }

    pub fn oldest_open(&self) -> Option<&Ticket> {
        self.tickets
            .iter()
            .filter(|t| !t.state.is_terminal())
            .min_by_key(|t| t.created_at)
    }
}

impl Default for TicketQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerProfileSummary {
    pub name: String,
    pub uuid: Option<String>,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub total_secs: u64,
    pub sessions: u32,
    pub ips: BTreeSet<String>,
    pub ban_count: u32,
    pub action_count: u32,
}

impl PlayerProfileSummary {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            uuid: None,
            first_seen: None,
            last_seen: None,
            total_secs: 0,
            sessions: 0,
            ips: BTreeSet::new(),
            ban_count: 0,
            action_count: 0,
        }
    }

    pub fn observe(&mut self, at: DateTime<Utc>, secs: u64, ip: Option<&str>) {
        if self.first_seen.is_none() {
            self.first_seen = Some(at);
        }
        self.last_seen = Some(at);
        self.total_secs = self.total_secs.saturating_add(secs);
        self.sessions = self.sessions.saturating_add(1);
        if let Some(ip) = ip {
            if !ip.is_empty() {
                self.ips.insert(ip.to_string());
            }
        }
    }

    pub fn hours_played(&self) -> f64 {
        self.total_secs as f64 / 3600.0
    }

    pub fn days_since_first(&self, now: DateTime<Utc>) -> Option<i64> {
        self.first_seen.map(|t| (now - t).num_days())
    }

    pub fn days_since_last(&self, now: DateTime<Utc>) -> Option<i64> {
        self.last_seen.map(|t| (now - t).num_days())
    }

    pub fn risk_score(&self) -> u8 {
        let mut score = 0u8;
        if self.ips.len() > 5 {
            score += 2;
        } else if self.ips.len() > 2 {
            score += 1;
        }
        if self.ban_count > 0 {
            score += (self.ban_count.min(3) * 2) as u8;
        }
        if self.total_secs < 600 && self.sessions > 0 {
            score += 1;
        }
        score.min(10)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityBucket {
    pub hour: u32,
    pub sessions: u32,
}

pub fn activity_heatmap(events: &[(DateTime<Utc>, String)], player: &str) -> Vec<ActivityBucket> {
    let mut buckets: BTreeMap<u32, u32> = (0..24).map(|h| (h, 0)).collect();
    for (at, who) in events {
        if !who.eq_ignore_ascii_case(player) {
            continue;
        }
        let hour = at.format("%H").to_string().parse::<u32>().unwrap_or(0);
        if let Some(b) = buckets.get_mut(&hour) {
            *b += 1;
        }
    }
    buckets
        .into_iter()
        .map(|(hour, sessions)| ActivityBucket { hour, sessions })
        .collect()
}

pub fn peak_hour(heatmap: &[ActivityBucket]) -> Option<u32> {
    heatmap
        .iter()
        .max_by_key(|b| b.sessions)
        .filter(|b| b.sessions > 0)
        .map(|b| b.hour)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub name: String,
    pub hours: f64,
    pub rank: usize,
}

pub fn leaderboard(profiles: &[PlayerProfileSummary], limit: usize) -> Vec<LeaderboardEntry> {
    let mut sorted: Vec<&PlayerProfileSummary> = profiles.iter().collect();
    sorted.sort_by(|a, b| {
        b.total_secs
            .cmp(&a.total_secs)
            .then_with(|| a.name.cmp(&b.name))
    });
    sorted
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, p)| LeaderboardEntry {
            name: p.name.clone(),
            hours: p.hours_played(),
            rank: i + 1,
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeoHint {
    pub ip: String,
    pub country: String,
    pub city: String,
    pub asn: String,
}

impl GeoHint {
    pub fn unknown(ip: &str) -> Self {
        Self {
            ip: ip.to_string(),
            country: "未知".into(),
            city: "未知".into(),
            asn: String::new(),
        }
    }

    pub fn describe(&self) -> String {
        if self.city.is_empty() || self.city == "未知" {
            self.country.clone()
        } else {
            format!("{}, {}", self.city, self.country)
        }
    }

    pub fn is_private(ip: &str) -> bool {
        let clean = ip.trim_start_matches("::ffff:");
        if let Ok(v4) = clean.parse::<std::net::Ipv4Addr>() {
            return v4.is_private() || v4.is_loopback() || v4.is_link_local();
        }
        if let Ok(v6) = clean.parse::<std::net::Ipv6Addr>() {
            return v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00;
        }
        false
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AvatarSource {
    pub kind: String,
    pub template: String,
}

impl AvatarSource {
    pub fn crafatar() -> Self {
        Self {
            kind: "crafatar".into(),
            template: "https://crafatar.com/avatars/{uuid}?size=64&overlay".into(),
        }
    }

    pub fn minotar() -> Self {
        Self {
            kind: "minotar".into(),
            template: "https://minotar.net/avatar/{name}/64".into(),
        }
    }

    pub fn url_for(&self, name: &str, uuid: Option<&str>) -> String {
        match (self.kind.as_str(), uuid) {
            ("crafatar", Some(u)) => self.template.replace("{uuid}", u),
            _ => self.template.replace("{name}", name),
        }
    }

    pub fn fallback(name: &str) -> String {
        format!("https://mc-heads.net/avatar/{name}/64")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicPortalConfig {
    pub enabled: bool,
    pub base_url: String,
    pub allow_self_ban_lookup: bool,
    pub allow_appeal_submit: bool,
    pub require_email: bool,
    pub rate_limit_per_hour: u32,
}

impl Default for PublicPortalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            allow_self_ban_lookup: true,
            allow_appeal_submit: true,
            require_email: false,
            rate_limit_per_hour: 5,
        }
    }
}

impl PublicPortalConfig {
    pub fn can_submit(&self, submissions_last_hour: u32) -> bool {
        self.enabled && self.allow_appeal_submit && submissions_last_hour < self.rate_limit_per_hour
    }

    pub fn appeal_url(&self, ticket_id: &str) -> String {
        format!("{}/appeal/{ticket_id}", self.base_url.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ban_applies_scoped_and_expires() {
        let now = Utc::now();
        let ban = GlobalBan::new("Griefer", "player", "破坏", "root").on_instances(&["survival"]);
        assert!(ban.applies_to("survival", now));
        assert!(!ban.applies_to("creative", now));
        let temp = GlobalBan::new("Cheater", "player", "作弊", "root").with_duration(-1);
        assert!(temp.is_expired(now));
        assert!(!temp.applies_to("survival", now));
    }

    #[test]
    fn ban_registry_operations() {
        let now = Utc::now();
        let mut reg = BanRegistry::new();
        let ban = GlobalBan::new("A", "player", "x", "root");
        let id = ban.id.clone();
        reg.add(ban);
        reg.add(GlobalBan::new("B", "ip", "y", "root").on_instances(&["other"]));
        assert!(reg.is_banned("a", "any", now).is_some());
        assert!(reg.is_banned("b", "any", now).is_none());
        assert!(reg.is_banned("b", "other", now).is_some());
        assert_eq!(reg.active_for("any", now).len(), 1);
        assert!(reg.lift(&id));
        assert!(reg.is_banned("a", "any", now).is_none());
        assert!(!reg.lift("nope"));
    }

    #[test]
    fn ban_sync_payload_format() {
        let now = Utc::now();
        let mut reg = BanRegistry::new();
        reg.add(GlobalBan::new("Zed", "player", "spam", "root"));
        let payload = reg.sync_payload("s1", now);
        assert_eq!(payload.len(), 1);
        assert!(payload[0].starts_with("ban Zed"));
    }

    #[test]
    fn action_log_filters_and_caps() {
        let mut log = ActionLog::new(3);
        log.record("root", "i1", "kick", "Alice", "", "ok");
        log.record("support", "i1", "ban", "Bob", "cheat", "ok");
        log.record("root", "i1", "op", "Alice", "", "ok");
        assert_eq!(log.by_target("alice").len(), 2);
        assert_eq!(log.by_actor("root").len(), 2);
        assert_eq!(log.by_action("ban").len(), 1);
        assert_eq!(log.search("cheat").len(), 1);
        log.record("root", "i1", "kick", "Carol", "", "ok");
        assert_eq!(log.len(), 3);
        assert_eq!(log.by_target("alice").len(), 1);
        assert_eq!(log.by_target("bob").len(), 1);
    }

    #[test]
    fn ticket_lifecycle() {
        let mut q = TicketQueue::new();
        let mut t = Ticket::new(TicketKind::BanAppeal, "Bob", "s1", "申诉", "我没有作弊");
        let id = q.submit(t.clone());
        assert_eq!(q.open().len(), 1);
        t = q.get_mut(&id).unwrap().clone();
        assert!(t.claim("admin1"));
        assert_eq!(t.state, TicketState::Claimed);
        assert!(t.approve("admin1", "解封"));
        assert!(t.state.is_terminal());
        assert!(!t.reject("admin2", "不行"));
        q.get_mut(&id).unwrap().approve("admin1", "解封");
        assert!(q.open().is_empty());
    }

    #[test]
    fn ticket_notes_and_stats() {
        let mut q = TicketQueue::new();
        let id = q.submit(Ticket::new(
            TicketKind::Whitelist,
            "Cara",
            "s1",
            "申请",
            "想加入",
        ));
        {
            let t = q.get_mut(&id).unwrap();
            t.add_note("admin", "已核对");
            assert_eq!(t.notes.len(), 1);
        }
        let stats = q.stats();
        assert_eq!(stats.get("待处理"), Some(&1));
        assert!(q.oldest_open().is_some());
        assert_eq!(q.by_player("cara").len(), 1);
        assert_eq!(q.by_kind(&TicketKind::Whitelist).len(), 1);
    }

    #[test]
    fn ticket_kind_and_state_parsing() {
        assert_eq!(TicketKind::parse("appeal"), Some(TicketKind::BanAppeal));
        assert_eq!(TicketKind::parse("report"), Some(TicketKind::Report));
        assert_eq!(TicketKind::parse("nope"), None);
        assert!(TicketState::Approved.is_terminal());
        assert!(!TicketState::Open.is_terminal());
    }

    #[test]
    fn profile_accumulates_stats() {
        let mut p = PlayerProfileSummary::new("Alice");
        let now = Utc::now();
        p.observe(now - chrono::Duration::days(30), 3600, Some("1.2.3.4"));
        p.observe(now, 1800, Some("5.6.7.8"));
        assert_eq!(p.sessions, 2);
        assert_eq!(p.total_secs, 5400);
        assert!((p.hours_played() - 1.5).abs() < 0.01);
        assert_eq!(p.ips.len(), 2);
        assert_eq!(p.days_since_first(now), Some(30));
        assert_eq!(p.days_since_last(now), Some(0));
    }

    #[test]
    fn profile_risk_scoring() {
        let mut p = PlayerProfileSummary::new("Suspicious");
        assert_eq!(p.risk_score(), 0);
        for i in 0..6 {
            p.ips.insert(format!("10.0.0.{i}"));
        }
        assert!(p.risk_score() >= 2);
        p.ban_count = 2;
        assert!(p.risk_score() >= 5);
    }

    #[test]
    fn heatmap_and_peak() {
        let base = chrono::DateTime::parse_from_rfc3339("2026-05-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let events = vec![
            (base + chrono::Duration::hours(20), "Alice".to_string()),
            (base + chrono::Duration::hours(20), "Alice".to_string()),
            (base + chrono::Duration::hours(3), "Alice".to_string()),
            (base + chrono::Duration::hours(20), "Bob".to_string()),
        ];
        let hm = activity_heatmap(&events, "alice");
        assert_eq!(hm.len(), 24);
        assert_eq!(hm[20].sessions, 2);
        assert_eq!(peak_hour(&hm), Some(20));
    }

    #[test]
    fn leaderboard_orders_by_hours() {
        let mut a = PlayerProfileSummary::new("Alice");
        a.total_secs = 7200;
        let mut b = PlayerProfileSummary::new("Bob");
        b.total_secs = 36000;
        let lb = leaderboard(&[a, b], 10);
        assert_eq!(lb[0].name, "Bob");
        assert_eq!(lb[0].rank, 1);
        assert!((lb[0].hours - 10.0).abs() < 0.01);
    }

    #[test]
    fn geo_helpers() {
        let g = GeoHint::unknown("1.2.3.4");
        assert_eq!(g.describe(), "未知");
        let real = GeoHint {
            ip: "8.8.8.8".into(),
            country: "美国".into(),
            city: "洛杉矶".into(),
            asn: "AS15169".into(),
        };
        assert!(real.describe().contains("洛杉矶"));
        assert!(GeoHint::is_private("192.168.1.1"));
        assert!(GeoHint::is_private("127.0.0.1"));
        assert!(GeoHint::is_private("::1"));
        assert!(!GeoHint::is_private("8.8.8.8"));
    }

    #[test]
    fn avatar_url_generation() {
        let c = AvatarSource::crafatar();
        let url = c.url_for("Alice", Some("uuid-123"));
        assert!(url.contains("uuid-123"));
        let m = AvatarSource::minotar();
        assert!(m.url_for("Alice", None).contains("Alice"));
        assert!(AvatarSource::fallback("Bob").contains("Bob"));
    }

    #[test]
    fn portal_rate_limits() {
        let mut p = PublicPortalConfig::default();
        assert!(!p.can_submit(0));
        p.enabled = true;
        assert!(p.can_submit(0));
        assert!(!p.can_submit(5));
        assert!(p.appeal_url("t1").ends_with("/appeal/t1"));
    }
}
