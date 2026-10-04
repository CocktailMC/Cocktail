use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::crypto;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum StepKind {
    Start,
    Stop,
    Restart,
    Backup,
    Command,
    Wait,
    Webhook,
    Snapshot,
    Prune,
    Notify,
}

impl StepKind {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "start" => StepKind::Start,
            "stop" => StepKind::Stop,
            "restart" => StepKind::Restart,
            "backup" => StepKind::Backup,
            "command" | "rcon" => StepKind::Command,
            "wait" | "sleep" => StepKind::Wait,
            "webhook" | "http" => StepKind::Webhook,
            "snapshot" => StepKind::Snapshot,
            "prune" | "cleanup" => StepKind::Prune,
            "notify" | "alert" => StepKind::Notify,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            StepKind::Start => "启动",
            StepKind::Stop => "停止",
            StepKind::Restart => "重启",
            StepKind::Backup => "备份",
            StepKind::Command => "执行命令",
            StepKind::Wait => "等待",
            StepKind::Webhook => "调用 Webhook",
            StepKind::Snapshot => "创建快照",
            StepKind::Prune => "清理旧备份",
            StepKind::Notify => "发送通知",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub delay_secs: u64,
    pub backoff: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_secs: 5,
            backoff: 2.0,
        }
    }
}

impl RetryPolicy {
    pub fn delay_for(&self, attempt: u32) -> u64 {
        if attempt == 0 {
            return 0;
        }
        let factor = self.backoff.powi(attempt as i32 - 1);
        ((self.delay_secs as f64) * factor).min(3600.0) as u64
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub name: String,
    pub kind: StepKind,
    pub instance_id: Option<String>,
    pub params: BTreeMap<String, String>,
    pub depends_on: Vec<String>,
    pub on_failure: Option<String>,
    pub retry: RetryPolicy,
    pub timeout_secs: u64,
    pub condition: Option<String>,
}

impl Step {
    pub fn new(id: &str, kind: StepKind) -> Self {
        Self {
            id: id.to_string(),
            name: kind.label().to_string(),
            kind,
            instance_id: None,
            params: BTreeMap::new(),
            depends_on: Vec::new(),
            on_failure: None,
            retry: RetryPolicy::default(),
            timeout_secs: 300,
            condition: None,
        }
    }

    pub fn after(mut self, dep: &str) -> Self {
        self.depends_on.push(dep.to_string());
        self
    }

    pub fn on(mut self, instance: &str) -> Self {
        self.instance_id = Some(instance.to_string());
        self
    }

    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<Step>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum StepState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    TimedOut,
}

impl StepState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            StepState::Succeeded | StepState::Failed | StepState::Skipped | StepState::TimedOut
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            StepState::Pending => "等待",
            StepState::Running => "执行中",
            StepState::Succeeded => "成功",
            StepState::Failed => "失败",
            StepState::Skipped => "跳过",
            StepState::TimedOut => "超时",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepRun {
    pub step_id: String,
    pub state: StepState,
    pub attempts: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub output: String,
    pub error: Option<String>,
}

impl StepRun {
    pub fn new(step_id: &str) -> Self {
        Self {
            step_id: step_id.to_string(),
            state: StepState::Pending,
            attempts: 0,
            started_at: None,
            finished_at: None,
            output: String::new(),
            error: None,
        }
    }

