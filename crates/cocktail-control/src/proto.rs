//! control ↔ 远程 agent 节点的 JSON over WS 协议。
//!
//! 阶段 2 拆分后：纯协议类型（`AgentUp`/`AgentDown`/`ApplyInstance`/
//! `InstanceManifest`/`NicStat`/`TcpStates`/`PROTOCOL_VERSION`）已抽到
//! `cocktail_shared::proto`，本模块通过 `pub use` 重新导出，保持现有
//! `use crate::proto::AgentUp` 等调用路径不变。
//! `From<&Instance>` 等需要本地运行时句柄的 impl 仍保留在 control 端。

pub use cocktail_shared::proto::{
    AgentDown, AgentUp, ApplyInstance, InstanceManifest, NicStat, PROTOCOL_VERSION, TcpStates,
    api_version, default_protocol_version, kind_instance,
};

use crate::instance::Instance;

/// 从本地 `Instance` 派生 `ApplyInstance`（跨进程 IPC 友好）。
/// 因为 `Instance` 留 control，此 impl 也留 control。
impl From<&Instance> for ApplyInstance {
    fn from(i: &Instance) -> Self {
        Self {
            id: i.id.clone(),
            spec: i.spec.clone(),
            generation: i.generation,
        }
    }
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

#[cfg(test)]
mod tests {
    // 共享层测试已在 cocktail-shared/src/proto.rs 覆盖。
    // 这里保留一个简单的 end-to-end 校验：从 Instance 派生 ApplyInstance 不丢字段。
    use super::*;
    use crate::instance::InstanceSpec;

    #[test]
    fn apply_from_instance_roundtrip() {
        let spec = InstanceSpec {
            name: "demo".into(),
            workdir: "data/instances/demo".into(),
            command: None,
            args: Vec::new(),
            memory_mib: 1024,
            core: "demo".into(),
            port: 25565,
            auto_restart: false,
            eula_accepted: false,
            webhook_url: None,
            runtime: Default::default(),
            docker_image: None,
            cpu_limit: None,
            tags: Vec::new(),
            group: None,
            node_id: "local".into(),
            desired_running: false,
            backup_keep: 7,
            backup_hour: None,
            java_major: None,
            mc_version: None,
        };
        let inst = Instance::new(spec);
        let apply = ApplyInstance::from(&inst);
        assert_eq!(apply.id, inst.id);
        assert_eq!(apply.generation, inst.generation);
        assert_eq!(apply.spec.name, "demo");
    }
}
