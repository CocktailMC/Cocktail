use std::collections::{BTreeMap, VecDeque};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::crypto;

pub fn normalize_code(raw: &str) -> String {
    raw.trim()
        .to_uppercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryCode {
    pub hash: String,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoverySet {
    pub admin_id: i64,
    pub created_at: DateTime<Utc>,
    pub codes: Vec<RecoveryCode>,
}

impl RecoverySet {
    pub fn generate(admin_id: i64, count: usize) -> (Self, Vec<String>) {
        let mut plain = Vec::with_capacity(count);
        let mut codes = Vec::with_capacity(count);
        for _ in 0..count {
            let raw = format!(
                "{}-{}",
                crypto::random_token(5).to_uppercase(),
                crypto::random_token(5).to_uppercase()
            );
            let hash = crypto::sha256_hex(normalize_code(&raw).as_bytes());
            codes.push(RecoveryCode {
                hash,
                used_at: None,
            });
            plain.push(raw);
        }
        (
            Self {
                admin_id,
                created_at: Utc::now(),
                codes,
            },
            plain,
        )
    }

    pub fn remaining(&self) -> usize {
        self.codes.iter().filter(|c| c.used_at.is_none()).count()
    }

    pub fn is_low(&self) -> bool {
        self.remaining() <= 2
    }

    pub fn consume(&mut self, presented: &str) -> bool {
        let normalized = normalize_code(presented);
        if normalized.is_empty() {
            return false;
        }
        let hash = crypto::sha256_hex(normalized.as_bytes());
        for code in self.codes.iter_mut() {
            if code.used_at.is_some() {
                continue;
            }
            if crypto::ct_eq(hash.as_bytes(), code.hash.as_bytes()) {
                code.used_at = Some(Utc::now());
                return true;
            }
        }
        false
    }

    pub fn revoke_all(&mut self) -> usize {
        let now = Utc::now();
        let mut n = 0;
        for code in self.codes.iter_mut() {
            if code.used_at.is_none() {
                code.used_at = Some(now);
                n += 1;
            }
        }
        n
    }

    pub fn regenerate(&mut self, count: usize) -> Vec<String> {
        let (_, plain) = RecoverySet::generate(self.admin_id, count);
        self.created_at = Utc::now();
        self.codes = plain
            .iter()
            .map(|raw| RecoveryCode {
                hash: crypto::sha256_hex(normalize_code(raw).as_bytes()),
                used_at: None,
            })
            .collect();
        plain
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricPoint {
    pub at: DateTime<Utc>,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub points: VecDeque<MetricPoint>,
    pub cap: usize,
}

impl Series {
    pub fn new(name: &str, cap: usize) -> Self {
        Self {
            name: name.to_string(),
            labels: BTreeMap::new(),
            points: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    pub fn push(&mut self, at: DateTime<Utc>, value: f64) {
        self.points.push_back(MetricPoint { at, value });
        while self.points.len() > self.cap {
            self.points.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn latest(&self) -> Option<f64> {
        self.points.back().map(|p| p.value)
    }

    pub fn first(&self) -> Option<f64> {
        self.points.front().map(|p| p.value)
    }

    pub fn min(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.value)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map(|a| a.min(v)).unwrap_or(v))
            })
    }

    pub fn max(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.value)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map(|a| a.max(v)).unwrap_or(v))
            })
    }

    pub fn avg(&self) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        let sum: f64 = self.points.iter().map(|p| p.value).sum();
        Some(sum / self.points.len() as f64)
    }

    pub fn rate_per_sec(&self) -> Option<f64> {
        let first = self.points.front()?;
        let last = self.points.back()?;
        let span = (last.at - first.at).num_seconds();
        if span <= 0 || self.points.len() < 2 {
            return None;
        }
        let delta = last.value - first.value;
        Some(delta / span as f64)
    }

    pub fn since(&self, at: DateTime<Utc>) -> Vec<MetricPoint> {
        self.points.iter().filter(|p| p.at >= at).cloned().collect()
    }

    pub fn downsample(&self, buckets: usize) -> Vec<f64> {
        if buckets == 0 || self.points.is_empty() {
            return Vec::new();
        }
        if self.points.len() <= buckets {
            return self.points.iter().map(|p| p.value).collect();
        }
        let size = self.points.len().div_ceil(buckets);
        let mut out = Vec::new();
        for chunk in self.points.iter().collect::<Vec<_>>().chunks(size) {
            let sum: f64 = chunk.iter().map(|p| p.value).sum();
            out.push(sum / chunk.len() as f64);
        }
        out
    }

    pub fn to_prometheus(&self) -> String {
        let mut out = String::new();
        for point in &self.points {
            let ts = point.at.timestamp_millis();
            if self.labels.is_empty() {
                out.push_str(&format!("{} {} {ts}\n", self.name, point.value));
            } else {
                let parts: Vec<String> = self
                    .labels
                    .iter()
                    .map(|(k, v)| format!("{k}=\"{v}\""))
                    .collect();
                out.push_str(&format!(
                    "{}{{{}}} {} {ts}\n",
                    self.name,
                    parts.join(","),
                    point.value
                ));
            }
        }
        out
    }
}