    pub fn duration_secs(&self) -> Option<i64> {
        match (self.started_at, self.finished_at) {
            (Some(a), Some(b)) => Some((b - a).num_seconds()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub steps: BTreeMap<String, StepRun>,
    pub trigger: String,
    pub success: Option<bool>,
}

impl WorkflowRun {
    pub fn new(workflow: &Workflow, trigger: &str) -> Self {
        let mut steps = BTreeMap::new();
        for s in &workflow.steps {
            steps.insert(s.id.clone(), StepRun::new(&s.id));
        }
        Self {
            id: crypto::random_token(16),
            workflow_id: workflow.id.clone(),
            started_at: Utc::now(),
            finished_at: None,
            steps,
            trigger: trigger.to_string(),
            success: None,
        }
    }

    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .steps
            .values()
            .filter(|s| s.state.is_terminal())
            .count();
        (done, self.steps.len())
    }

    pub fn failed_steps(&self) -> Vec<&str> {
        self.steps
            .values()
            .filter(|s| matches!(s.state, StepState::Failed | StepState::TimedOut))
            .map(|s| s.step_id.as_str())
            .collect()
    }
}

pub fn validate(workflow: &Workflow) -> Vec<String> {
    let mut issues = Vec::new();
    if workflow.steps.is_empty() {
        issues.push("工作流至少需要一个步骤".into());
    }
    let ids: BTreeSet<&str> = workflow.steps.iter().map(|s| s.id.as_str()).collect();
    if ids.len() != workflow.steps.len() {
        issues.push("步骤 ID 重复".into());
    }
    for step in &workflow.steps {
        for dep in &step.depends_on {
            if !ids.contains(dep.as_str()) {
                issues.push(format!("步骤 {} 依赖不存在的 {dep}", step.id));
            }
            if dep == &step.id {
                issues.push(format!("步骤 {} 依赖自身", step.id));
            }
        }
        if let Some(fb) = &step.on_failure {
            if !ids.contains(fb.as_str()) {
                issues.push(format!("步骤 {} 的失败分支不存在", step.id));
            }
        }
    }
    if has_cycle(workflow) {
        issues.push("步骤依赖存在环".into());
    }
    issues
}

pub fn has_cycle(workflow: &Workflow) -> bool {
    let mut indegree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for step in &workflow.steps {
        indegree.entry(step.id.as_str()).or_insert(0);
        for dep in &step.depends_on {
            adj.entry(dep.as_str()).or_default().push(step.id.as_str());
            *indegree.entry(step.id.as_str()).or_insert(0) += 1;
        }
    }
    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut seen = 0usize;
    while let Some(node) = queue.pop_front() {
        seen += 1;
        if let Some(next) = adj.get(node) {
            for n in next {
                if let Some(d) = indegree.get_mut(n) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push_back(n);
                    }
                }
            }
        }
    }
    seen != workflow.steps.len()
}

pub fn execution_order(workflow: &Workflow) -> Vec<Vec<String>> {
    let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
    let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for step in &workflow.steps {
        indegree.entry(step.id.clone()).or_insert(0);
        for dep in &step.depends_on {
            adj.entry(dep.clone()).or_default().push(step.id.clone());
            *indegree.entry(step.id.clone()).or_insert(0) += 1;
        }
    }
    let mut layers = Vec::new();
    let mut current: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let mut placed = 0usize;
    while !current.is_empty() {
        current.sort();
        layers.push(current.clone());
        placed += current.len();
        let mut next = Vec::new();
        for node in &current {
            if let Some(children) = adj.get(node) {
                for c in children {
                    if let Some(d) = indegree.get_mut(c) {
                        *d = d.saturating_sub(1);
                        if *d == 0 {
                            next.push(c.clone());
                        }
                    }
                }
            }
        }
        current = next;
        if placed > workflow.steps.len() {
            break;
        }
    }
    layers
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CronSpec {
    pub minute: Vec<u32>,
    pub hour: Vec<u32>,
    pub day: Vec<u32>,
    pub month: Vec<u32>,
    pub weekday: Vec<u32>,
}

impl CronSpec {
    pub fn parse(expr: &str) -> Option<Self> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return None;
        }
        let minute = parse_field(parts[0], 0, 59)?;
        let hour = parse_field(parts[1], 0, 23)?;
        let day = parse_field(parts[2], 1, 31)?;
        let month = parse_field(parts[3], 1, 12)?;
        let weekday = parse_field(parts[4], 0, 6)?;
        Some(Self {
            minute,
            hour,
            day,
            month,
            weekday,
        })
    }

    pub fn matches(&self, at: DateTime<Utc>) -> bool {
        self.minute.contains(&at.minute())
            && self.hour.contains(&at.hour())
            && self.day.contains(&at.day())
            && self.month.contains(&at.month())
            && self.weekday.contains(&at.weekday().num_days_from_sunday())
    }

