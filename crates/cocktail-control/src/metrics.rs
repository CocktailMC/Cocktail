use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

fn f64_bits(v: f64) -> u64 {
    v.to_bits()
}

fn bits_f64(b: u64) -> f64 {
    f64::from_bits(b)
}

#[derive(Debug, Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    pub fn inc(&self) {
        self.inc_by(1.0);
    }

    pub fn inc_by(&self, delta: f64) {
        let mut cur = self.value.load(Ordering::Relaxed);
        loop {
            let next = f64_bits(bits_f64(cur) + delta);
            match self
                .value
                .compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }

    pub fn get(&self) -> f64 {
        bits_f64(self.value.load(Ordering::Relaxed))
    }
}

#[derive(Debug, Default)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    pub fn set(&self, v: f64) {
        self.value.store(f64_bits(v), Ordering::Relaxed);
    }

    pub fn get(&self) -> f64 {
        bits_f64(self.value.load(Ordering::Relaxed))
    }

    pub fn add(&self, delta: f64) {
        let mut cur = self.value.load(Ordering::Relaxed);
        loop {
            let next = f64_bits(bits_f64(cur) + delta);
            match self
                .value
                .compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return,
                Err(observed) => cur = observed,
            }
        }
    }
}

pub const DEFAULT_BUCKETS: [f64; 12] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

