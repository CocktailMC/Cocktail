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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub ts: DateTime<Utc>,
    pub cpu_pct: f32,
    pub memory_mib: f32,
    pub tps: Option<f32>,
    pub players: u32,
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
            process: None,
        }
    }

    pub fn public_view(&self) -> InstanceView {
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
}

#[derive(Debug, Deserialize)]
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
