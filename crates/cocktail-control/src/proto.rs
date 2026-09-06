use serde::{Deserialize, Serialize};

use crate::instance::{Instance, InstanceSpec, InstanceStatus, LogLine, MetricSample};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyInstance {
    pub id: String,
    pub spec: InstanceSpec,
    pub generation: u64,
}

impl From<&Instance> for ApplyInstance {
    fn from(i: &Instance) -> Self {
        Self {
            id: i.id.clone(),
            spec: i.spec.clone(),
            generation: i.generation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentDown {
    Welcome {
        node_id: String,
        instances: Vec<ApplyInstance>,
    },
    Apply {
        instance: ApplyInstance,
    },
    Stop {
        instance_id: String,
    },
    Command {
        instance_id: String,
        command: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentUp {
    Hello {
        hostname: String,
        os: String,
        arch: String,
    },
    Heartbeat {
        #[serde(default)]
        cpu_pct: f32,
        #[serde(default)]
        memory_mib: f32,
        #[serde(default)]
        rx_bps: f32,
        #[serde(default)]
        tx_bps: f32,
    },
    Status {
        instance_id: String,
        status: InstanceStatus,
        pid: Option<u32>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceManifest {
    #[serde(rename = "apiVersion", default = "api_version")]
    pub api_version: String,
    #[serde(default = "kind_instance")]
    pub kind: String,
    pub id: String,
    pub spec: InstanceSpec,
}

fn api_version() -> String {
    "cocktail.mc/v1".into()
}

fn kind_instance() -> String {
    "Instance".into()
}

impl InstanceManifest {
    pub fn from_instance(i: &Instance) -> Self {
        Self {
            api_version: api_version(),
            kind: kind_instance(),
            id: i.id.clone(),
            spec: i.spec.clone(),
        }
    }
}
