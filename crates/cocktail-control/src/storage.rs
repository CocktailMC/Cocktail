use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::crypto;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BackendKind {
    Local,
    S3,
    Webdav,
    Rclone,
}

impl BackendKind {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "local" | "file" | "fs" => BackendKind::Local,
            "s3" | "minio" | "b2" => BackendKind::S3,
            "webdav" | "dav" => BackendKind::Webdav,
            "rclone" => BackendKind::Rclone,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            BackendKind::Local => "本地磁盘",
            BackendKind::S3 => "S3 兼容对象存储",
            BackendKind::Webdav => "WebDAV",
            BackendKind::Rclone => "rclone 远端",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageConfig {
    pub kind: BackendKind,
    pub bucket: String,
    pub prefix: String,
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub remote: String,
    pub path_style: bool,
    pub encrypt: bool,
    pub verify_after_upload: bool,
    pub concurrency: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            kind: BackendKind::Local,
            bucket: String::new(),
            prefix: "cocktail".into(),
            endpoint: String::new(),
            region: "us-east-1".into(),
            access_key: String::new(),
            secret_key: String::new(),
            remote: String::new(),
            path_style: true,
            encrypt: true,
            verify_after_upload: true,
            concurrency: 4,
        }
    }
}

impl StorageConfig {
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        match self.kind {
            BackendKind::Local => {
                if self.prefix.trim().is_empty() {
                    issues.push("本地路径不能为空".into());
                }
            }
            BackendKind::S3 => {
                if self.bucket.trim().is_empty() {
                    issues.push("S3 必须指定 bucket".into());
                }
                if self.endpoint.trim().is_empty() {
                    issues.push("S3 必须指定 endpoint".into());
                }
                if self.access_key.trim().is_empty() {
                    issues.push("S3 必须提供 access key".into());
                }
                if self.secret_key.trim().is_empty() {
                    issues.push("S3 必须提供 secret key".into());
                }
            }
            BackendKind::Webdav => {
                if self.endpoint.trim().is_empty() {
                    issues.push("WebDAV 必须指定 endpoint".into());
                }
            }
            BackendKind::Rclone => {
                if self.remote.trim().is_empty() {
                    issues.push("rclone 必须指定 remote 名称".into());
                }
            }
        }
        if self.concurrency == 0 || self.concurrency > 64 {
            issues.push("并发数需在 1 到 64 之间".into());
        }
        issues
    }

    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        if !c.secret_key.is_empty() {
            c.secret_key = "****".into();
        }
        if !c.access_key.is_empty() {
            let n = c.access_key.len();
            let head = &c.access_key[..n.min(4)];
            c.access_key = format!("{head}****");
        }
        c
    }

    pub fn object_key(&self, instance_id: &str, name: &str) -> String {
        let prefix = self.prefix.trim_matches('/');
        format!("{prefix}/{instance_id}/{name}")
    }

    pub fn is_remote(&self) -> bool {
        self.kind != BackendKind::Local
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadResult {
    pub key: String,
    pub size: u64,
    pub checksum: String,
    pub encrypted: bool,
    pub backend: String,
    pub verified: bool,
}

pub fn upload_payload(
    config: &StorageConfig,
    aad: &str,
    data: &[u8],
) -> anyhow::Result<(Vec<u8>, String, bool)> {
    let checksum = crypto::sha256_hex(data);
    if config.encrypt {
        let sealed = crate::secrets::encrypt_bytes(aad, data);
        Ok((sealed, checksum, true))
    } else {
        Ok((data.to_vec(), checksum, false))
    }
}

pub fn download_payload(aad: &str, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if crate::secrets::is_encrypted_bytes(data) {
        crate::secrets::decrypt_bytes(aad, data).ok_or_else(|| anyhow::anyhow!("远端对象解密失败"))
    } else {
        Ok(data.to_vec())
    }
}

pub fn verify_upload(expected: &str, payload: &[u8]) -> bool {
    if crate::secrets::is_encrypted_bytes(payload) {
        return true;
    }
    crypto::sha256_hex(payload) == expected
}

pub fn local_root(config: &StorageConfig) -> PathBuf {
    PathBuf::from(&config.prefix)
}

pub fn local_write(config: &StorageConfig, key: &str, payload: &[u8]) -> anyhow::Result<PathBuf> {
    let root = local_root(config);
    let target = root.join(key);
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = target.with_extension("tmp");
    std::fs::write(&tmp, payload)?;
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

pub fn local_read(config: &StorageConfig, key: &str) -> anyhow::Result<Vec<u8>> {
    let path = local_root(config).join(key);
    Ok(std::fs::read(path)?)
}

pub fn local_list(config: &StorageConfig) -> anyhow::Result<Vec<(String, u64)>> {
    let root = local_root(config);
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(meta) = entry.metadata() {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, meta.len()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionMirror {
    pub keep_local_days: u64,
    pub keep_remote_days: u64,
    pub prune_local_after_upload: bool,
}

impl Default for RetentionMirror {
    fn default() -> Self {
        Self {
            keep_local_days: 7,
            keep_remote_days: 365,
            prune_local_after_upload: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum NodeState {
    Active,
    Draining,
    Maintenance,
    Offline,
}

impl NodeState {
    pub fn accepts_new_work(&self) -> bool {
        matches!(self, NodeState::Active)
    }

    pub fn label(&self) -> &'static str {
        match self {
            NodeState::Active => "在役",
            NodeState::Draining => "排空中",
            NodeState::Maintenance => "维护中",
            NodeState::Offline => "离线",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "active" | "online" => NodeState::Active,
            "draining" | "drain" => NodeState::Draining,
            "maintenance" | "maint" => NodeState::Maintenance,
            "offline" | "down" => NodeState::Offline,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeFacts {
    pub id: String,
    pub name: String,
    pub state: NodeState,
    pub labels: Vec<String>,
    pub cpu_pct: f32,
    pub mem_used_mib: f32,
    pub mem_total_mib: f32,
    pub disk_free_gib: f32,
    pub instance_count: usize,
    pub max_instances: usize,
    pub weight: f32,
}

impl NodeFacts {
    pub fn mem_pct(&self) -> f32 {
        if self.mem_total_mib <= 0.0 {
            return 0.0;
        }
        (self.mem_used_mib / self.mem_total_mib * 100.0).clamp(0.0, 100.0)
    }

    pub fn capacity_used(&self) -> f32 {
        if self.max_instances == 0 {
            return 1.0;
        }
        (self.instance_count as f32 / self.max_instances as f32).clamp(0.0, 1.0)
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    pub fn score(&self) -> f32 {
        let cpu = self.cpu_pct.clamp(0.0, 100.0) / 100.0;
        let mem = self.mem_pct() / 100.0;
        let cap = self.capacity_used();
        let disk = if self.disk_free_gib <= 0.0 { 1.0 } else { 0.0 };
        let raw = cpu * 0.35 + mem * 0.35 + cap * 0.25 + disk * 0.05;
        (raw * self.weight.max(0.1)).clamp(0.0, 2.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlacementRequest {
    pub required_labels: Vec<String>,
    pub avoid_labels: Vec<String>,
    pub need_mem_mib: f32,
    pub need_disk_gib: f32,
    pub pinned_node: Option<String>,
}

impl Default for PlacementRequest {
    fn default() -> Self {
        Self {
            required_labels: Vec::new(),
            avoid_labels: Vec::new(),
            need_mem_mib: 0.0,
            need_disk_gib: 0.0,
            pinned_node: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Placement {
    pub node_id: String,
    pub node_name: String,
    pub score: f32,
    pub reason: String,
}

pub fn eligible(facts: &[NodeFacts], req: &PlacementRequest) -> Vec<NodeFacts> {
    facts
        .iter()
        .filter(|n| n.state.accepts_new_work())
        .filter(|n| {
            if let Some(pin) = &req.pinned_node {
                return &n.id == pin;
            }
            true
        })
        .filter(|n| req.required_labels.iter().all(|l| n.has_label(l)))
        .filter(|n| !req.avoid_labels.iter().any(|l| n.has_label(l)))
        .filter(|n| n.max_instances == 0 || n.instance_count < n.max_instances)
        .filter(|n| {
            req.need_mem_mib <= 0.0 || (n.mem_total_mib - n.mem_used_mib) >= req.need_mem_mib
        })
        .filter(|n| req.need_disk_gib <= 0.0 || n.disk_free_gib >= req.need_disk_gib)
        .cloned()
        .collect()
}

pub fn best_placement(facts: &[NodeFacts], req: &PlacementRequest) -> Option<Placement> {
    let pool = eligible(facts, req);
    let mut scored: Vec<(f32, &NodeFacts)> = pool.iter().map(|n| (n.score(), n)).collect();
    scored.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.instance_count.cmp(&b.1.instance_count))
    });
    let (score, node) = scored.first()?;
    let reason = if req.pinned_node.is_some() {
        "指定节点".to_string()
    } else {
        format!(
            "负载最低 (cpu {:.0}% / mem {:.0}% / 实例 {}/{})",
            node.cpu_pct,
            node.mem_pct(),
            node.instance_count,
            node.max_instances
        )
    };
    Some(Placement {
        node_id: node.id.clone(),
        node_name: node.name.clone(),
        score: *score,
        reason,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub instance_id: String,
    pub from_node: String,
    pub to_node: String,
    pub steps: Vec<String>,
    pub estimated_bytes: u64,
    pub requires_downtime: bool,
    pub snapshot_id: Option<String>,
}

pub fn plan_migration(
    instance_id: &str,
    from: &str,
    to: &str,
    estimated_bytes: u64,
    snapshot_id: Option<String>,
) -> MigrationPlan {
    let mut steps = vec![
        format!("停止实例 {instance_id}"),
        "保存世界并等待刷盘".to_string(),
    ];
    if snapshot_id.is_some() {
        steps.push("创建迁移快照".to_string());
    }
    steps.push(format!("传输数据到节点 {to}"));
    steps.push("校验目标节点文件完整性".to_string());
    steps.push("在目标节点重建实例".to_string());
    steps.push("切换端口与路由".to_string());
    steps.push("启动实例并验证健康".to_string());
    steps.push(format!("清理源节点 {from} 上的旧副本"));
    MigrationPlan {
        instance_id: instance_id.to_string(),
        from_node: from.to_string(),
        to_node: to.to_string(),
        steps,
        estimated_bytes,
        requires_downtime: true,
        snapshot_id,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrainReport {
    pub node_id: String,
    pub moved: Vec<String>,
    pub remaining: Vec<String>,
    pub can_offline: bool,
}

pub fn plan_drain(node_id: &str, instances: &[String], target_capacity: usize) -> DrainReport {
    let movable = instances.len().min(target_capacity);
    let moved: Vec<String> = instances.iter().take(movable).cloned().collect();
    let remaining: Vec<String> = instances.iter().skip(movable).cloned().collect();
    DrainReport {
        node_id: node_id.to_string(),
        moved,
        remaining: remaining.clone(),
        can_offline: remaining.is_empty(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeHealth {
    pub node_id: String,
    pub last_seen_secs: u64,
    pub healthy: bool,
    pub action: String,
}

pub fn evaluate_health(
    node_id: &str,
    last_seen_secs: u64,
    warn_after: u64,
    offline_after: u64,
) -> NodeHealth {
    let (healthy, action) = if last_seen_secs >= offline_after {
        (false, "标记离线并暂停调度".to_string())
    } else if last_seen_secs >= warn_after {
        (true, "心跳延迟, 触发告警".to_string())
    } else {
        (true, "正常".to_string())
    };
    NodeHealth {
        node_id: node_id.to_string(),
        last_seen_secs,
        healthy,
        action,
    }
}

pub fn cluster_summary(facts: &[NodeFacts]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for f in facts {
        *out.entry(f.state.label().to_string()).or_insert(0) += 1;
    }
    out
}

pub fn disk_alert(free_gib: f32, total_gib: f32, threshold_pct: f32) -> bool {
    if total_gib <= 0.0 {
        return false;
    }
    (free_gib / total_gib * 100.0) < threshold_pct
}

pub fn instance_quota_ok(used: f32, limit: f32, incoming: f32) -> bool {
    if limit <= 0.0 {
        return true;
    }
    used + incoming <= limit
}

pub fn port_conflict(requested: u16, taken: &[u16]) -> bool {
    taken.contains(&requested)
}

pub fn next_free_port(start: u16, end: u16, taken: &[u16]) -> Option<u16> {
    (start..=end).find(|p| !taken.contains(p))
}

pub fn storage_path_exists(config: &StorageConfig, key: &str) -> bool {
    local_root(config).join(key).exists()
}

pub fn ensure_storage_root(config: &StorageConfig) -> anyhow::Result<()> {
    let root = local_root(config);
    std::fs::create_dir_all(&root)?;
    Ok(())
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", bytes, UNITS[idx])
    } else {
        format!("{:.1} {}", value, UNITS[idx])
    }
}

pub fn is_within(path: &Path, root: &Path) -> bool {
    let Ok(p) = path.canonicalize() else {
        return false;
    };
    let Ok(r) = root.canonicalize() else {
        return false;
    };
    p.starts_with(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, cpu: f32, used: f32, total: f32, count: usize, labels: &[&str]) -> NodeFacts {
        NodeFacts {
            id: id.into(),
            name: id.into(),
            state: NodeState::Active,
            labels: labels.iter().map(|l| l.to_string()).collect(),
            cpu_pct: cpu,
            mem_used_mib: used,
            mem_total_mib: total,
            disk_free_gib: 100.0,
            instance_count: count,
            max_instances: 10,
            weight: 1.0,
        }
    }

    #[test]
    fn backend_kind_parsing() {
        assert_eq!(BackendKind::parse("s3"), Some(BackendKind::S3));
        assert_eq!(BackendKind::parse("MinIO"), Some(BackendKind::S3));
        assert_eq!(BackendKind::parse("webdav"), Some(BackendKind::Webdav));
        assert_eq!(BackendKind::parse("local"), Some(BackendKind::Local));
        assert_eq!(BackendKind::parse("nope"), None);
    }

    #[test]
    fn config_validation_catches_missing_fields() {
        let mut c = StorageConfig {
            kind: BackendKind::S3,
            ..Default::default()
        };
        let issues = c.validate();
        assert!(issues.iter().any(|i| i.contains("bucket")));
        assert!(issues.iter().any(|i| i.contains("endpoint")));
        c.bucket = "b".into();
        c.endpoint = "http://x".into();
        c.access_key = "a".into();
        c.secret_key = "s".into();
        assert!(c.validate().is_empty());
        c.concurrency = 0;
        assert!(!c.validate().is_empty());
    }

    #[test]
    fn config_redaction_hides_secrets() {
        let c = StorageConfig {
            access_key: "AKIAEXAMPLE".into(),
            secret_key: "verysecret".into(),
            ..Default::default()
        };
        let r = c.redacted();
        assert_eq!(r.secret_key, "****");
        assert!(r.access_key.starts_with("AKIA"));
        assert!(!r.access_key.contains("EXAMPLE"));
    }

    #[test]
    fn object_key_layout() {
        let c = StorageConfig {
            prefix: "/cocktail/".into(),
            ..Default::default()
        };
        assert_eq!(c.object_key("inst1", "a.zip"), "cocktail/inst1/a.zip");
        assert!(!c.is_remote());
        let remote = StorageConfig {
            kind: BackendKind::S3,
            ..Default::default()
        };
        assert!(remote.is_remote());
    }

    #[test]
    fn upload_download_roundtrip_with_encryption() {
        let c = StorageConfig {
            encrypt: true,
            ..Default::default()
        };
        let data = b"backup payload".to_vec();
        let (sealed, checksum, encrypted) = upload_payload(&c, "bk:1", &data).unwrap();
        assert!(encrypted);
        assert!(verify_upload(&checksum, &sealed));
        let back = download_payload("bk:1", &sealed).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn upload_plaintext_verifies_checksum() {
        let c = StorageConfig {
            encrypt: false,
            ..Default::default()
        };
        let data = b"plain".to_vec();
        let (raw, checksum, encrypted) = upload_payload(&c, "bk:2", &data).unwrap();
        assert!(!encrypted);
        assert!(verify_upload(&checksum, &raw));
        assert!(!verify_upload("deadbeef", &raw));
        assert_eq!(download_payload("bk:2", &raw).unwrap(), data);
    }

    #[test]
    fn local_backend_write_read_list() {
        let dir = std::env::temp_dir().join(format!("ck-st-{}", crypto::random_token(8)));
        let c = StorageConfig {
            prefix: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        ensure_storage_root(&c).unwrap();
        local_write(&c, "a/b.bin", b"hello").unwrap();
        assert!(storage_path_exists(&c, "a/b.bin"));
        assert_eq!(local_read(&c, "a/b.bin").unwrap(), b"hello");
        let list = local_list(&c).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "a/b.bin");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn node_state_gates_scheduling() {
        assert!(NodeState::Active.accepts_new_work());
        assert!(!NodeState::Maintenance.accepts_new_work());
        assert!(!NodeState::Draining.accepts_new_work());
        assert!(!NodeState::Offline.accepts_new_work());
        assert_eq!(NodeState::parse("drain"), Some(NodeState::Draining));
        assert_eq!(NodeState::parse("bogus"), None);
    }

    #[test]
    fn placement_picks_least_loaded() {
        let facts = vec![
            node("busy", 90.0, 900.0, 1000.0, 8, &[]),
            node("idle", 5.0, 100.0, 1000.0, 1, &[]),
            node("mid", 40.0, 400.0, 1000.0, 4, &[]),
        ];
        let pick = best_placement(&facts, &PlacementRequest::default()).unwrap();
        assert_eq!(pick.node_id, "idle");
    }

    #[test]
    fn placement_respects_labels_and_memory() {
        let facts = vec![
            node("ssd1", 10.0, 100.0, 1000.0, 1, &["ssd"]),
            node("hdd1", 5.0, 100.0, 1000.0, 1, &["hdd"]),
        ];
        let req = PlacementRequest {
            required_labels: vec!["ssd".into()],
            ..Default::default()
        };
        assert_eq!(best_placement(&facts, &req).unwrap().node_id, "ssd1");
        let req2 = PlacementRequest {
            avoid_labels: vec!["ssd".into()],
            ..Default::default()
        };
        assert_eq!(best_placement(&facts, &req2).unwrap().node_id, "hdd1");
        let req3 = PlacementRequest {
            need_mem_mib: 5000.0,
            ..Default::default()
        };
        assert!(best_placement(&facts, &req3).is_none());
    }

    #[test]
    fn placement_honours_pin_and_state() {
        let mut facts = vec![
            node("a", 1.0, 1.0, 1000.0, 0, &[]),
            node("b", 99.0, 999.0, 1000.0, 9, &[]),
        ];
        let req = PlacementRequest {
            pinned_node: Some("b".into()),
            ..Default::default()
        };
        assert_eq!(best_placement(&facts, &req).unwrap().node_id, "b");
        facts[0].state = NodeState::Maintenance;
        let req2 = PlacementRequest::default();
        assert_eq!(best_placement(&facts, &req2).unwrap().node_id, "b");
        facts[1].state = NodeState::Offline;
        assert!(best_placement(&facts, &req2).is_none());
    }

    #[test]
    fn placement_skips_full_nodes() {
        let mut full = node("full", 1.0, 1.0, 1000.0, 10, &[]);
        full.max_instances = 10;
        let facts = vec![full];
        assert!(best_placement(&facts, &PlacementRequest::default()).is_none());
    }

    #[test]
    fn migration_plan_steps() {
        let plan = plan_migration("i1", "n1", "n2", 5_000_000_000, Some("snap1".into()));
        assert_eq!(plan.steps.len(), 9);
        assert!(plan.requires_downtime);
        assert!(plan.steps.iter().any(|s| s.contains("传输数据")));
        assert!(plan.steps.iter().any(|s| s.contains("创建迁移快照")));
        let plain = plan_migration("i1", "n1", "n2", 1, None);
        assert_eq!(plain.steps.len(), 8);
    }

    #[test]
    fn drain_plan_reports_progress() {
        let instances: Vec<String> = (0..5).map(|i| format!("i{i}")).collect();
        let report = plan_drain("n1", &instances, 3);
        assert_eq!(report.moved.len(), 3);
        assert_eq!(report.remaining.len(), 2);
        assert!(!report.can_offline);
        let full = plan_drain("n1", &instances, 10);
        assert!(full.can_offline);
    }

    #[test]
    fn health_thresholds() {
        let ok = evaluate_health("n1", 5, 60, 180);
        assert!(ok.healthy);
        let warn = evaluate_health("n1", 90, 60, 180);
        assert!(warn.healthy);
        assert!(warn.action.contains("告警"));
        let down = evaluate_health("n1", 500, 60, 180);
        assert!(!down.healthy);
        assert!(down.action.contains("离线"));
    }

    #[test]
    fn quota_and_disk_helpers() {
        assert!(instance_quota_ok(3.0, 10.0, 5.0));
        assert!(!instance_quota_ok(8.0, 10.0, 5.0));
        assert!(instance_quota_ok(999.0, 0.0, 1.0));
        assert!(disk_alert(5.0, 100.0, 10.0));
        assert!(!disk_alert(50.0, 100.0, 10.0));
        assert!(!disk_alert(1.0, 0.0, 10.0));
    }

    #[test]
    fn port_allocation() {
        assert!(port_conflict(25565, &[25565, 25566]));
        assert!(!port_conflict(25570, &[25565, 25566]));
        assert_eq!(next_free_port(25565, 25570, &[25565, 25566]), Some(25567));
        let all: Vec<u16> = (25565..=25567).collect();
        assert_eq!(next_free_port(25565, 25567, &all), None);
    }

    #[test]
    fn cluster_summary_counts_states() {
        let mut facts = vec![
            node("a", 1.0, 1.0, 100.0, 0, &[]),
            node("b", 1.0, 1.0, 100.0, 0, &[]),
        ];
        facts[1].state = NodeState::Maintenance;
        let summary = cluster_summary(&facts);
        assert_eq!(summary.get("在役"), Some(&1));
        assert_eq!(summary.get("维护中"), Some(&1));
    }

    #[test]
    fn human_readable_bytes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn retention_mirror_defaults() {
        let r = RetentionMirror::default();
        assert_eq!(r.keep_local_days, 7);
        assert!(r.prune_local_after_upload);
        assert!(r.keep_remote_days > r.keep_local_days);
    }
}
