//! Cocktail 共享数据模型：control plane 与 init 子进程之间 IPC 消息使用的纯数据类型。
//!
//! 设计原则：只放 serde 友好的纯数据结构，不持有句柄、不依赖 tokio。
//! `Instance`（含 `ProcessHandle`）等带运行时句柄的类型由 control/init 各自
//! 维护本地版本，通过 `InstanceView`/`InstanceSpec` 等 IPC 友好的纯数据类型
//! 跨进程传递。
//!
//! 注：control 端的 `instance::model::Instance` 与 `FleetSummary` 因持有
//! 运行时句柄或跨 crate 引用（`DockerStatus`），保留在 control 本地定义；
//! 其余纯数据类型从此 crate 复用。

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
    Podman,
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
    #[serde(default = "default_node_id")]
    pub node_id: String,
    #[serde(default)]
    pub desired_running: bool,
    #[serde(default = "default_backup_keep")]
    pub backup_keep: u32,
    #[serde(default)]
    pub backup_hour: Option<u8>,

    #[serde(default)]
    pub java_major: Option<u32>,

    #[serde(default)]
    pub mc_version: Option<String>,
}

pub fn is_local_node(node_id: &str) -> bool {
    node_id.is_empty() || node_id == "local"
}

pub fn default_backup_keep() -> u32 {
    7
}

pub fn default_node_id() -> String {
    "local".into()
}

pub fn default_memory_mib() -> u32 {
    1024
}

pub fn default_core() -> String {
    "demo".into()
}

pub fn default_port() -> u16 {
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
    DownloadProgress {
        id: String,
        label: String,
        phase: String,
        received: u64,
        total: Option<u64>,
        pct: Option<f32>,
    },
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

pub fn default_view_node() -> String {
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupInfo {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub path: String,
    pub size_bytes: u64,
}

/// 备份内容扫描结果：由 init 端 `files::inspect_backup_zip` 产出，
/// control 端消费以构建 `RestorePreview`。跨进程 IPC 友好的纯数据。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackupScan {
    pub entries: u32,
    pub size_bytes: u64,
    pub world_bytes: u64,
    pub plugin_count: u32,
    pub has_server_properties: bool,
    pub has_level_dat: bool,
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

    pub every_secs: u64,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub next_run_at: DateTime<Utc>,
}

pub fn default_true() -> bool {
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

#[derive(Debug, Deserialize, Default)]
pub struct CloneInstanceRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub copy_data: Option<bool>,
    #[serde(default)]
    pub skip_logs: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PreflightReport {
    pub instance_id: String,
    pub warnings: Vec<String>,
    pub free_bytes: Option<u64>,
    pub used_bytes: u64,
    pub port_busy: bool,
}

#[derive(Debug, Serialize)]
pub struct WorldDownloadInfo {
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct VersionCompare {
    pub current: Option<String>,
    pub latest: Option<String>,
    pub behind: bool,
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct RestorePreview {
    pub backup_id: String,
    pub size_bytes: u64,
    pub created_at: String,
    pub entries: u32,
    pub world_size_bytes: u64,
    pub plugin_count: u32,
    pub has_server_properties: bool,
    pub has_level_dat: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BulkFailure {
    pub id: String,
    pub error: String,
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

/// 生成新实例 ID（UUID v4 字符串）。control 与 init 都可能创建实例，
/// 抽到共享层避免重复实现。
pub fn new_instance_id() -> String {
    Uuid::new_v4().to_string()
}
