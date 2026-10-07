//! Runtime RPC contracts. Handles identify one launch in one init session.
use crate::model::InstanceEvent;
use serde::{Deserialize, Serialize};

pub const INSTANCE_EVENT: &str = "instance.event";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StopMode {
    Graceful,
    Force,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerStatus {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerImage {
    pub repo_tag: String,
    pub id: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleInfo {
    pub token: String,
    pub child_id: u32,
    pub reattached: bool,
    pub container_name: Option<String>,
    pub start_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub token: String,
    pub event: InstanceEvent,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LaunchRequest {
    pub token: String,
    pub instance_id: String,
    pub workdir: String,
    pub port: u16,
    #[serde(flatten)]
    pub kind: LaunchKind,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchKind {
    Process {
        command: Option<String>,
        args: Vec<String>,
        memory_mib: u32,
    },
    Container {
        command: Option<String>,
        args: Vec<String>,
        memory_mib: u32,
        cpu_limit: Option<f32>,
        image: String,
    },
    Adopt {
        pid: u32,
        expected_start: Option<u64>,
        container_name: Option<String>,
        reattached: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HandleRequest {
    pub token: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct StopRequest {
    pub token: String,
    pub mode: StopMode,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandRequest {
    pub token: String,
    pub command: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PidRequest {
    pub pid: u32,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MatchRequest {
    pub pid: u32,
    pub expected_start: Option<u64>,
    pub workdir: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct NameRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstanceIdRequest {
    pub instance_id: String,
}