    pub fn next_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut cursor = from + chrono::Duration::minutes(1);
        cursor = cursor.with_second(0)?.with_nanosecond(0)?;
        for _ in 0..(60 * 24 * 366) {
            if self.matches(cursor) {
                return Some(cursor);
            }
            cursor += chrono::Duration::minutes(1);
        }
        None
    }

    pub fn describe(&self) -> String {
        if self.minute.len() == 1 && self.hour.len() == 1 {
            let d = if self.day.len() == 1 {
                format!("{} 日", self.day[0])
            } else {
                "每天".to_string()
            };
            format!("{} {:02}:{:02}", d, self.hour[0], self.minute[0])
        } else if self.minute.len() == 1 {
            format!("每小时第 {} 分", self.minute[0])
        } else {
            format!("每小时 {} 次", self.minute.len())
        }
    }
}

fn parse_field(raw: &str, min: u32, max: u32) -> Option<Vec<u32>> {
    let mut out = BTreeSet::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        if part == "*" {
            for v in min..=max {
                out.insert(v);
            }
            continue;
        }
        if let Some((step_part, step_raw)) = part.split_once('/') {
            let step: u32 = step_raw.parse().ok()?;
            if step == 0 {
                return None;
            }
            let (start, end) = if step_part == "*" {
                (min, max)
            } else if let Some((a, b)) = step_part.split_once('-') {
                (a.parse().ok()?, b.parse().ok()?)
            } else {
                let v: u32 = step_part.parse().ok()?;
                (v, max)
            };
            let mut v = start;
            while v <= end {
                if v >= min && v <= max {
                    out.insert(v);
                }
                v += step;
            }
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let start: u32 = a.parse().ok()?;
            let end: u32 = b.parse().ok()?;
            if start > end {
                return None;
            }
            for v in start..=end {
                if v >= min && v <= max {
                    out.insert(v);
                }
            }
            continue;
        }
        let v: u32 = part.parse().ok()?;
        if v < min || v > max {
            return None;
        }
        out.insert(v);
    }
    if out.is_empty() {
        None
    } else {
        Some(out.into_iter().collect())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub action: String,
    pub actor: String,
    pub instance_id: Option<String>,
    pub detail: String,
    pub prev_hash: String,
    pub hash: String,
}

pub fn audit_hash(entry: &AuditEntry) -> String {
    let mut buf = String::new();
    buf.push_str(&entry.seq.to_string());
    buf.push('|');
    buf.push_str(&entry.at.to_rfc3339());
    buf.push('|');
    buf.push_str(&entry.action);
    buf.push('|');
    buf.push_str(&entry.actor);
    buf.push('|');
    buf.push_str(entry.instance_id.as_deref().unwrap_or(""));
    buf.push('|');
    buf.push_str(&entry.detail);
    buf.push('|');
    buf.push_str(&entry.prev_hash);
    crypto::sha256_hex(buf.as_bytes())
}

