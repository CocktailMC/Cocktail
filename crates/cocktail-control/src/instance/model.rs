//! 实例数据模型。
//!
//! 阶段 2 拆分后：纯数据类型（`InstanceSpec`/`InstanceStatus`/`MetricSample` 等）
//! 已抽到 `cocktail_shared::model`，本模块通过 `pub use` 重新导出，保持
//! `crate::instance::InstanceSpec` 等旧调用路径不变。
//! `Instance` 与 `FleetSummary` 因持有运行时句柄（`ProcessHandle`）或
//! 引用 container 模块的 `DockerStatus`，仍保留本地定义。

// 重新导出共享层的纯数据类型，避免破坏现有 `use crate::instance::...` 调用路径。
pub use cocktail_shared::model::{
    BackupInfo, BulkActionRequest, BulkActionResult, BulkFailure, CloneInstanceRequest,
    CommandRequest, CreateInstanceRequest, CreateScheduleRequest, EulaRequest, FileContent,
    FileEntry, GroupCount, InstanceEvent, InstanceSpec, InstanceStatus, InstanceView, LogLine,
    MetricSample, NetPeer, PlayerActionRequest, PlayerInfo, PluginInfo, PreflightReport,
    PropertiesUpdate, PropertyEntry, RestorePreview, RuntimeCount, RuntimeKind, Schedule,
    ScheduleKind, UpdateInstanceRequest, VersionCompare, WorldDownloadInfo, WriteFileRequest,
    default_backup_keep, default_core, default_memory_mib, default_node_id, default_port,
    default_true, default_view_node, is_local_node, new_instance_id,
};

use chrono::Utc;
use uuid::Uuid;

/// 单个 Minecraft 实例的运行时实体。
///
/// 持有 `ProcessHandle`（本地子进程句柄），不可跨进程序列化，
/// 因此留在 control 端；跨进程传递时通过 `InstanceView`/`InstanceSpec`。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Instance {
    pub id: String,
    pub spec: InstanceSpec,
    pub status: InstanceStatus,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
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
    pub fn with_id(id: String, spec: InstanceSpec) -> Self {
        let now = Utc::now();
        Self {
            id,
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

    pub fn new(spec: InstanceSpec) -> Self {
        Self::with_id(Uuid::new_v4().to_string(), spec)
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
            reattached: self.process.as_ref().is_some_and(|p| p.reattached),
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

/// 全局 fleet 摘要。引用 container 模块的 `DockerStatus`，留 control 本地。
#[derive(Debug, serde::Serialize)]
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
