use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Created,
    Starting,
    Running,
    Stopping,
    Stopped,
    Crashed,
}

impl InstanceStatus {
    /// Whether an async `StatusChanged` may overwrite `current`.
    /// Stale in-flight events must not resurrect Stopping/Starting after the process is gone.
    pub fn can_apply_over(self, current: Self) -> bool {
        use InstanceStatus::*;
        if self == current {
            return true;
        }
        match (current, self) {
            (Stopped | Crashed | Created, Stopping) => false,
            (Running, Starting) => false,
            (Stopping, Running | Starting) => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    #[default]
    Process,
    Docker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSpec {
    pub name: String,
    pub workdir: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_memory_mib")]
    pub memory_mib: u32,
    #[serde(default = "default_core")]
    pub core: String,
    /// Bound into server.properties as server-port.
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub auto_restart: bool,
    #[serde(default)]
    pub eula_accepted: bool,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub runtime: RuntimeKind,
    /// Docker image when runtime=docker (default eclipse-temurin:21-jre).
    #[serde(default)]
    pub docker_image: Option<String>,
    /// Docker --cpus soft limit.
    #[serde(default)]
    pub cpu_limit: Option<f32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default = "default_node_id")]
    pub node_id: String,
    #[serde(default)]
    pub desired_running: bool,
    #[serde(default = "default_backup_keep")]
    pub backup_keep: u32,
    #[serde(default)]
    pub backup_hour: Option<u8>,
    /// Explicit Temurin/Java major (8/17/21/…). None = pick from mc_version or 21.
    #[serde(default)]
    pub java_major: Option<u32>,
    /// Minecraft version last installed for this instance (used to pick Java).
    #[serde(default)]
    pub mc_version: Option<String>,
}

pub fn is_local_node(node_id: &str) -> bool {
    node_id.is_empty() || node_id == "local"
}

fn default_backup_keep() -> u32 {
    7
}

fn default_node_id() -> String {
    "local".into()
}

fn default_memory_mib() -> u32 {
    1024
}

fn default_core() -> String {
    "demo".into()
}

fn default_port() -> u16 {
    25565
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetPeer {
    pub ip: String,
    pub connections: u32,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub ipv6: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub ts: DateTime<Utc>,
    pub cpu_pct: f32,
    pub memory_mib: f32,
    pub tps: Option<f32>,
    pub players: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mspt: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players_max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entities: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunks: Option<u32>,
    #[serde(default)]
    pub gc_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heap_used_mib: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heap_max_mib: Option<f32>,
    #[serde(default)]
    pub net_rx_bps: f32,
    #[serde(default)]
    pub net_tx_bps: f32,
    #[serde(default)]
    pub net_connections: u32,
    #[serde(default)]
    pub net_unique_ips: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_listen: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_peers: Vec<NetPeer>,
    #[serde(default)]
    pub net_syn_recv: u32,
    #[serde(default)]
    pub net_time_wait: u32,
    #[serde(default)]
    pub net_fin_wait: u32,
    #[serde(default)]
    pub net_udp: u32,
    #[serde(default)]
    pub net_rx_pps: f32,
    #[serde(default)]
    pub net_tx_pps: f32,
    #[serde(default)]
    pub net_rx_bytes: u64,
    #[serde(default)]
    pub net_tx_bytes: u64,
    #[serde(default)]
    pub net_session_rx: u64,
    #[serde(default)]
    pub net_session_tx: u64,
    #[serde(default)]
    pub net_peak_rx_bps: f32,
    #[serde(default)]
    pub net_peak_tx_bps: f32,
    #[serde(default)]
    pub net_drops: u64,
    #[serde(default)]
    pub net_errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_rtt_ms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_ping_online: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_ping_max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_ping_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_alerts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub ts: DateTime<Utc>,
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstanceEvent {
    StatusChanged {
        instance_id: String,
        status: InstanceStatus,
        at: DateTime<Utc>,
    },
    Log {
        instance_id: String,
        line: LogLine,
    },
    Metric {
        instance_id: String,
        sample: MetricSample,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub spec: InstanceSpec,
    pub status: InstanceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_metrics: Option<MetricSample>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_players: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_start_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker_container: Option<String>,
    #[serde(default)]
    pub generation: u64,
    #[serde(skip)]
    pub(crate) process: Option<crate::instance::ProcessHandle>,
}

impl Instance {
    pub fn new(spec: InstanceSpec) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            spec,
            status: InstanceStatus::Created,
            created_at: now,
            updated_at: now,
            last_metrics: None,
            last_players: Vec::new(),
            last_pid: None,
            last_start_time: None,
            docker_container: None,
            generation: 1,
            process: None,
        }
    }

    pub fn public_view(&self) -> InstanceView {
        let m = self.last_metrics.as_ref();
        let status = match self.status {
            InstanceStatus::Running => "running",
            InstanceStatus::Crashed => "crashed",
            _ => "other",
        };
        let report = crate::util::health_report(
            status,
            m.and_then(|x| x.tps),
            m.and_then(|x| x.mspt),
            m.map(|x| x.memory_mib).unwrap_or(0.0),
            self.spec.memory_mib as f32,
            m.map(|x| x.net_alerts.len()).unwrap_or(0),
        );
        InstanceView {
            id: self.id.clone(),
            spec: self.spec.clone(),
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_metrics: self.last_metrics.clone(),
            last_players: self.last_players.clone(),
            pid: self
                .process
                .as_ref()
                .map(|p| p.child_id)
                .filter(|p| *p > 0)
                .or(self.last_pid),
            reattached: self
                .process
                .as_ref()
                .is_some_and(|p| p.reattached),
            node_id: if self.spec.node_id.is_empty() {
                "local".into()
            } else {
                self.spec.node_id.clone()
            },
            desired_running: self.spec.desired_running,
            generation: self.generation,
            docker_container: self.docker_container.clone(),
            health_score: report.0,
            health_reasons: report.1,
        }
    }

    pub fn persist_snapshot(&self) -> Self {
        Self {
            id: self.id.clone(),
            spec: self.spec.clone(),
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_metrics: None,
            last_players: Vec::new(),
            last_pid: self.last_pid,
            last_start_time: self.last_start_time,
            docker_container: self.docker_container.clone(),
            generation: self.generation,
            process: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceView {
    pub id: String,
    pub spec: InstanceSpec,
    pub status: InstanceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_metrics: Option<MetricSample>,
    #[serde(default)]
    pub last_players: Vec<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub reattached: bool,
    #[serde(default = "default_view_node")]
    pub node_id: String,
    #[serde(default)]
    pub desired_running: bool,
    #[serde(default)]
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker_container: Option<String>,
    #[serde(default)]
    pub health_score: u8,
    #[serde(default)]
    pub health_reasons: Vec<String>,
}

fn default_view_node() -> String {
    "local".into()
}

#[derive(Debug, Deserialize)]
pub struct CreateInstanceRequest {
    pub name: String,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_memory_mib")]
    pub memory_mib: u32,
    #[serde(default = "default_core")]
    pub core: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub auto_restart: bool,
    #[serde(default)]
    pub eula_accepted: bool,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub runtime: RuntimeKind,
    #[serde(default)]
    pub docker_image: Option<String>,
    #[serde(default)]
    pub cpu_limit: Option<f32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub backup_keep: Option<u32>,
    #[serde(default)]
    pub backup_hour: Option<u8>,
    #[serde(default)]
    pub java_major: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateInstanceRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub memory_mib: Option<u32>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub auto_restart: Option<bool>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub core: Option<String>,
    #[serde(default)]
    pub eula_accepted: Option<bool>,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub runtime: Option<RuntimeKind>,
    #[serde(default)]
    pub docker_image: Option<String>,
    #[serde(default)]
    pub cpu_limit: Option<f32>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub desired_running: Option<bool>,
    #[serde(default)]
    pub backup_keep: Option<u32>,
    #[serde(default)]
    pub backup_hour: Option<u8>,
    /// 0 clears to auto.
    #[serde(default)]
    pub java_major: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Debug, Deserialize)]
pub struct EulaRequest {
    pub accepted: bool,
}

#[derive(Debug, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct BackupInfo {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    Backup,
    Restart,
    Command,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub instance_id: String,
    pub kind: ScheduleKind,
    /// Interval in seconds.
    pub every_secs: u64,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub next_run_at: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    pub instance_id: String,
    pub kind: ScheduleKind,
    pub every_secs: u64,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct PropertiesUpdate {
    pub entries: Vec<PropertyEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PropertyEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PlayerInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default)]
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ping_ms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    #[serde(default)]
    pub session_secs: u64,
    #[serde(default)]
    pub total_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlayerActionRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BulkActionRequest {
    pub action: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BulkActionResult {
    pub ok: Vec<String>,
    pub failed: Vec<BulkFailure>,
}

#[derive(Debug, Serialize)]
pub struct BulkFailure {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct FleetSummary {
    pub total: usize,
    pub running: usize,
    pub stopped: usize,
    pub starting: usize,
    pub crashed: usize,
    pub by_group: Vec<GroupCount>,
    pub by_runtime: Vec<RuntimeCount>,
    pub docker: crate::instance::container::DockerStatus,
}

#[derive(Debug, Serialize)]
pub struct GroupCount {
    pub group: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct RuntimeCount {
    pub runtime: String,
    pub count: usize,
}