pub fn append_audit(
    chain: &mut Vec<AuditEntry>,
    action: &str,
    actor: &str,
    instance_id: Option<&str>,
    detail: &str,
) -> AuditEntry {
    let prev_hash = chain.last().map(|e| e.hash.clone()).unwrap_or_else(|| "genesis".into());
    let mut entry = AuditEntry {
        seq: chain.len() as u64 + 1,
        at: Utc::now(),
        action: action.to_string(),
        actor: actor.to_string(),
        instance_id: instance_id.map(|s| s.to_string()),
        detail: detail.to_string(),
        prev_hash,
        hash: String::new(),
    };
    entry.hash = audit_hash(&entry);
    chain.push(entry.clone());
    entry
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainVerification {
    pub total: usize,
    pub valid: bool,
    pub first_bad_seq: Option<u64>,
    pub reason: String,
}

pub fn verify_audit_chain(chain: &[AuditEntry]) -> ChainVerification {
    let mut prev = "genesis".to_string();
    for (i, entry) in chain.iter().enumerate() {
        if entry.seq != i as u64 + 1 {
            return ChainVerification {
                total: chain.len(),
                valid: false,
                first_bad_seq: Some(entry.seq),
                reason: format!("序号断裂, 期望 {} 实际 {}", i + 1, entry.seq),
            };
        }
        if entry.prev_hash != prev {
            return ChainVerification {
                total: chain.len(),
                valid: false,
                first_bad_seq: Some(entry.seq),
                reason: "前序摘要不匹配".into(),
            };
        }
        let expect = audit_hash(entry);
        if expect != entry.hash {
            return ChainVerification {
                total: chain.len(),
                valid: false,
                first_bad_seq: Some(entry.seq),
                reason: "记录内容被篡改".into(),
            };
        }
        prev = entry.hash.clone();
    }
    ChainVerification {
        total: chain.len(),
        valid: true,
        first_bad_seq: None,
        reason: "完整".into(),
    }
}

pub fn detect_tampering(chain: &[AuditEntry], seq: u64) -> Option<usize> {
    chain.iter().position(|e| e.seq == seq)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebhookTrigger {
    pub id: String,
    pub workflow_id: String,
    pub secret: String,
    pub enabled: bool,
}

impl WebhookTrigger {
    pub fn new(workflow_id: &str) -> Self {
        Self {
            id: crypto::random_token(16),
            workflow_id: workflow_id.to_string(),
            secret: crypto::random_token(32),
            enabled: true,
        }
    }

    pub fn sign(&self, body: &str) -> String {
        let mac = crypto::hmac_sha256(self.secret.as_bytes(), body.as_bytes());
        format!("sha256={}", crypto::hex(&mac))
    }

    pub fn verify(&self, body: &str, signature: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let expect = self.sign(body);
        crypto::ct_eq(expect.as_bytes(), signature.as_bytes())
    }
}

pub fn should_run_cron(spec: &CronSpec, last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    if !spec.matches(now) {
        return false;
    }
    match last {
        Some(t) => (now - t).num_minutes() >= 1,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_workflow() -> Workflow {
        Workflow {
            id: "wf1".into(),
            name: "每日维护".into(),
            description: String::new(),
            steps: vec![
                Step::new("save", StepKind::Command).on("i1").with("command", "save-all"),
                Step::new("backup", StepKind::Backup).on("i1").after("save"),
                Step::new("restart", StepKind::Restart).on("i1").after("backup"),
            ],
            enabled: true,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn step_kind_parsing() {
        assert_eq!(StepKind::parse("backup"), Some(StepKind::Backup));
        assert_eq!(StepKind::parse("RCON"), Some(StepKind::Command));
        assert_eq!(StepKind::parse("nope"), None);
    }

    #[test]
    fn retry_backoff() {
        let p = RetryPolicy {
            max_attempts: 4,
            delay_secs: 2,
            backoff: 2.0,
        };
        assert_eq!(p.delay_for(0), 0);
        assert_eq!(p.delay_for(1), 2);
        assert_eq!(p.delay_for(2), 4);
        assert_eq!(p.delay_for(3), 8);
    }

    #[test]
    fn validation_detects_missing_dep() {
        let mut wf = sample_workflow();
        wf.steps[1].depends_on.push("ghost".into());
        let issues = validate(&wf);
        assert!(issues.iter().any(|i| i.contains("ghost")));
    }

    #[test]
    fn validation_detects_cycle() {
        let mut wf = sample_workflow();
        wf.steps[0].depends_on.push("restart".into());
        assert!(has_cycle(&wf));
        assert!(validate(&wf).iter().any(|i| i.contains("环")));
    }

    #[test]
    fn validation_clean_workflow() {
        let wf = sample_workflow();
        assert!(validate(&wf).is_empty());
        assert!(!has_cycle(&wf));
    }

    #[test]
    fn execution_order_layers() {
        let wf = sample_workflow();
        let layers = execution_order(&wf);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0], vec!["save"]);
        assert_eq!(layers[2], vec!["restart"]);
    }

    #[test]
    fn parallel_steps_same_layer() {
        let wf = Workflow {
            id: "wf2".into(),
            name: "并行".into(),
            description: String::new(),
            steps: vec![
                Step::new("a", StepKind::Backup),
                Step::new("b", StepKind::Backup),
                Step::new("c", StepKind::Prune).after("a").after("b"),
            ],
            enabled: true,
            created_at: Utc::now(),
        };
        let layers = execution_order(&wf);
        assert_eq!(layers[0], vec!["a", "b"]);
        assert_eq!(layers[1], vec!["c"]);
    }

    #[test]
    fn run_tracks_progress() {
        let wf = sample_workflow();
        let mut run = WorkflowRun::new(&wf, "manual");
        assert_eq!(run.progress(), (0, 3));
        run.steps.get_mut("save").unwrap().state = StepState::Succeeded;
        run.steps.get_mut("backup").unwrap().state = StepState::Failed;
        assert_eq!(run.progress(), (2, 3));
        assert_eq!(run.failed_steps(), vec!["backup"]);
    }

    #[test]
    fn step_run_duration() {
        let mut sr = StepRun::new("s");
        assert!(sr.duration_secs().is_none());
        let now = Utc::now();
        sr.started_at = Some(now);
        sr.finished_at = Some(now + chrono::Duration::seconds(42));
        assert_eq!(sr.duration_secs(), Some(42));
    }

    #[test]
    fn cron_parsing_basics() {
        let c = CronSpec::parse("0 4 * * *").unwrap();
        assert_eq!(c.minute, vec![0]);
        assert_eq!(c.hour, vec![4]);
        assert_eq!(c.day.len(), 31);
        assert!(CronSpec::parse("bad").is_none());
        assert!(CronSpec::parse("99 * * * *").is_none());
    }

    #[test]
    fn cron_matching_and_next() {
        let c = CronSpec::parse("30 3 * * *").unwrap();
        let base = chrono::DateTime::parse_from_rfc3339("2026-03-10T03:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(c.matches(base));
        assert!(!c.matches(base + chrono::Duration::minutes(1)));
        let next = c.next_after(base).unwrap();
        assert_eq!(next, base + chrono::Duration::days(1));
        assert!(c.describe().contains("03:30"));
    }

    #[test]
    fn cron_ranges_and_steps() {
        let c = CronSpec::parse("*/15 * * * *").unwrap();
        assert_eq!(c.minute, vec![0, 15, 30, 45]);
        let r = CronSpec::parse("0 9-17 * * 1-5").unwrap();
        assert_eq!(r.hour.first(), Some(&9));
        assert_eq!(r.hour.last(), Some(&17));
        assert_eq!(r.weekday, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn cron_should_run_guards_duplicates() {
        let c = CronSpec::parse("0 4 * * *").unwrap();
        let at = chrono::DateTime::parse_from_rfc3339("2026-03-10T04:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(should_run_cron(&c, None, at));
        assert!(!should_run_cron(&c, Some(at), at));
        assert!(should_run_cron(&c, Some(at - chrono::Duration::minutes(5)), at));
    }

    #[test]
    fn audit_chain_appends_and_verifies() {
        let mut chain = Vec::new();
        append_audit(&mut chain, "instance.start", "root", Some("i1"), "{}");
        append_audit(&mut chain, "player.kick", "support", Some("i1"), "{\"p\":\"x\"}");
        append_audit(&mut chain, "settings.update", "root", None, "{}");
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].prev_hash, "genesis");
        assert_eq!(chain[1].prev_hash, chain[0].hash);
        let v = verify_audit_chain(&chain);
        assert!(v.valid);
        assert_eq!(v.total, 3);
    }

    #[test]
    fn audit_chain_detects_tampering() {
        let mut chain = Vec::new();
        append_audit(&mut chain, "a", "root", None, "{}");
        append_audit(&mut chain, "b", "root", None, "{}");
        chain[0].detail = "tampered".into();
        let v = verify_audit_chain(&chain);
        assert!(!v.valid);
        assert_eq!(v.first_bad_seq, Some(1));
        assert!(v.reason.contains("篡改"));
    }

    #[test]
    fn audit_chain_detects_removal() {
        let mut chain = Vec::new();
        append_audit(&mut chain, "a", "root", None, "{}");
        append_audit(&mut chain, "b", "root", None, "{}");
        chain.remove(0);
        let v = verify_audit_chain(&chain);
        assert!(!v.valid);
        assert!(v.reason.contains("序号断裂"));
    }

    #[test]
    fn webhook_signature_verification() {
        let t = WebhookTrigger::new("wf1");
        let body = "{\"event\":\"ping\"}";
        let sig = t.sign(body);
        assert!(sig.starts_with("sha256="));
        assert!(t.verify(body, &sig));
        assert!(!t.verify("{\"event\":\"other\"}", &sig));
        let mut disabled = WebhookTrigger::new("wf1");
        disabled.enabled = false;
        assert!(!disabled.verify(body, &disabled.sign(body)));
    }

    #[test]
    fn step_state_terminal() {
        assert!(StepState::Succeeded.is_terminal());
        assert!(StepState::Failed.is_terminal());
        assert!(StepState::TimedOut.is_terminal());
        assert!(!StepState::Running.is_terminal());
        assert!(!StepState::Pending.is_terminal());
    }
}
