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
        #[serde(default = "default_protocol_version")]
        protocol_version: u32,
    },
    Apply {
        instance: ApplyInstance,
        #[serde(default)]
        seq: u64,
    },
    Stop {
        instance_id: String,
        #[serde(default)]
        seq: u64,
    },
    Command {
        instance_id: String,
        command: String,
        #[serde(default)]
        seq: u64,
    },
}

impl AgentDown {
    pub fn with_seq(self, seq: u64) -> Self {
        match self {
            AgentDown::Apply { instance, .. } => AgentDown::Apply { instance, seq },
            AgentDown::Stop { instance_id, .. } => AgentDown::Stop { instance_id, seq },
            AgentDown::Command {
                instance_id,
                command,
                ..
            } => AgentDown::Command {
                instance_id,
                command,
                seq,
            },
            other => other,
        }
    }

    pub fn seq(&self) -> u64 {
        match self {
            AgentDown::Apply { seq, .. }
            | AgentDown::Stop { seq, .. }
            | AgentDown::Command { seq, .. } => *seq,
            AgentDown::Welcome { .. } => 0,
        }
    }
}

pub const PROTOCOL_VERSION: u32 = 1;

fn default_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentUp {
    Hello {
        hostname: String,
        os: String,
        arch: String,
        #[serde(default = "default_protocol_version")]
        protocol_version: u32,
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
        #[serde(default)]
        nic_stats: Vec<NicStat>,
        #[serde(default)]
        tcp_states: TcpStates,
    },
    Ack {
        seq: u64,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NicStat {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_pkts: u64,
    pub tx_pkts: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TcpStates {
    pub estab: u32,
    pub syn_recv: u32,
    pub time_wait: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_roundtrip() {
        let msg = AgentUp::Ack { seq: 42 };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"seq\":42"));
        let back: AgentUp = serde_json::from_str(&s).unwrap();
        match back {
            AgentUp::Ack { seq } => assert_eq!(seq, 42),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn legacy_apply_without_seq_defaults_zero() {
        let raw = r#"{"type":"stop","instance_id":"abc"}"#;
        let msg: AgentDown = serde_json::from_str(raw).unwrap();
        match msg {
            AgentDown::Stop { instance_id, seq } => {
                assert_eq!(instance_id, "abc");
                assert_eq!(seq, 0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn legacy_heartbeat_without_net_stats() {
        let raw = r#"{"type":"heartbeat","cpu_pct":1.5}"#;
        let msg: AgentUp = serde_json::from_str(raw).unwrap();
        match msg {
            AgentUp::Heartbeat {
                nic_stats,
                tcp_states,
                ..
            } => {
                assert!(nic_stats.is_empty());
                assert_eq!(tcp_states.estab, 0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn welcome_carries_protocol_version() {
        let raw = r#"{"type":"welcome","node_id":"n1","instances":[]}"#;
        let msg: AgentDown = serde_json::from_str(raw).unwrap();
        match msg {
            AgentDown::Welcome {
                protocol_version, ..
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
            }
            _ => panic!("wrong variant"),
        }
    }
}