pub struct Histogram {
    bounds: Vec<f64>,
    counts: Vec<AtomicU64>,
    sum_bits: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    pub fn new(bounds: Vec<f64>) -> Self {
        let mut sorted = bounds;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        let mut counts = Vec::with_capacity(n + 1);
        for _ in 0..=n {
            counts.push(AtomicU64::new(0));
        }
        Self {
            bounds: sorted,
            counts,
            sum_bits: AtomicU64::new(f64_bits(0.0)),
            count: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, value: f64) {
        let mut idx = self.bounds.len();
        for (i, b) in self.bounds.iter().enumerate() {
            if value <= *b {
                idx = i;
                break;
            }
        }
        self.counts[idx].fetch_add(1, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut cur = self.sum_bits.load(Ordering::Relaxed);
        loop {
            let next = f64_bits(bits_f64(cur) + value);
            match self.sum_bits.compare_exchange_weak(
                cur,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => cur = observed,
            }
        }
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn sum(&self) -> f64 {
        bits_f64(self.sum_bits.load(Ordering::Relaxed))
    }

    pub fn buckets(&self) -> Vec<(f64, u64)> {
        let mut cumulative = 0u64;
        let mut out = Vec::new();
        for (i, b) in self.bounds.iter().enumerate() {
            cumulative += self.counts[i].load(Ordering::Relaxed);
            out.push((*b, cumulative));
        }
        cumulative += self.counts[self.bounds.len()].load(Ordering::Relaxed);
        out.push((f64::INFINITY, cumulative));
        out
    }

    pub fn quantile(&self, q: f64) -> f64 {
        let total = self.count();
        if total == 0 {
            return 0.0;
        }
        let target = (q.clamp(0.0, 1.0) * total as f64).ceil() as u64;
        for (bound, cum) in self.buckets() {
            if cum >= target {
                return if bound.is_infinite() {
                    self.bounds.last().copied().unwrap_or(0.0)
                } else {
                    bound
                };
            }
        }
        self.bounds.last().copied().unwrap_or(0.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricMeta {
    pub name: String,
    pub help: String,
    pub kind: String,
}

#[derive(Default)]
pub struct Registry {
    counters: RwLock<BTreeMap<String, Arc<Counter>>>,
    gauges: RwLock<BTreeMap<String, Arc<Gauge>>>,
    histograms: RwLock<BTreeMap<String, Arc<Histogram>>>,
    meta: RwLock<BTreeMap<String, MetricMeta>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    fn label_suffix(labels: &[(&str, &str)]) -> String {
        if labels.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();
        format!("{{{}}}", parts.join(","))
    }

    pub fn counter(&self, name: &str, help: &str, labels: &[(&str, &str)]) -> Arc<Counter> {
        let key = format!("{name}{}", Self::label_suffix(labels));
        if let Ok(g) = self.counters.read() {
            if let Some(c) = g.get(&key) {
                return c.clone();
            }
        }
        let c = Arc::new(Counter::default());
        if let Ok(mut g) = self.counters.write() {
            g.insert(key.clone(), c.clone());
        }
        if let Ok(mut m) = self.meta.write() {
            m.insert(
                key,
                MetricMeta {
                    name: name.to_string(),
                    help: help.to_string(),
                    kind: "counter".into(),
                },
            );
        }
        c
    }

    pub fn gauge(&self, name: &str, help: &str, labels: &[(&str, &str)]) -> Arc<Gauge> {
        let key = format!("{name}{}", Self::label_suffix(labels));
        if let Ok(g) = self.gauges.read() {
            if let Some(c) = g.get(&key) {
                return c.clone();
            }
        }
        let g = Arc::new(Gauge::default());
        if let Ok(mut guard) = self.gauges.write() {
            guard.insert(key.clone(), g.clone());
        }
        if let Ok(mut m) = self.meta.write() {
            m.insert(
                key,
                MetricMeta {
                    name: name.to_string(),
                    help: help.to_string(),
                    kind: "gauge".into(),
                },
            );
        }
        g
    }

    pub fn histogram(
        &self,
        name: &str,
        help: &str,
        labels: &[(&str, &str)],
        bounds: Vec<f64>,
    ) -> Arc<Histogram> {
        let key = format!("{name}{}", Self::label_suffix(labels));
        if let Ok(g) = self.histograms.read() {
            if let Some(c) = g.get(&key) {
                return c.clone();
            }
        }
        let h = Arc::new(Histogram::new(bounds));
        if let Ok(mut guard) = self.histograms.write() {
            guard.insert(key.clone(), h.clone());
        }
        if let Ok(mut m) = self.meta.write() {
            m.insert(
                key,
                MetricMeta {
                    name: name.to_string(),
                    help: help.to_string(),
                    kind: "histogram".into(),
                },
            );
        }
        h
    }

    pub fn observe_duration(&self, name: &str, help: &str, labels: &[(&str, &str)], d: Duration) {
        let h = self.histogram(name, help, labels, DEFAULT_BUCKETS.to_vec());
        h.observe(d.as_secs_f64());
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let meta = self.meta.read().map(|m| m.clone()).unwrap_or_default();
        if let Ok(counters) = self.counters.read() {
            let mut seen_help = BTreeMap::new();
            for (key, c) in counters.iter() {
                if let Some(m) = meta.get(key) {
                    seen_help
                        .entry(m.name.clone())
                        .or_insert_with(|| (m.help.clone(), "counter".to_string()));
                }
                out.push_str(&format!("{key} {}\n", c.get()));
            }
            for (name, (help, kind)) in seen_help {
                out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
            }
        }
        if let Ok(gauges) = self.gauges.read() {
            let mut seen_help = BTreeMap::new();
            let mut lines = String::new();
            for (key, g) in gauges.iter() {
                if let Some(m) = meta.get(key) {
                    seen_help
                        .entry(m.name.clone())
                        .or_insert_with(|| (m.help.clone(), "gauge".to_string()));
                }
                lines.push_str(&format!("{key} {}\n", g.get()));
            }
            for (name, (help, kind)) in seen_help {
                out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
            }
            out.push_str(&lines);
        }
        if let Ok(histograms) = self.histograms.read() {
            let mut seen_help = BTreeMap::new();
            let mut lines = String::new();
            for (key, h) in histograms.iter() {
                let (base, label_part) = match key.split_once('{') {
                    Some((b, rest)) => (b.to_string(), format!("{{{rest}")),
                    None => (key.clone(), String::new()),
                };
                if let Some(m) = meta.get(key) {
                    seen_help
                        .entry(m.name.clone())
                        .or_insert_with(|| (m.help.clone(), "histogram".to_string()));
                }
                for (bound, cum) in h.buckets() {
                    let le = if bound.is_infinite() {
                        "+Inf".to_string()
                    } else {
                        bound.to_string()
                    };
                    if label_part.is_empty() {
                        lines.push_str(&format!("{base}_bucket{{le=\"{le}\"}} {cum}\n"));
                    } else {
                        let inner = label_part.trim_start_matches('{').trim_end_matches('}');
                        lines.push_str(&format!("{base}_bucket{{{inner},le=\"{le}\"}} {cum}\n"));
                    }
                }
                if label_part.is_empty() {
                    lines.push_str(&format!("{base}_sum {}\n", h.sum()));
                    lines.push_str(&format!("{base}_count {}\n", h.count()));
                } else {
                    lines.push_str(&format!("{base}_sum{label_part} {}\n", h.sum()));
                    lines.push_str(&format!("{base}_count{label_part} {}\n", h.count()));
                }
            }
            for (name, (help, kind)) in seen_help {
                out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
            }
            out.push_str(&lines);
        }
        out
    }

    pub fn snapshot(&self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        if let Ok(counters) = self.counters.read() {
            for (k, c) in counters.iter() {
                out.push((k.clone(), c.get()));
            }
        }
        if let Ok(gauges) = self.gauges.read() {
            for (k, g) in gauges.iter() {
                out.push((k.clone(), g.get()));
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Comparator {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Ne,
}

impl Comparator {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim() {
            ">" | "gt" => Comparator::Gt,
            ">=" | "gte" => Comparator::Gte,
            "<" | "lt" => Comparator::Lt,
            "<=" | "lte" => Comparator::Lte,
            "==" | "eq" => Comparator::Eq,
            "!=" | "ne" => Comparator::Ne,
            _ => return None,
        })
    }

    pub fn test(&self, value: f64, threshold: f64) -> bool {
        match self {
            Comparator::Gt => value > threshold,
            Comparator::Gte => value >= threshold,
            Comparator::Lt => value < threshold,
            Comparator::Lte => value <= threshold,
            Comparator::Eq => (value - threshold).abs() < f64::EPSILON,
            Comparator::Ne => (value - threshold).abs() >= f64::EPSILON,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Comparator::Gt => ">",
            Comparator::Gte => ">=",
            Comparator::Lt => "<",
            Comparator::Lte => "<=",
            Comparator::Eq => "==",
            Comparator::Ne => "!=",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub metric: String,
    pub comparator: Comparator,
    pub threshold: f64,
    pub for_secs: u64,
    pub severity: String,
    pub enabled: bool,
    pub silence_secs: u64,
    pub inhibits: Vec<String>,
}

impl AlertRule {
    pub fn new(id: &str, metric: &str, comparator: Comparator, threshold: f64) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            metric: metric.to_string(),
            comparator,
            threshold,
            for_secs: 0,
            severity: "warning".into(),
            enabled: true,
            silence_secs: 300,
            inhibits: Vec::new(),
        }
    }

    pub fn with_for(mut self, secs: u64) -> Self {
        self.for_secs = secs;
        self
    }

    pub fn with_severity(mut self, sev: &str) -> Self {
        self.severity = sev.to_string();
        self
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertFiring {
    pub id: String,
    pub name: String,
    pub severity: String,
    pub metric: String,
    pub value: f64,
    pub threshold: f64,
    pub comparator: String,
    pub since_secs: u64,
}

#[derive(Debug, Default, Clone)]
struct RuleState {
    pending_since: Option<Instant>,
    firing: bool,
    silenced_until: Option<Instant>,
    last_value: f64,
}

pub struct AlertEngine {
    rules: RwLock<Vec<AlertRule>>,
    states: RwLock<BTreeMap<String, RuleState>>,
    history: RwLock<Vec<(String, String, i64)>>,
}

impl AlertEngine {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules: RwLock::new(rules),
            states: RwLock::new(BTreeMap::new()),
            history: RwLock::new(Vec::new()),
        }
    }

    pub fn set_rules(&self, rules: Vec<AlertRule>) {
        if let Ok(mut g) = self.rules.write() {
            *g = rules;
        }
    }

    pub fn rules(&self) -> Vec<AlertRule> {
        self.rules.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn silence(&self, id: &str, secs: u64) {
        if let Ok(mut g) = self.states.write() {
            let entry = g.entry(id.to_string()).or_default();
            entry.silenced_until = Some(Instant::now() + Duration::from_secs(secs));
        }
    }

    pub fn unsilence(&self, id: &str) {
        if let Ok(mut g) = self.states.write() {
            if let Some(entry) = g.get_mut(id) {
                entry.silenced_until = None;
            }
        }
    }

    pub fn history(&self) -> Vec<(String, String, i64)> {
        self.history.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn evaluate(&self, values: &BTreeMap<String, f64>) -> Vec<AlertFiring> {
        let rules = self.rules();
        let now = Instant::now();
        let mut firing = Vec::new();
        let mut inhibit_ids: Vec<String> = Vec::new();
        let mut states = match self.states.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        for rule in &rules {
            if !rule.enabled {
                continue;
            }
            let Some(value) = values.get(&rule.metric) else {
                continue;
            };
            let state = states.entry(rule.id.clone()).or_default();
            state.last_value = *value;
            let matched = rule.comparator.test(*value, rule.threshold);
            if matched {
                if state.pending_since.is_none() {
                    state.pending_since = Some(now);
                }
                let since = state.pending_since.unwrap_or(now);
                let held = now.duration_since(since).as_secs();
                if held >= rule.for_secs {
                    let silenced = state.silenced_until.map(|t| t > now).unwrap_or(false);
                    if !silenced {
                        state.firing = true;
                        firing.push(AlertFiring {
                            id: rule.id.clone(),
                            name: rule.name.clone(),
                            severity: rule.severity.clone(),
                            metric: rule.metric.clone(),
                            value: *value,
                            threshold: rule.threshold,
                            comparator: rule.comparator.symbol().to_string(),
                            since_secs: held,
                        });
                        if !rule.inhibits.is_empty() {
                            inhibit_ids.extend(rule.inhibits.iter().cloned());
                        }
                    }
                }
            } else {
                state.pending_since = None;
                state.firing = false;
            }
        }
        drop(states);
        if !inhibit_ids.is_empty() {
            firing.retain(|f| !inhibit_ids.contains(&f.id));
        }
        if let Ok(mut h) = self.history.write() {
            for f in &firing {
                h.push((f.id.clone(), f.severity.clone(), f.value as i64));
            }
            if h.len() > 500 {
                let excess = h.len() - 500;
                h.drain(0..excess);
            }
        }
        firing
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthReport {
    pub ok: bool,
    pub checks: Vec<CheckResult>,
}

impl HealthReport {
    pub fn from_checks(checks: Vec<CheckResult>) -> Self {
        let ok = checks.iter().all(|c| c.ok);
        Self { ok, checks }
    }

    pub fn status_code(&self) -> u16 {
        if self.ok { 200 } else { 503 }
    }

    pub fn summary(&self) -> String {
        let failed: Vec<&str> = self
            .checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name.as_str())
            .collect();
        if failed.is_empty() {
            "ok".to_string()
        } else {
            format!("failed: {}", failed.join(","))
        }
    }
}

pub fn default_rules() -> Vec<AlertRule> {
    vec![
        AlertRule::new(
            "cpu_sustained",
            "cocktail_node_cpu_pct",
            Comparator::Gt,
            90.0,
        )
        .with_for(300)
        .with_name("CPU 持续超过 90%"),
        AlertRule::new(
            "mem_sustained",
            "cocktail_node_mem_pct",
            Comparator::Gt,
            92.0,
        )
        .with_for(300)
        .with_name("内存持续超过 92%"),
        AlertRule::new(
            "backup_failed",
            "cocktail_backup_failures_total",
            Comparator::Gt,
            2.0,
        )
        .with_for(0)
        .with_severity("critical")
        .with_name("备份连续失败"),
        AlertRule::new(
            "players_drop",
            "cocktail_players_online",
            Comparator::Lt,
            1.0,
        )
        .with_for(60)
        .with_name("在线人数归零"),
        AlertRule::new(
            "api_error_rate",
            "cocktail_http_errors_ratio",
            Comparator::Gt,
            0.1,
        )
        .with_for(120)
        .with_severity("critical")
        .with_name("接口错误率过高"),
        AlertRule::new("disk_low", "cocktail_disk_free_pct", Comparator::Lt, 10.0)
            .with_for(60)
            .with_severity("critical")
            .with_name("磁盘剩余不足 10%"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_and_gauge_accumulate() {
        let r = Registry::new();
        let c = r.counter("hits_total", "hits", &[("route", "/a")]);
        c.inc();
        c.inc_by(2.0);
        assert_eq!(c.get(), 3.0);
        let g = r.gauge("temp", "temp", &[]);
        g.set(10.0);
        g.add(5.0);
        assert_eq!(g.get(), 15.0);
        let other = r.counter("hits_total", "hits", &[("route", "/b")]);
        other.inc();
        let text = r.render();
        assert!(text.contains("hits_total{route=\"/a\"} 3"));
        assert!(text.contains("hits_total{route=\"/b\"} 1"));
        assert!(text.contains("# TYPE hits_total counter"));
    }

    #[test]
    fn histogram_buckets_and_quantiles() {
        let h = Histogram::new(vec![1.0, 2.0, 3.0]);
        for v in [0.5, 1.5, 2.5, 9.0] {
            h.observe(v);
        }
        assert_eq!(h.count(), 4);
        assert!((h.sum() - 13.5).abs() < 1e-9);
        let buckets = h.buckets();
        assert_eq!(buckets[0], (1.0, 1));
        assert_eq!(buckets[1], (2.0, 2));
        assert_eq!(buckets[2], (3.0, 3));
        assert_eq!(buckets[3].1, 4);
        assert!(buckets[3].0.is_infinite());
        assert!((h.quantile(0.5) - 2.0).abs() < 1e-9);
        assert!((h.quantile(1.0) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn histogram_prometheus_format() {
        let r = Registry::new();
        let h = r.histogram("lat_seconds", "latency", &[("op", "get")], vec![0.1, 1.0]);
        h.observe(0.05);
        h.observe(0.5);
        let text = r.render();
        assert!(text.contains("lat_seconds_bucket{op=\"get\",le=\"0.1\"} 1"));
        assert!(text.contains("lat_seconds_bucket{op=\"get\",le=\"+Inf\"} 2"));
        assert!(text.contains("lat_seconds_count{op=\"get\"} 2"));
        assert!(text.contains("# TYPE lat_seconds histogram"));
    }

    #[test]
    fn comparator_semantics() {
        assert!(Comparator::parse(">").unwrap().test(5.0, 4.0));
        assert!(Comparator::parse("gte").unwrap().test(4.0, 4.0));
        assert!(Comparator::parse("<=").unwrap().test(4.0, 4.0));
        assert!(Comparator::parse("ne").unwrap().test(5.0, 4.0));
        assert!(Comparator::parse("?").is_none());
    }

    #[test]
    fn alert_for_duration_gating() {
        let engine = AlertEngine::new(vec![
            AlertRule::new("hot", "cpu", Comparator::Gt, 80.0).with_for(60),
        ]);
        let mut values = BTreeMap::new();
        values.insert("cpu".to_string(), 95.0);
        assert!(engine.evaluate(&values).is_empty());
        assert!(engine.evaluate(&values).is_empty());
        let instant = AlertEngine::new(vec![
            AlertRule::new("hot", "cpu", Comparator::Gt, 80.0).with_for(0),
        ]);
        let fired = instant.evaluate(&values);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].id, "hot");
        values.insert("cpu".to_string(), 10.0);
        assert!(instant.evaluate(&values).is_empty());
    }

    #[test]
    fn alert_silence_and_inhibition() {
        let engine = AlertEngine::new(vec![AlertRule::new("parent", "p", Comparator::Gt, 1.0), {
            let mut r = AlertRule::new("child", "c", Comparator::Gt, 1.0);
            r.inhibits = vec!["parent".to_string()];
            r
        }]);
        let mut values = BTreeMap::new();
        values.insert("p".to_string(), 5.0);
        values.insert("c".to_string(), 5.0);
        let fired = engine.evaluate(&values);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].id, "child");
        let solo = AlertEngine::new(vec![AlertRule::new("x", "v", Comparator::Gt, 1.0)]);
        let mut v = BTreeMap::new();
        v.insert("v".to_string(), 9.0);
        solo.silence("x", 300);
        assert!(solo.evaluate(&v).is_empty());
        solo.unsilence("x");
        assert_eq!(solo.evaluate(&v).len(), 1);
    }

    #[test]
    fn health_report_aggregates() {
        let ok = HealthReport::from_checks(vec![
            CheckResult {
                name: "db".into(),
                ok: true,
                detail: "writable".into(),
            },
            CheckResult {
                name: "disk".into(),
                ok: true,
                detail: "40% free".into(),
            },
        ]);
        assert!(ok.ok);
        assert_eq!(ok.status_code(), 200);
        assert_eq!(ok.summary(), "ok");
        let bad = HealthReport::from_checks(vec![
            CheckResult {
                name: "db".into(),
                ok: true,
                detail: "ok".into(),
            },
            CheckResult {
                name: "disk".into(),
                ok: false,
                detail: "full".into(),
            },
        ]);
        assert!(!bad.ok);
        assert_eq!(bad.status_code(), 503);
        assert!(bad.summary().contains("disk"));
    }

    #[test]
    fn default_rules_are_sane() {
        let rules = default_rules();
        assert!(rules.len() >= 5);
        assert!(rules.iter().any(|r| r.severity == "critical"));
        assert!(rules.iter().all(|r| !r.metric.is_empty()));
    }
}