pub struct SeriesStore {
    series: BTreeMap<String, Series>,
    cap: usize,
}

impl SeriesStore {
    pub fn new(cap: usize) -> Self {
        Self {
            series: BTreeMap::new(),
            cap,
        }
    }

    pub fn record(&mut self, name: &str, at: DateTime<Utc>, value: f64) {
        let cap = self.cap;
        self.series
            .entry(name.to_string())
            .or_insert_with(|| Series::new(name, cap))
            .push(at, value);
    }

    pub fn get(&self, name: &str) -> Option<&Series> {
        self.series.get(name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.series.keys().map(|s| s.as_str()).collect()
    }

    pub fn trend(&self, name: &str) -> Option<&'static str> {
        let s = self.series.get(name)?;
        if s.len() < 2 {
            return Some("平稳");
        }
        let first = s.first()?;
        let last = s.latest()?;
        let delta = last - first;
        let base = first.abs().max(1.0);
        let pct = delta / base * 100.0;
        Some(if pct > 10.0 {
            "上升"
        } else if pct < -10.0 {
            "下降"
        } else {
            "平稳"
        })
    }

    pub fn purge_before(&mut self, at: DateTime<Utc>) -> usize {
        let mut removed = 0;
        for series in self.series.values_mut() {
            let before = series.points.len();
            series.points.retain(|p| p.at >= at);
            removed += before - series.points.len();
        }
        removed
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum DeliveryState {
    Pending,
    Delivered,
    Failed,
    Dead,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub url: String,
    pub event: String,
    pub payload: String,
    pub signature: String,
    pub state: DeliveryState,
    pub attempts: u32,
    pub max_attempts: u32,
    pub next_attempt_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Delivery {
    pub fn new(url: &str, event: &str, payload: &str, secret: &str) -> Self {
        let mac = crypto::hmac_sha256(secret.as_bytes(), payload.as_bytes());
        Self {
            id: crypto::random_token(16),
            url: url.to_string(),
            event: event.to_string(),
            payload: payload.to_string(),
            signature: format!("sha256={}", crypto::hex(&mac)),
            state: DeliveryState::Pending,
            attempts: 0,
            max_attempts: 5,
            next_attempt_at: Utc::now(),
            last_error: None,
            created_at: Utc::now(),
        }
    }

    pub fn due(&self, now: DateTime<Utc>) -> bool {
        matches!(self.state, DeliveryState::Pending | DeliveryState::Failed)
            && self.next_attempt_at <= now
    }

    pub fn backoff_secs(&self) -> i64 {
        let exp = self.attempts.min(10);
        (30i64 * (1i64 << exp)).min(3600)
    }

    pub fn mark_delivered(&mut self) {
        self.state = DeliveryState::Delivered;
        self.attempts = self.attempts.saturating_add(1);
        self.last_error = None;
    }

    pub fn mark_failed(&mut self, error: &str) {
        self.attempts = self.attempts.saturating_add(1);
        self.last_error = Some(error.to_string());
        if self.attempts >= self.max_attempts {
            self.state = DeliveryState::Dead;
        } else {
            self.state = DeliveryState::Failed;
            self.next_attempt_at = Utc::now() + chrono::Duration::seconds(self.backoff_secs());
        }
    }

    pub fn verify(&self, secret: &str, body: &str) -> bool {
        let mac = crypto::hmac_sha256(secret.as_bytes(), body.as_bytes());
        let expect = format!("sha256={}", crypto::hex(&mac));
        crypto::ct_eq(expect.as_bytes(), self.signature.as_bytes())
    }
}

pub struct DeliveryQueue {
    items: Vec<Delivery>,
}

impl DeliveryQueue {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn enqueue(&mut self, delivery: Delivery) {
        self.items.push(delivery);
    }

    pub fn due(&self, now: DateTime<Utc>) -> Vec<&Delivery> {
        self.items.iter().filter(|d| d.due(now)).collect()
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Delivery> {
        self.items.iter_mut().find(|d| d.id == id)
    }

    pub fn stats(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for d in &self.items {
            let key = match d.state {
                DeliveryState::Pending => "待发送",
                DeliveryState::Delivered => "已送达",
                DeliveryState::Failed => "重试中",
                DeliveryState::Dead => "已放弃",
            };
            *out.entry(key.to_string()).or_insert(0) += 1;
        }
        out
    }

    pub fn prune_delivered(&mut self, keep: usize) -> usize {
        let delivered: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, d)| d.state == DeliveryState::Delivered)
            .map(|(i, _)| i)
            .collect();
        if delivered.len() <= keep {
            return 0;
        }
        let drop_count = delivered.len() - keep;
        let drop_set: std::collections::BTreeSet<usize> =
            delivered.into_iter().take(drop_count).collect();
        let before = self.items.len();
        let mut idx = 0;
        self.items.retain(|_| {
            let keep_item = !drop_set.contains(&idx);
            idx += 1;
            keep_item
        });
        before - self.items.len()
    }
}

impl Default for DeliveryQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationTarget {
    pub id: String,
    pub kind: String,
    pub url: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub secret: String,
}

impl NotificationTarget {
    pub fn new(kind: &str, url: &str) -> Self {
        Self {
            id: crypto::random_token(12),
            kind: kind.to_string(),
            url: url.to_string(),
            events: Vec::new(),
            enabled: true,
            secret: crypto::random_token(32),
        }
    }

    pub fn subscribe(mut self, events: &[&str]) -> Self {
        self.events = events.iter().map(|e| e.to_string()).collect();
        self
    }

    pub fn wants(&self, event: &str) -> bool {
        if !self.enabled {
            return false;
        }
        self.events.is_empty() || self.events.iter().any(|e| e == event || e == "*")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PanelEventOut {
    pub id: String,
    pub at: DateTime<Utc>,
    pub event: String,
    pub instance_id: Option<String>,
    pub severity: String,
    pub summary: String,
    pub payload: serde_json::Value,
}

impl PanelEventOut {
    pub fn new(event: &str, severity: &str, summary: &str) -> Self {
        Self {
            id: crypto::random_token(12),
            at: Utc::now(),
            event: event.to_string(),
            instance_id: None,
            severity: severity.to_string(),
            summary: summary.to_string(),
            payload: serde_json::Value::Null,
        }
    }

    pub fn on(mut self, instance: &str) -> Self {
        self.instance_id = Some(instance.to_string());
        self
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn body(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn is_critical(&self) -> bool {
        self.severity == "critical"
    }
}

pub fn fanout(targets: &[NotificationTarget], event: &PanelEventOut) -> Vec<Delivery> {
    targets
        .iter()
        .filter(|t| t.wants(&event.event))
        .map(|t| Delivery::new(&t.url, &event.event, &event.body(), &t.secret))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_generate_and_consume() {
        let (mut set, plain) = RecoverySet::generate(1, 10);
        assert_eq!(plain.len(), 10);
        assert_eq!(set.remaining(), 10);
        assert!(set.consume(&plain[0]));
        assert!(!set.consume(&plain[0]));
        assert_eq!(set.remaining(), 9);
        assert!(!set.consume("BOGUS-CODE"));
    }

    #[test]
    fn recovery_codes_normalize_input() {
        let (mut set, plain) = RecoverySet::generate(1, 3);
        let spaced = plain[0].to_lowercase().replace('-', " ");
        assert!(set.consume(&spaced));
    }

    #[test]
    fn recovery_low_watermark_and_revoke() {
        let (mut set, plain) = RecoverySet::generate(1, 10);
        assert!(!set.is_low());
        for code in plain.iter().take(8) {
            assert!(set.consume(code));
        }
        assert!(set.is_low());
        assert_eq!(set.remaining(), 2);
        assert_eq!(set.revoke_all(), 2);
        assert_eq!(set.remaining(), 0);
    }

    #[test]
    fn recovery_regenerate_invalidates_old() {
        let (mut set, old) = RecoverySet::generate(1, 5);
        let fresh = set.regenerate(5);
        assert_eq!(fresh.len(), 5);
        assert_eq!(set.remaining(), 5);
        assert!(!set.consume(&old[0]));
        assert!(set.consume(&fresh[0]));
    }

    #[test]
    fn series_statistics() {
        let mut s = Series::new("cpu", 100);
        let base = Utc::now();
        for i in 0..10 {
            s.push(base + chrono::Duration::seconds(i), i as f64);
        }
        assert_eq!(s.len(), 10);
        assert_eq!(s.first(), Some(0.0));
        assert_eq!(s.latest(), Some(9.0));
        assert_eq!(s.min(), Some(0.0));
        assert_eq!(s.max(), Some(9.0));
        assert_eq!(s.avg(), Some(4.5));
        assert!(s.rate_per_sec().unwrap() > 0.9);
    }

    #[test]
    fn series_capacity_and_downsample() {
        let mut s = Series::new("m", 5);
        let base = Utc::now();
        for i in 0..20 {
            s.push(base + chrono::Duration::seconds(i), i as f64);
        }
        assert_eq!(s.len(), 5);
        assert_eq!(s.first(), Some(15.0));
        let down = s.downsample(2);
        assert_eq!(down.len(), 2);
        assert!(down[0] < down[1]);
    }

    #[test]
    fn series_prometheus_output() {
        let mut s = Series::new("node_cpu", 10).with_label("node", "n1");
        s.push(Utc::now(), 42.0);
        let text = s.to_prometheus();
        assert!(text.starts_with("node_cpu{node=\"n1\"} 42"));
    }

    #[test]
    fn series_store_trends_and_purge() {
        let mut store = SeriesStore::new(50);
        let base = Utc::now();
        for i in 0..10 {
            store.record(
                "players",
                base + chrono::Duration::seconds(i),
                10.0 + i as f64 * 2.0,
            );
        }
        assert_eq!(store.names(), vec!["players"]);
        assert_eq!(store.trend("players"), Some("上升"));
        for i in 0..10 {
            store.record("flat", base + chrono::Duration::seconds(i), 5.0);
        }
        assert_eq!(store.trend("flat"), Some("平稳"));
        let removed = store.purge_before(base + chrono::Duration::seconds(5));
        assert_eq!(removed, 10);
        assert_eq!(store.get("players").unwrap().len(), 5);
    }

    #[test]
    fn delivery_signature_and_lifecycle() {
        let mut d = Delivery::new(
            "https://x/hook",
            "instance.crashed",
            "{\"a\":1}",
            "topsecret",
        );
        assert!(d.signature.starts_with("sha256="));
        assert!(d.verify("topsecret", "{\"a\":1}"));
        assert!(!d.verify("wrong", "{\"a\":1}"));
        assert!(d.due(Utc::now()));
        d.mark_delivered();
        assert_eq!(d.state, DeliveryState::Delivered);
        assert!(!d.due(Utc::now()));
    }

    #[test]
    fn delivery_retry_and_dead_letter() {
        let mut d = Delivery::new("https://x/hook", "e", "{}", "s");
        d.max_attempts = 3;
        d.mark_failed("timeout");
        assert_eq!(d.state, DeliveryState::Failed);
        assert!(d.last_error.as_deref() == Some("timeout"));
        d.mark_failed("timeout");
        assert_eq!(d.state, DeliveryState::Failed);
        d.mark_failed("timeout");
        assert_eq!(d.state, DeliveryState::Dead);
        assert!(!d.due(Utc::now()));
    }

    #[test]
    fn delivery_backoff_grows() {
        let mut d = Delivery::new("u", "e", "{}", "s");
        let first = d.backoff_secs();
        d.attempts = 3;
        assert!(d.backoff_secs() > first);
        d.attempts = 99;
        assert_eq!(d.backoff_secs(), 3600);
    }

    #[test]
    fn delivery_queue_operations() {
        let mut q = DeliveryQueue::new();
        q.enqueue(Delivery::new("u1", "e", "{}", "s"));
        q.enqueue(Delivery::new("u2", "e", "{}", "s"));
        assert_eq!(q.due(Utc::now()).len(), 2);
        let id = q.items[0].id.clone();
        assert!(q.get_mut(&id).is_some());
        q.get_mut(&id).unwrap().mark_delivered();
        assert_eq!(q.stats().get("已送达"), Some(&1));
        assert_eq!(q.due(Utc::now()).len(), 1);
    }

    #[test]
    fn notification_target_filtering() {
        let t = NotificationTarget::new("discord", "https://d/hook")
            .subscribe(&["instance.crashed", "backup.failed"]);
        assert!(t.wants("instance.crashed"));
        assert!(!t.wants("player.joined"));
        let mut off = NotificationTarget::new("discord", "u");
        off.enabled = false;
        assert!(!off.wants("anything"));
        let all = NotificationTarget::new("generic", "u");
        assert!(all.wants("anything"));
    }

    #[test]
    fn fanout_produces_signed_deliveries() {
        let targets = vec![
            NotificationTarget::new("discord", "https://d").subscribe(&["instance.crashed"]),
            NotificationTarget::new("slack", "https://s").subscribe(&["backup.done"]),
        ];
        let event = PanelEventOut::new("instance.crashed", "critical", "实例崩溃")
            .on("i1")
            .with_payload(serde_json::json!({"code": 1}));
        let out = fanout(&targets, &event);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "https://d");
        assert!(out[0].verify(&targets[0].secret, &event.body()));
        assert!(event.is_critical());
    }
}
