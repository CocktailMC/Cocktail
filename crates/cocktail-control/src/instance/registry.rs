use std::path::PathBuf;

use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::cluster;
use crate::proto::{AgentDown, ApplyInstance, InstanceManifest};
use crate::state::AppState;
use crate::util;

use super::files;
use super::model::{
    BackupInfo, BulkActionRequest, BulkActionResult, BulkFailure, CloneInstanceRequest,
    CommandRequest, CreateInstanceRequest, CreateScheduleRequest, EulaRequest, FileContent,
    FileEntry, FleetSummary, GroupCount, Instance, InstanceEvent, InstanceSpec, InstanceStatus,
    InstanceView, PlayerInfo, PluginInfo, PreflightReport, PropertyEntry, RestorePreview,
    RuntimeCount, RuntimeKind, Schedule, ScheduleKind, UpdateInstanceRequest, VersionCompare,
    is_local_node,
};
use super::process::{self, StopMode};

pub async fn list_instances(state: &AppState) -> Vec<InstanceView> {
    let guard = state.instances.read().await;
    let mut list: Vec<_> = guard.values().map(|i| i.public_view()).collect();
    list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    list
}

pub async fn get_instance(state: &AppState, id: &str) -> Option<InstanceView> {
    state
        .instances
        .read()
        .await
        .get(id)
        .map(|i| i.public_view())
}

pub async fn create_instance(
    state: &AppState,
    req: CreateInstanceRequest,
) -> anyhow::Result<InstanceView> {
    let node_id = req
        .node_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "local".into());
    if !cluster::node_exists(state, &node_id).await {
        anyhow::bail!("节点不存在：{node_id}");
    }
    ensure_port_free(state, req.port, &node_id, None).await?;

    let id = Uuid::new_v4().to_string();
    let workdir = req
        .workdir
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| files::default_instance_root(&id));
    ensure_exclusive_workdir(state, &workdir, None).await?;

    if is_local_node(&node_id) {
        files::ensure_seed_files(&workdir, req.port, req.eula_accepted)?;
    }

    let docker_image = match req.runtime {
        RuntimeKind::Docker | RuntimeKind::Podman => Some(
            req.docker_image
                .unwrap_or_else(|| "eclipse-temurin:21-jre".into()),
        ),
        RuntimeKind::Process => req.docker_image,
    };

    let spec = InstanceSpec {
        name: req.name,
        workdir,
        command: req.command,
        args: req.args,
        memory_mib: req.memory_mib,
        core: req.core,
        port: req.port,
        auto_restart: req.auto_restart,
        eula_accepted: req.eula_accepted,
        webhook_url: req.webhook_url,
        runtime: req.runtime,
        docker_image,
        cpu_limit: req.cpu_limit,
        tags: req.tags,
        group: req.group,
        node_id,
        desired_running: false,
        backup_keep: req.backup_keep.unwrap_or(7).clamp(1, 90),
        backup_hour: req.backup_hour.filter(|h| *h <= 23),
        java_major: req.java_major.filter(|m| *m >= 8),
        mc_version: None,
    };

    let instance = Instance::with_id(id.clone(), spec);
    let view = instance.public_view();

    state.instances.write().await.insert(id.clone(), instance);
    state.publish(InstanceEvent::StatusChanged {
        instance_id: id.clone(),
        status: InstanceStatus::Created,
        at: Utc::now(),
    });
    let _ = state.persist().await;
    let _ = crate::netops::try_apply(state).await;
    util::audit(
        "instance.create",
        Some(&id),
        json!({ "name": view.spec.name }),
        "api",
    );

    Ok(view)
}

pub async fn update_instance(
    state: &AppState,
    id: &str,
    req: UpdateInstanceRequest,
) -> anyhow::Result<InstanceView> {
    if let Some(port) = req.port {
        let node = {
            let guard = state.instances.read().await;
            guard
                .get(id)
                .map(|i| i.spec.node_id.clone())
                .unwrap_or_else(|| "local".into())
        };
        ensure_port_free(state, port, &node, Some(id)).await?;
    }

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    if let Some(name) = req.name {
        if name.trim().is_empty() {
            anyhow::bail!("name is required");
        }
        instance.spec.name = name;
    }
    if let Some(m) = req.memory_mib {
        instance.spec.memory_mib = m;
    }
    if let Some(p) = req.port {
        instance.spec.port = p;
        files::sync_port(&instance.spec.workdir, p)?;
    }
    if let Some(ar) = req.auto_restart {
        instance.spec.auto_restart = ar;
    }
    if let Some(cmd) = req.command {
        instance.spec.command = if cmd.is_empty() { None } else { Some(cmd) };
    }
    if let Some(args) = req.args {
        instance.spec.args = args;
    }
    if let Some(core) = req.core {
        instance.spec.core = core;
    }
    if let Some(eula) = req.eula_accepted {
        instance.spec.eula_accepted = eula;
        util::write_eula(&instance.spec.workdir, eula)?;
    }
    if let Some(url) = req.webhook_url {
        instance.spec.webhook_url = if url.is_empty() { None } else { Some(url) };
    }
    if let Some(rt) = req.runtime {
        instance.spec.runtime = rt;
    }
    if let Some(img) = req.docker_image {
        instance.spec.docker_image = if img.is_empty() { None } else { Some(img) };
    }
    if let Some(cpu) = req.cpu_limit {
        instance.spec.cpu_limit = if cpu > 0.0 { Some(cpu) } else { None };
    }
    if let Some(tags) = req.tags {
        instance.spec.tags = tags;
    }
    if let Some(group) = req.group {
        instance.spec.group = if group.is_empty() { None } else { Some(group) };
    }
    if let Some(node_id) = req.node_id {
        if !cluster::node_exists(state, &node_id).await {
            anyhow::bail!("节点不存在：{node_id}");
        }
        instance.spec.node_id = if node_id.is_empty() {
            "local".into()
        } else {
            node_id
        };
    }
    if let Some(desired) = req.desired_running {
        instance.spec.desired_running = desired;
    }
    if let Some(k) = req.backup_keep {
        instance.spec.backup_keep = k.clamp(1, 90);
    }
    if let Some(h) = req.backup_hour {
        instance.spec.backup_hour = if h <= 23 { Some(h) } else { None };
    }
    if let Some(m) = req.java_major {
        instance.spec.java_major = if m >= 8 { Some(m) } else { None };
    }
    instance.generation = instance.generation.saturating_add(1);
    instance.updated_at = Utc::now();
    let view = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    let _ = crate::netops::try_apply(state).await;
    util::audit("instance.update", Some(id), json!({}), "api");
    Ok(view)
}

pub async fn accept_eula(
    state: &AppState,
    id: &str,
    req: EulaRequest,
) -> anyhow::Result<InstanceView> {
    update_instance(
        state,
        id,
        UpdateInstanceRequest {
            eula_accepted: Some(req.accepted),
            ..UpdateInstanceRequest::default()
        },
    )
    .await
}

pub async fn start_instance(state: &AppState, id: &str) -> anyhow::Result<InstanceView> {
    let (port, needs_eula, eula_ok, node_id, stopping, pid, container) = {
        let guard = state.instances.read().await;
        let instance = guard
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

        let needs_eula = instance.spec.command.is_some()
            || super::versions::core_needs_eula(&instance.spec.core);
        let eula_ok = instance.spec.eula_accepted || util::eula_is_accepted(&instance.spec.workdir);
        let container = instance.docker_container.clone().or_else(|| {
            instance
                .process
                .as_ref()
                .and_then(|h| h.container_name.clone())
        });
        let pid = instance
            .process
            .as_ref()
            .map(|h| h.child_id)
            .or(instance.last_pid);
        (
            instance.spec.port,
            needs_eula,
            eula_ok,
            instance.spec.node_id.clone(),
            instance.status == InstanceStatus::Stopping,
            pid,
            container,
        )
    };

    if stopping {
        let alive = runtime_is_alive(pid, container.as_deref()).await;
        if alive {
            anyhow::bail!("instance is stopping");
        }
    }

    if needs_eula && !eula_ok {
        anyhow::bail!("EULA not accepted; call POST /eula with {{\"accepted\":true}}");
    }
    ensure_port_free(state, port, &node_id, Some(id)).await?;

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    instance.spec.desired_running = true;

    if matches!(
        instance.status,
        InstanceStatus::Running | InstanceStatus::Starting
    ) {
        let view = instance.public_view();
        drop(guard);
        let _ = state.persist().await;
        if !is_local_node(&node_id) {
            let snap = {
                let g = state.instances.read().await;
                g.get(id).map(ApplyInstance::from)
            };
            if let Some(snap) = snap {
                cluster::send_down(
                    state,
                    &node_id,
                    AgentDown::Apply {
                        instance: snap,
                        seq: 0,
                    },
                )
                .await?;
            }
        }
        return Ok(view);
    }

    instance.generation = instance.generation.saturating_add(1);

    instance.status = InstanceStatus::Starting;
    instance.updated_at = Utc::now();
    let instance_id = instance.id.clone();

    if !is_local_node(&node_id) {
        let snap = ApplyInstance::from(&*instance);
        let view = instance.public_view();
        drop(guard);
        let _ = state.persist().await;
        cluster::send_down(
            state,
            &node_id,
            AgentDown::Apply {
                instance: snap,
                seq: 0,
            },
        )
        .await?;
        util::audit(
            "instance.start",
            Some(&instance_id),
            json!({ "node": node_id }),
            "api",
        );
        return Ok(view);
    }

    let workdir = instance.spec.workdir.clone();
    let mut command = instance.spec.command.clone();
    let mut args = instance.spec.args.clone();
    let memory_mib = instance.spec.memory_mib;
    let port = instance.spec.port;
    let eula = instance.spec.eula_accepted;
    let runtime = instance.spec.runtime;
    let docker_image = instance
        .spec
        .docker_image
        .clone()
        .unwrap_or_else(|| "eclipse-temurin:21-jre".into());
    let cpu_limit = instance.spec.cpu_limit;
    let java_major = instance.spec.java_major;
    let mc_version = instance.spec.mc_version.clone();
    let events = state.events.clone();

    if command.is_none() || (command.as_deref() == Some("java") && args.is_empty()) {
        if files::jar_exists(&workdir, "server.jar") {
            let (cmd, a) = util::java_jar_startup("server.jar");
            command = Some(cmd.clone());
            args = a.clone();
            instance.spec.command = Some(cmd);
            instance.spec.args = a;
            if instance.spec.core == "demo" {
                instance.spec.core = "custom".into();
            }
        } else if matches!(runtime, RuntimeKind::Docker | RuntimeKind::Podman)
            || (instance.spec.core != "demo" && super::versions::is_known_core(&instance.spec.core))
            || instance.spec.core == "custom"
            || instance.spec.core == "spigot"
        {
            anyhow::bail!(
                "未配置启动命令且找不到 server.jar：请先在「版本 / jar」导入压缩包、jar 或下载核心"
            );
        }
    }

    let seed_port = match runtime {
        RuntimeKind::Docker | RuntimeKind::Podman => 25565,
        RuntimeKind::Process => port,
    };
    files::ensure_seed_files(&workdir, seed_port, eula)?;

    state.publish(InstanceEvent::StatusChanged {
        instance_id: instance_id.clone(),
        status: InstanceStatus::Starting,
        at: Utc::now(),
    });
    drop(guard);

    let docker_image = if matches!(runtime, RuntimeKind::Docker | RuntimeKind::Podman)
        && (docker_image.is_empty() || docker_image.starts_with("eclipse-temurin:"))
    {
        crate::java::docker_image_for(
            java_major
                .unwrap_or_else(|| crate::java::recommended_java_major(mc_version.as_deref())),
        )
    } else {
        docker_image
    };
    let command = if runtime == RuntimeKind::Process
        && command.as_deref().is_some_and(util::is_java_command)
    {
        match crate::java::ensure_for_spec(&workdir, java_major, mc_version.as_deref()).await {
            Ok(bin) => crate::java::rewrite_java_command(command, &bin),
            Err(e) => {
                let mut g = state.instances.write().await;
                if let Some(inst) = g.get_mut(&instance_id) {
                    inst.status = InstanceStatus::Stopped;
                    inst.spec.desired_running = false;
                    inst.updated_at = Utc::now();
                }
                drop(g);
                let _ = state.persist().await;
                anyhow::bail!("无法准备 Java 运行时（Adoptium Temurin）：{e}");
            }
        }
    } else {
        command
    };

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(&instance_id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    let handle = match runtime {
        RuntimeKind::Docker | RuntimeKind::Podman => {
            super::container::spawn_docker_instance(
                instance_id.clone(),
                workdir,
                command,
                args,
                memory_mib,
                port,
                cpu_limit,
                &docker_image,
                events,
            )
            .await?
        }
        RuntimeKind::Process => {
            process::spawn_instance(
                instance_id.clone(),
                workdir,
                command,
                args,
                memory_mib,
                port,
                events,
            )
            .await?
        }
    };
    instance.process = Some(handle);
    if let Some(proc) = instance.process.as_ref() {
        if proc.child_id > 0 {
            instance.last_pid = Some(proc.child_id);
            instance.last_start_time = process::process_snapshot(proc.child_id).map(|(t, _)| t);
        }
        instance.docker_container = proc.container_name.clone();
    }
    instance.updated_at = Utc::now();
    let view = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit("instance.start", Some(&instance_id), json!({}), "api");
    Ok(view)
}

pub async fn reattach_running(state: &std::sync::Arc<AppState>) {
    let snapshot: Vec<(
        String,
        RuntimeKind,
        String,
        Option<u32>,
        Option<u64>,
        Option<String>,
        u16,
    )> = {
        let guard = state.instances.read().await;
        guard
            .values()
            .filter(|i| is_local_node(&i.spec.node_id))
            .map(|i| {
                (
                    i.id.clone(),
                    i.spec.runtime,
                    i.spec.workdir.clone(),
                    i.last_pid,
                    i.last_start_time,
                    i.docker_container.clone(),
                    i.spec.port,
                )
            })
            .collect()
    };

    for (id, runtime, workdir, last_pid, start, container, port) in snapshot {
        let adopted = match runtime {
            RuntimeKind::Docker | RuntimeKind::Podman => {
                if let Some(name) = container.clone() {
                    if let Some(pid) = process::docker_container_pid(&name).await {
                        match process::adopt_running(
                            id.clone(),
                            pid,
                            workdir,
                            state.events.clone(),
                            Some(name),
                            true,
                            port,
                        )
                        .await
                        {
                            Ok(handle) => Some(handle),
                            Err(e) => {
                                tracing::warn!(error = %e, %id, "failed to adopt docker instance");
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            RuntimeKind::Process => {
                if let Some(pid) = last_pid.filter(|p| *p > 0) {
                    if process::process_matches(pid, start, &workdir) {
                        match process::adopt_running(
                            id.clone(),
                            pid,
                            workdir,
                            state.events.clone(),
                            None,
                            true,
                            port,
                        )
                        .await
                        {
                            Ok(handle) => Some(handle),
                            Err(e) => {
                                tracing::warn!(error = %e, %id, "failed to adopt process");
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        let mut guard = state.instances.write().await;
        if let Some(inst) = guard.get_mut(&id) {
            if let Some(handle) = adopted {
                tracing::info!(
                    instance_id = %id,
                    pid = handle.child_id,
                    "reattached running instance"
                );
                inst.last_pid = Some(handle.child_id).filter(|p| *p > 0);
                inst.last_start_time = process::process_snapshot(handle.child_id).map(|(t, _)| t);
                inst.docker_container = handle.container_name.clone();
                inst.process = Some(handle);
                inst.status = InstanceStatus::Running;
                inst.updated_at = Utc::now();
            } else if matches!(
                inst.status,
                InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
            ) {
                inst.status = InstanceStatus::Stopped;
                inst.last_pid = None;
                inst.last_start_time = None;
                inst.docker_container = None;
                inst.updated_at = Utc::now();
            }
        }
    }
    let _ = state.persist().await;
}

pub async fn stop_instance(state: &AppState, id: &str) -> anyhow::Result<InstanceView> {
    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    instance.spec.desired_running = false;
    instance.generation = instance.generation.saturating_add(1);
    let node_id = instance.spec.node_id.clone();

    if !is_local_node(&node_id) {
        instance.status = InstanceStatus::Stopping;
        instance.updated_at = Utc::now();
        let snap_id = instance.id.clone();
        let view = instance.public_view();
        drop(guard);
        let _ = state.persist().await;
        cluster::send_down(
            state,
            &node_id,
            AgentDown::Stop {
                instance_id: snap_id.clone(),
                seq: 0,
            },
        )
        .await?;
        util::audit(
            "instance.stop",
            Some(&snap_id),
            json!({ "node": node_id }),
            "api",
        );
        return Ok(view);
    }

    if matches!(
        instance.status,
        InstanceStatus::Stopped | InstanceStatus::Created | InstanceStatus::Crashed
    ) {
        instance.status = InstanceStatus::Stopped;
        instance.updated_at = Utc::now();
        let view = instance.public_view();
        drop(guard);
        let _ = state.persist().await;
        return Ok(view);
    }

    instance.status = InstanceStatus::Stopping;
    instance.updated_at = Utc::now();
    state.publish(InstanceEvent::StatusChanged {
        instance_id: id.to_string(),
        status: InstanceStatus::Stopping,
        at: Utc::now(),
    });

    if let Some(handle) = instance.process.take() {
        drop(guard);
        handle.stop(StopMode::Graceful).await;
        let mut guard = state.instances.write().await;
        if let Some(inst) = guard.get_mut(id) {
            if !inst.spec.desired_running {
                inst.status = InstanceStatus::Stopped;
                inst.process = None;
                inst.last_pid = None;
                inst.last_start_time = None;
                inst.docker_container = None;
                inst.updated_at = Utc::now();
                state.publish(InstanceEvent::StatusChanged {
                    instance_id: id.to_string(),
                    status: InstanceStatus::Stopped,
                    at: inst.updated_at,
                });
            }
            let view = inst.public_view();
            drop(guard);
            let _ = state.persist().await;
            util::audit(
                "instance.stop",
                Some(id),
                json!({ "mode": "graceful" }),
                "api",
            );
            return Ok(view);
        }
        anyhow::bail!("instance not found after stop");
    }

    instance.status = InstanceStatus::Stopped;
    instance.process = None;
    instance.last_pid = None;
    instance.last_start_time = None;
    instance.docker_container = None;
    instance.updated_at = Utc::now();
    state.publish(InstanceEvent::StatusChanged {
        instance_id: id.to_string(),
        status: InstanceStatus::Stopped,
        at: instance.updated_at,
    });
    let view = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    Ok(view)
}

pub async fn restart_instance(state: &AppState, id: &str) -> anyhow::Result<InstanceView> {
    let _ = stop_instance(state, id).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    start_instance(state, id).await
}

pub async fn delete_instance(state: &AppState, id: &str) -> anyhow::Result<()> {
    let _ = stop_instance(state, id).await;
    let removed = state.instances.write().await.remove(id);
    if removed.is_none() {
        anyhow::bail!("instance not found");
    }
    let _ = state.persist().await;
    util::audit("instance.delete", Some(id), json!({}), "api");
    Ok(())
}

pub async fn send_command(state: &AppState, id: &str, req: CommandRequest) -> anyhow::Result<()> {
    let cmd = req.command.trim().to_string();
    if cmd.is_empty() {
        anyhow::bail!("command is empty");
    }
    let (running, node_id, has_handle) = {
        let guard = state.instances.read().await;
        let instance = guard
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
        (
            instance.status == InstanceStatus::Running,
            instance.spec.node_id.clone(),
            instance.process.is_some(),
        )
    };
    if !running {
        anyhow::bail!("instance is not running");
    }
    if !is_local_node(&node_id) {
        cluster::send_down(
            state,
            &node_id,
            AgentDown::Command {
                instance_id: id.to_string(),
                command: cmd.clone(),
                seq: 0,
            },
        )
        .await?;
        util::audit(
            "instance.command",
            Some(id),
            json!({ "command": cmd }),
            "api",
        );
        return Ok(());
    }
    let _ = has_handle;
    let guard = state.instances.read().await;
    let instance = guard
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if instance.status != InstanceStatus::Running {
        anyhow::bail!("instance is not running");
    }
    let handle = instance
        .process
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("process handle missing"))?;
    handle.send_command(cmd.clone()).await?;
    util::audit(
        "instance.command",
        Some(id),
        json!({ "command": cmd }),
        "api",
    );
    Ok(())
}

pub async fn reconcile_local(state: &std::sync::Arc<AppState>) {
    let ids: Vec<(String, bool, InstanceStatus)> = {
        let guard = state.instances.read().await;
        guard
            .values()
            .filter(|i| is_local_node(&i.spec.node_id))
            .map(|i| (i.id.clone(), i.spec.desired_running, i.status))
            .collect()
    };
    for (id, desired, status) in ids {
        if desired {
            if matches!(
                status,
                InstanceStatus::Created | InstanceStatus::Stopped | InstanceStatus::Crashed
            ) {
                if let Err(e) = start_instance(state, &id).await {
                    tracing::warn!(error = %e, %id, "reconcile start failed");
                }
            }
        } else if matches!(status, InstanceStatus::Running | InstanceStatus::Starting) {
            if let Err(e) = stop_instance(state, &id).await {
                tracing::warn!(error = %e, %id, "reconcile stop failed");
            }
        } else if status == InstanceStatus::Stopping {
            recover_stuck_stopping(state, &id).await;
        }
    }
}

pub async fn recover_stale_statuses(state: &std::sync::Arc<AppState>) {
    let ids: Vec<String> = {
        let guard = state.instances.read().await;
        guard
            .values()
            .filter(|i| is_local_node(&i.spec.node_id))
            .filter(|i| i.status == InstanceStatus::Stopping)
            .map(|i| i.id.clone())
            .collect()
    };
    for id in ids {
        recover_stuck_stopping(state, &id).await;
    }
}

async fn runtime_is_alive(pid: Option<u32>, container: Option<&str>) -> bool {
    if let Some(name) = container.filter(|s| !s.is_empty()) {
        return process::docker_container_running(name).await;
    }
    pid.filter(|p| *p > 0).is_some_and(process::pid_is_alive)
}

async fn recover_stuck_stopping(state: &AppState, id: &str) {
    let (pid, container) = {
        let guard = state.instances.read().await;
        let Some(inst) = guard.get(id) else { return };
        if inst.status != InstanceStatus::Stopping {
            return;
        }
        let container = inst
            .docker_container
            .clone()
            .or_else(|| inst.process.as_ref().and_then(|h| h.container_name.clone()));
        let pid = inst.process.as_ref().map(|h| h.child_id).or(inst.last_pid);
        (pid, container)
    };
    if runtime_is_alive(pid, container.as_deref()).await {
        return;
    }
    let mut guard = state.instances.write().await;
    let Some(inst) = guard.get_mut(id) else {
        return;
    };
    if inst.status != InstanceStatus::Stopping || inst.spec.desired_running {
        return;
    }
    tracing::info!(instance_id = %id, "clearing stuck stopping status; process is gone");
    inst.status = InstanceStatus::Stopped;
    inst.process = None;
    inst.last_pid = None;
    inst.last_start_time = None;
    inst.docker_container = None;
    inst.updated_at = Utc::now();
    let at = inst.updated_at;
    drop(guard);
    state.publish(InstanceEvent::StatusChanged {
        instance_id: id.to_string(),
        status: InstanceStatus::Stopped,
        at,
    });
    let _ = state.persist().await;
}

pub async fn spec_yaml(state: &AppState, id: &str) -> anyhow::Result<String> {
    let guard = state.instances.read().await;
    let inst = guard
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    serde_yaml::to_string(&InstanceManifest::from_instance(inst))
        .map_err(|e| anyhow::anyhow!("yaml: {e}"))
}

pub async fn apply_spec_body(
    state: &AppState,
    id: &str,
    body: &str,
) -> anyhow::Result<InstanceView> {
    let mut manifest = parse_manifest(id, body)?;
    if manifest.id.is_empty() {
        manifest.id = id.to_string();
    }
    if manifest.id != id {
        anyhow::bail!("manifest id 与 URL 不一致");
    }
    if manifest.spec.node_id.is_empty() {
        manifest.spec.node_id = "local".into();
    }
    if !cluster::node_exists(state, &manifest.spec.node_id).await {
        anyhow::bail!("节点不存在：{}", manifest.spec.node_id);
    }
    ensure_port_free(state, manifest.spec.port, &manifest.spec.node_id, Some(id)).await?;

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    instance.spec = manifest.spec;
    instance.generation = instance.generation.saturating_add(1);
    instance.updated_at = Utc::now();
    let desired = instance.spec.desired_running;
    let node_id = instance.spec.node_id.clone();
    let snap = ApplyInstance::from(&*instance);
    let view = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit(
        "instance.apply",
        Some(id),
        json!({ "generation": view.generation }),
        "api",
    );

    if is_local_node(&node_id) {
        if desired {
            start_instance(state, id).await?;
        } else {
            stop_instance(state, id).await?;
        }
    } else {
        cluster::send_down(
            state,
            &node_id,
            AgentDown::Apply {
                instance: snap,
                seq: 0,
            },
        )
        .await?;
    }
    get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))
}

fn parse_manifest(id: &str, body: &str) -> anyhow::Result<InstanceManifest> {
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        if let Ok(m) = serde_json::from_str::<InstanceManifest>(trimmed) {
            return Ok(m);
        }
        let spec: InstanceSpec = serde_json::from_str(trimmed)?;
        return Ok(InstanceManifest {
            api_version: "cocktail.mc/v1".into(),
            kind: "Instance".into(),
            id: id.into(),
            spec,
        });
    }
    if let Ok(m) = serde_yaml::from_str::<InstanceManifest>(trimmed) {
        return Ok(m);
    }
    let spec: InstanceSpec = serde_yaml::from_str(trimmed)?;
    Ok(InstanceManifest {
        api_version: "cocktail.mc/v1".into(),
        kind: "Instance".into(),
        id: id.into(),
        spec,
    })
}

pub async fn list_files(state: &AppState, id: &str, path: &str) -> anyhow::Result<Vec<FileEntry>> {
    let workdir = workdir_of(state, id).await?;
    files::list_files(&workdir, path)
}

pub async fn read_file(state: &AppState, id: &str, path: &str) -> anyhow::Result<FileContent> {
    let workdir = workdir_of(state, id).await?;
    files::read_file(&workdir, path)
}

pub async fn read_bytes(
    state: &AppState,
    id: &str,
    path: &str,
) -> anyhow::Result<(String, Vec<u8>)> {
    let workdir = workdir_of(state, id).await?;
    files::read_bytes(&workdir, path)
}

pub async fn write_file(
    state: &AppState,
    id: &str,
    path: &str,
    content: &str,
) -> anyhow::Result<FileContent> {
    let workdir = workdir_of(state, id).await?;
    let out = files::write_file(&workdir, path, content)?;
    util::audit("file.write", Some(id), json!({ "path": path }), "api");
    Ok(out)
}

pub async fn write_bytes(
    state: &AppState,
    id: &str,
    path: &str,
    bytes: &[u8],
) -> anyhow::Result<FileEntry> {
    let workdir = workdir_of(state, id).await?;
    let out = files::write_bytes(&workdir, path, bytes)?;
    util::audit(
        "file.upload",
        Some(id),
        json!({ "path": path, "size": bytes.len() }),
        "api",
    );
    Ok(out)
}

pub async fn delete_file(state: &AppState, id: &str, path: &str) -> anyhow::Result<()> {
    let workdir = workdir_of(state, id).await?;
    files::delete_path(&workdir, path)?;
    util::audit("file.delete", Some(id), json!({ "path": path }), "api");
    Ok(())
}

pub async fn mkdir(state: &AppState, id: &str, path: &str) -> anyhow::Result<FileEntry> {
    let workdir = workdir_of(state, id).await?;
    let out = files::mkdir(&workdir, path)?;
    util::audit("file.mkdir", Some(id), json!({ "path": path }), "api");
    Ok(out)
}

pub async fn install_local_jar(
    state: &AppState,
    id: &str,
    jar_path: &str,
    bytes: &[u8],
    core: Option<String>,
    accept_eula: bool,
) -> anyhow::Result<InstanceView> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("stop the instance before installing a jar");
    }

    let jar_rel = {
        let p = jar_path.trim().trim_start_matches(['/', '\\']);
        if p.is_empty() {
            "server.jar".to_string()
        } else if p.to_ascii_lowercase().ends_with(".jar") {
            p.replace('\\', "/")
        } else {
            format!("{}/server.jar", p.trim_end_matches('/'))
        }
    };
    if bytes.is_empty() {
        anyhow::bail!("jar file is empty");
    }

    files::write_bytes(&view.spec.workdir, &jar_rel, bytes)?;
    let (command, args) = util::java_jar_startup(&jar_rel);

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    instance.spec.command = Some(command);
    instance.spec.args = args;
    instance.spec.core = core
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| "custom".into());
    if accept_eula {
        instance.spec.eula_accepted = true;
        util::write_eula(&instance.spec.workdir, true)?;
    }
    instance.updated_at = Utc::now();
    let out = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit(
        "jar.install",
        Some(id),
        json!({ "path": jar_rel, "size": bytes.len() }),
        "api",
    );
    Ok(out)
}

pub async fn set_startup_jar(
    state: &AppState,
    id: &str,
    jar_path: &str,
) -> anyhow::Result<InstanceView> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("stop the instance before changing startup");
    }
    let jar_rel = jar_path
        .trim()
        .trim_start_matches(['/', '\\'])
        .replace('\\', "/");
    if jar_rel.is_empty() || !jar_rel.to_ascii_lowercase().ends_with(".jar") {
        anyhow::bail!("jar_path must end with .jar");
    }
    if !files::jar_exists(&view.spec.workdir, &jar_rel) {
        anyhow::bail!("jar not found: {jar_rel}");
    }
    let (command, args) = util::java_jar_startup(&jar_rel);
    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    instance.spec.command = Some(command);
    instance.spec.args = args;
    if instance.spec.core == "demo" {
        instance.spec.core = "custom".into();
    }
    instance.updated_at = Utc::now();
    let out = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit(
        "jar.set_startup",
        Some(id),
        json!({ "path": jar_rel }),
        "api",
    );
    Ok(out)
}

pub async fn create_backup(state: &AppState, id: &str) -> anyhow::Result<BackupInfo> {
    let workdir = workdir_of(state, id).await?;
    let bak = files::create_backup(id, &workdir)?;
    let keep = get_instance(state, id)
        .await
        .map(|v| v.spec.backup_keep.max(1))
        .unwrap_or(7);
    let _ = files::prune_backups(id, keep);
    util::audit("backup.create", Some(id), json!({ "id": bak.id }), "api");
    Ok(bak)
}

pub async fn list_backups(state: &AppState, id: &str) -> anyhow::Result<Vec<BackupInfo>> {
    if get_instance(state, id).await.is_none() {
        anyhow::bail!("instance not found");
    }
    files::list_backups(id)
}

pub async fn delete_backup(state: &AppState, id: &str, backup_id: &str) -> anyhow::Result<()> {
    if get_instance(state, id).await.is_none() {
        anyhow::bail!("instance not found");
    }
    files::delete_backup(id, backup_id)?;
    util::audit("backup.delete", Some(id), json!({ "id": backup_id }), "api");
    Ok(())
}

pub async fn restore_backup(state: &AppState, id: &str, backup_id: &str) -> anyhow::Result<()> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("stop the instance before restore");
    }
    files::restore_backup(id, backup_id, &view.spec.workdir)?;
    util::audit(
        "backup.restore",
        Some(id),
        json!({ "id": backup_id }),
        "api",
    );
    Ok(())
}

pub async fn get_properties(state: &AppState, id: &str) -> anyhow::Result<Vec<PropertyEntry>> {
    let workdir = workdir_of(state, id).await?;
    let path = PathBuf::from(&workdir).join("server.properties");
    Ok(util::read_properties(&path)?
        .into_iter()
        .map(|(key, value)| PropertyEntry { key, value })
        .collect())
}

pub async fn set_properties(
    state: &AppState,
    id: &str,
    entries: &[PropertyEntry],
) -> anyhow::Result<Vec<PropertyEntry>> {
    let workdir = workdir_of(state, id).await?;
    let path = PathBuf::from(&workdir).join("server.properties");
    let pairs: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.key.clone(), e.value.clone()))
        .collect();
    util::write_properties(&path, &pairs)?;
    if let Some(port) = entries.iter().find(|e| e.key == "server-port") {
        if let Ok(p) = port.value.parse::<u16>() {
            let mut guard = state.instances.write().await;
            if let Some(inst) = guard.get_mut(id) {
                inst.spec.port = p;
            }
            drop(guard);
            let _ = state.persist().await;
        }
    }
    get_properties(state, id).await
}

pub async fn list_plugins(state: &AppState, id: &str) -> anyhow::Result<Vec<PluginInfo>> {
    let workdir = workdir_of(state, id).await?;
    let mut out = Vec::new();
    for folder in ["plugins", "mods"] {
        let dir = PathBuf::from(&workdir).join(folder);
        std::fs::create_dir_all(&dir)?;
        for ent in std::fs::read_dir(dir)? {
            let ent = ent?;
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".jar") && !name.ends_with(".jar.disabled") {
                continue;
            }
            let enabled = name.ends_with(".jar") && !name.ends_with(".jar.disabled");
            out.push(PluginInfo {
                name: name.clone(),
                path: format!("{folder}/{name}"),
                size: ent.metadata()?.len(),
                enabled,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

pub async fn set_plugin_enabled(
    state: &AppState,
    id: &str,
    name: &str,
    enabled: bool,
) -> anyhow::Result<PluginInfo> {
    let workdir = workdir_of(state, id).await?;
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_string();

    let mut current = None;
    let mut folder = "plugins";
    for f in ["plugins", "mods"] {
        let candidate = PathBuf::from(&workdir).join(f).join(&base);
        if candidate.exists() {
            current = Some(candidate);
            folder = f;
            break;
        }
    }
    let current = current.ok_or_else(|| anyhow::anyhow!("plugin not found"))?;
    let root = PathBuf::from(&workdir).join(folder);

    let dest = if enabled {
        let n = base.trim_end_matches(".disabled").to_string();
        root.join(n)
    } else if base.ends_with(".disabled") {
        current.clone()
    } else {
        root.join(format!("{base}.disabled"))
    };

    if current != dest {
        std::fs::rename(&current, &dest)?;
    }
    let meta = std::fs::metadata(&dest)?;
    let final_name = dest.file_name().unwrap().to_string_lossy().into_owned();
    Ok(PluginInfo {
        name: final_name.clone(),
        path: format!("{folder}/{final_name}"),
        size: meta.len(),
        enabled: !final_name.ends_with(".disabled"),
    })
}

pub async fn install_modrinth(
    state: &AppState,
    id: &str,
    req: super::modrinth::InstallModrinthRequest,
) -> anyhow::Result<super::modrinth::InstallResult> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    let version = super::modrinth::pick_version(&req).await?;
    if version.primary_url.is_empty() {
        anyhow::bail!("no download URL");
    }
    let project_type = req.project_type.as_deref().unwrap_or("");
    let target =
        super::modrinth::infer_target(project_type, &version.loaders, req.target.as_deref());
    let filename = version.primary_filename.clone();
    let safe_name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("modrinth.jar")
        .to_string();
    if !safe_name.to_ascii_lowercase().ends_with(".jar")
        && !safe_name.to_ascii_lowercase().ends_with(".zip")
    {
        anyhow::bail!("unsupported file type from Modrinth: {safe_name}");
    }

    let bytes = super::modrinth::download_bytes(&version.primary_url, &safe_name).await?;
    let rel = format!("{target}/{safe_name}");
    files::write_bytes(&view.spec.workdir, &rel, &bytes)?;
    util::audit(
        "modrinth.install",
        Some(id),
        json!({
            "project_id": req.project_id,
            "version_id": version.id,
            "path": rel,
            "size": bytes.len(),
        }),
        "api",
    );
    Ok(super::modrinth::InstallResult {
        path: rel,
        filename: safe_name,
        size: bytes.len() as u64,
        project_id: req.project_id,
        version_id: version.id,
        version_number: version.version_number,
        target,
    })
}

pub async fn install_hangar(
    state: &AppState,
    id: &str,
    req: super::hangar::InstallRequest,
) -> anyhow::Result<super::hangar::InstallResult> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    let version = super::hangar::pick_version(&req).await?;
    let safe_name = version
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("hangar.jar")
        .to_string();
    let bytes = super::hangar::download_bytes(&version.download_url, &safe_name).await?;
    let rel = format!("plugins/{safe_name}");
    files::write_bytes(&view.spec.workdir, &rel, &bytes)?;
    util::audit(
        "hangar.install",
        Some(id),
        json!({
            "slug": req.slug,
            "version": version.name,
            "path": rel,
            "size": bytes.len(),
        }),
        "api",
    );
    Ok(super::hangar::InstallResult {
        path: rel,
        filename: safe_name,
        size: bytes.len() as u64,
        slug: req.slug,
        version: version.name,
        platform: version.platform,
    })
}

pub async fn install_spiget(
    state: &AppState,
    id: &str,
    req: super::spiget::InstallRequest,
) -> anyhow::Result<super::spiget::InstallResult> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    let (bytes, filename, version_id, version_name) =
        super::spiget::download_resource(&req).await?;
    let safe_name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("spiget.jar")
        .to_string();
    let rel = format!("plugins/{safe_name}");
    files::write_bytes(&view.spec.workdir, &rel, &bytes)?;
    util::audit(
        "spiget.install",
        Some(id),
        json!({
            "resource_id": req.resource_id,
            "version_id": version_id,
            "path": rel,
            "size": bytes.len(),
        }),
        "api",
    );
    Ok(super::spiget::InstallResult {
        path: rel,
        filename: safe_name,
        size: bytes.len() as u64,
        resource_id: req.resource_id,
        version_id,
        version_name,
    })
}

pub async fn list_schedules(state: &AppState) -> Vec<Schedule> {
    state.schedules.read().await.clone()
}

pub async fn create_schedule(
    state: &AppState,
    req: CreateScheduleRequest,
) -> anyhow::Result<Schedule> {
    if get_instance(state, &req.instance_id).await.is_none() {
        anyhow::bail!("instance not found");
    }
    if req.every_secs < 30 {
        anyhow::bail!("every_secs must be >= 30");
    }
    let schedule = Schedule {
        id: Uuid::new_v4().to_string(),
        instance_id: req.instance_id,
        kind: req.kind,
        every_secs: req.every_secs,
        command: req.command,
        enabled: req.enabled,
        next_run_at: Utc::now() + Duration::seconds(req.every_secs as i64),
    };
    state.schedules.write().await.push(schedule.clone());
    let _ = state.persist().await;
    Ok(schedule)
}

pub async fn delete_schedule(state: &AppState, id: &str) -> anyhow::Result<()> {
    let mut guard = state.schedules.write().await;
    let before = guard.len();
    guard.retain(|s| s.id != id);
    if guard.len() == before {
        anyhow::bail!("schedule not found");
    }
    drop(guard);
    let _ = state.persist().await;
    Ok(())
}

pub async fn apply_event(state: &std::sync::Arc<AppState>, event: &InstanceEvent) {
    match event {
        InstanceEvent::StatusChanged {
            instance_id,
            status,
            at,
        } => {
            let mut should_restart = false;
            let mut webhook: Option<(Option<String>, String, String)> = None;
            {
                let mut guard = state.instances.write().await;
                if let Some(inst) = guard.get_mut(instance_id) {
                    if !status.can_apply_over(inst.status) {
                        return;
                    }
                    inst.status = *status;
                    inst.updated_at = *at;
                    if *status == InstanceStatus::Crashed && inst.spec.auto_restart {
                        should_restart = true;
                    }
                    if matches!(*status, InstanceStatus::Stopped | InstanceStatus::Crashed) {
                        inst.process = None;
                        inst.last_pid = None;
                        inst.last_start_time = None;
                        if *status != InstanceStatus::Stopping {
                            inst.docker_container = None;
                        }
                    }
                    if *status == InstanceStatus::Crashed {
                        webhook = Some((
                            inst.spec.webhook_url.clone(),
                            inst.id.clone(),
                            inst.spec.name.clone(),
                        ));
                    }
                }
            }
            let _ = state.persist().await;
            if let Some((inst_url, id, name)) = webhook {
                let url = match inst_url {
                    Some(u) if !u.is_empty() => Some(u),
                    _ => state.effective_webhook().await,
                };
                if let Some(url) = url {
                    let sid = id.clone();
                    let sname = name.clone();
                    tokio::spawn(async move {
                        util::notify_webhook(&url, &sid, "crashed", &sname).await;
                    });
                }
                let state = std::sync::Arc::clone(state);
                let sid = id.clone();
                let sname = name.clone();
                tokio::spawn(async move {
                    crate::ops::notify_event(
                        &state,
                        "崩溃",
                        &format!("{sname} ({sid}) 进程异常退出"),
                    )
                    .await;
                    crate::automations::on_crash(&state, &sid, &sname).await;
                });
            }
            if should_restart {
                let id = instance_id.clone();
                let state = std::sync::Arc::clone(state);
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Err(e) = start_instance(&state, &id).await {
                        tracing::warn!(error = %e, %id, "auto-restart failed");
                    }
                });
            }
        }
        InstanceEvent::Metric {
            instance_id,
            sample,
        } => {
            let mut guard = state.instances.write().await;
            if let Some(inst) = guard.get_mut(instance_id) {
                inst.last_metrics = Some(sample.clone());
                inst.updated_at = sample.ts;
            }
            drop(guard);
            let mut hist = state.metric_history.write().await;
            let buf = hist
                .entry(instance_id.clone())
                .or_insert_with(std::collections::VecDeque::new);
            buf.push_back(sample.clone());
            while buf.len() > crate::state::METRIC_BUFFER {
                buf.pop_front();
            }
        }
        InstanceEvent::Log { instance_id, line } => {
            util::append_instance_log(instance_id, &line.stream, &line.line);
            super::players::ingest_line(state, instance_id, &line.line).await;
            if let Some(names) = super::players::parse_online_players(&line.line) {
                let mut guard = state.instances.write().await;
                if let Some(inst) = guard.get_mut(instance_id) {
                    inst.last_players = names;
                }
            }
        }
        InstanceEvent::DownloadProgress { .. } => {}
    }
}

pub async fn install_core(
    state: &AppState,
    id: &str,
    req: super::versions::InstallRequest,
) -> anyhow::Result<InstanceView> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("stop the instance before installing a core");
    }

    let (command, args) = super::versions::download_and_install(
        &view.spec.workdir,
        &req.core,
        &req.version,
        req.loader.as_deref(),
    )
    .await?;

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    instance.spec.command = Some(command);
    instance.spec.args = args;
    instance.spec.core = req.core.clone();
    instance.spec.mc_version = Some(req.version.clone());
    instance.updated_at = Utc::now();
    let out = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit(
        "core.install",
        Some(id),
        json!({ "core": req.core, "version": req.version, "loader": req.loader }),
        "api",
    );
    Ok(out)
}

pub async fn list_players(state: &AppState, id: &str) -> anyhow::Result<Vec<PlayerInfo>> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    Ok(super::players::list_enriched(state, id, &view.last_players).await)
}

pub async fn probe_players(state: &AppState, id: &str) -> anyhow::Result<Vec<PlayerInfo>> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if view.status == InstanceStatus::Running
        && view.last_metrics.as_ref().and_then(|m| m.tps).is_some()
    {
        let _ = send_command(
            state,
            id,
            CommandRequest {
                command: "list".into(),
            },
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    list_players(state, id).await
}

pub async fn player_action(
    state: &AppState,
    id: &str,
    name: &str,
    action: &str,
    reason: Option<String>,
) -> anyhow::Result<()> {
    let cmd = match action {
        "kick" => {
            let r = reason.unwrap_or_else(|| "Kicked by Cocktail Manager".into());
            format!("kick {name} {r}")
        }
        "ban" => {
            let r = reason.unwrap_or_else(|| "Banned by Cocktail Manager".into());
            format!("ban {name} {r}")
        }
        "pardon" => format!("pardon {name}"),
        "op" => format!("op {name}"),
        "deop" => format!("deop {name}"),
        "whitelist" => format!("whitelist add {name}"),
        "unwhitelist" => format!("whitelist remove {name}"),
        other => anyhow::bail!("unknown action: {other}"),
    };
    send_command(state, id, CommandRequest { command: cmd }).await?;
    util::audit(
        "player.action",
        Some(id),
        json!({ "player": name, "action": action }),
        "api",
    );
    Ok(())
}

pub async fn list_worlds(
    state: &AppState,
    id: &str,
) -> anyhow::Result<Vec<super::worlds::WorldInfo>> {
    let workdir = workdir_of(state, id).await?;
    super::worlds::list_worlds(&workdir)
}

pub async fn reset_world(state: &AppState, id: &str, world: &str) -> anyhow::Result<()> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("stop the instance before resetting a world");
    }
    super::worlds::reset_world(&view.spec.workdir, world)?;
    util::audit("world.reset", Some(id), json!({ "world": world }), "api");
    Ok(())
}

pub async fn export_world(state: &AppState, id: &str, world: &str) -> anyhow::Result<BackupInfo> {
    let workdir = workdir_of(state, id).await?;
    let bak = super::worlds::export_world(id, &workdir, world)?;
    util::audit("world.export", Some(id), json!({ "world": world }), "api");
    Ok(bak)
}

pub async fn import_world(
    state: &AppState,
    id: &str,
    world: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("stop the instance before importing a world");
    }
    super::worlds::import_world(&view.spec.workdir, world, bytes)?;
    util::audit("world.import", Some(id), json!({ "world": world }), "api");
    Ok(())
}

async fn workdir_of(state: &AppState, id: &str) -> anyhow::Result<String> {
    let inst = state
        .instances
        .read()
        .await
        .get(id)
        .map(|i| (i.spec.workdir.clone(), i.spec.node_id.clone()))
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if !is_local_node(&inst.1) {
        anyhow::bail!("远程节点上的文件/备份请在该节点本机处理；控制面当前仅代理本机工作目录");
    }
    Ok(inst.0)
}

async fn ensure_port_free(
    state: &AppState,
    port: u16,
    node_id: &str,
    except_id: Option<&str>,
) -> anyhow::Result<()> {
    let guard = state.instances.read().await;
    for inst in guard.values() {
        if Some(inst.id.as_str()) == except_id {
            continue;
        }
        let same_node = inst.spec.node_id == node_id
            || (is_local_node(&inst.spec.node_id) && is_local_node(node_id));
        if !same_node {
            continue;
        }
        if inst.spec.port == port
            && matches!(
                inst.status,
                InstanceStatus::Running | InstanceStatus::Starting
            )
        {
            anyhow::bail!(
                "port {port} already in use by instance '{}'",
                inst.spec.name
            );
        }
        if inst.spec.port == port && except_id.is_some() {
            continue;
        }
        if inst.spec.port == port {
            anyhow::bail!(
                "port {port} already assigned to instance '{}'",
                inst.spec.name
            );
        }
    }
    Ok(())
}

async fn ensure_exclusive_workdir(
    state: &AppState,
    workdir: &str,
    except_id: Option<&str>,
) -> anyhow::Result<()> {
    let guard = state.instances.read().await;
    for inst in guard.values() {
        if except_id.is_some_and(|id| inst.id == id) {
            continue;
        }
        if files::workdirs_conflict(workdir, &inst.spec.workdir) {
            anyhow::bail!(
                "文件根 '{}' 已被实例 '{}' 占用，每个杯子必须独占目录",
                workdir,
                inst.spec.name
            );
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn run_due_schedules(state: &std::sync::Arc<AppState>) {
    let now = Utc::now();
    let due: Vec<Schedule> = {
        let guard = state.schedules.read().await;
        guard
            .iter()
            .filter(|s| s.enabled && s.next_run_at <= now)
            .cloned()
            .collect()
    };
    for sched in due {
        match sched.kind {
            ScheduleKind::Backup => {
                let _ = create_backup(state, &sched.instance_id).await;
                let keep = get_instance(state, &sched.instance_id)
                    .await
                    .map(|v| v.spec.backup_keep.max(1))
                    .unwrap_or(7);
                let _ = files::prune_backups(&sched.instance_id, keep);
            }
            ScheduleKind::Restart => {
                let _ = restart_instance(state, &sched.instance_id).await;
            }
            ScheduleKind::Command => {
                if let Some(cmd) = &sched.command {
                    let _ = send_command(
                        state,
                        &sched.instance_id,
                        CommandRequest {
                            command: cmd.clone(),
                        },
                    )
                    .await;
                }
            }
        }
        let mut guard = state.schedules.write().await;
        if let Some(s) = guard.iter_mut().find(|s| s.id == sched.id) {
            s.next_run_at = Utc::now() + Duration::seconds(s.every_secs as i64);
        }
        drop(guard);
        let _ = state.persist().await;
    }
}

pub async fn docker_engine_status() -> super::container::DockerStatus {
    super::container::docker_status().await
}

pub async fn docker_list_images() -> anyhow::Result<Vec<super::container::DockerImage>> {
    super::container::list_images().await
}

pub async fn docker_pull_image(image: &str) -> anyhow::Result<()> {
    super::container::pull_image(image).await
}

pub async fn player_history(state: &AppState, id: &str) -> anyhow::Result<Vec<PlayerInfo>> {
    let _ = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    Ok(super::players::history(state, id).await)
}

pub async fn fleet_summary(state: &AppState) -> FleetSummary {
    let list = list_instances(state).await;
    let mut running = 0;
    let mut stopped = 0;
    let mut starting = 0;
    let mut crashed = 0;
    let mut groups: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut runtimes: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for inst in &list {
        match inst.status {
            InstanceStatus::Running => running += 1,
            InstanceStatus::Starting | InstanceStatus::Stopping => starting += 1,
            InstanceStatus::Crashed => crashed += 1,
            InstanceStatus::Created | InstanceStatus::Stopped => stopped += 1,
        }
        let g = inst.spec.group.clone().unwrap_or_else(|| "default".into());
        *groups.entry(g).or_default() += 1;
        let rt = match inst.spec.runtime {
            RuntimeKind::Docker => "docker",
            RuntimeKind::Podman => "podman",
            RuntimeKind::Process => "process",
        };
        *runtimes.entry(rt.into()).or_default() += 1;
    }

    FleetSummary {
        total: list.len(),
        running,
        stopped,
        starting,
        crashed,
        by_group: groups
            .into_iter()
            .map(|(group, count)| GroupCount { group, count })
            .collect(),
        by_runtime: runtimes
            .into_iter()
            .map(|(runtime, count)| RuntimeCount { runtime, count })
            .collect(),
        docker: super::container::docker_status().await,
    }
}

pub async fn clone_instance(
    state: &AppState,
    id: &str,
    req: CloneInstanceRequest,
) -> anyhow::Result<InstanceView> {
    let src = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    let src_workdir = src.spec.workdir.clone();
    let new_id = Uuid::new_v4().to_string();
    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} 副本", src.spec.name));
    let node_id = req
        .node_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| src.spec.node_id.clone());
    if !cluster::node_exists(state, &node_id).await {
        anyhow::bail!("节点不存在：{node_id}");
    }
    let port = match req.port {
        Some(p) => p,
        None => next_free_port(state, &node_id, src.spec.port).await?,
    };
    ensure_port_free(state, port, &node_id, None).await?;
    let workdir = req
        .workdir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| files::default_instance_root(&new_id));
    ensure_exclusive_workdir(state, &workdir, None).await?;

    let copy_data = req.copy_data.unwrap_or(true);
    let src_dir = src_workdir.clone();
    let dst_dir = workdir.clone();
    let skip_logs = req.skip_logs.unwrap_or(true);
    tokio::task::spawn_blocking(move || {
        files::copy_instance_tree(&src_dir, &dst_dir, copy_data, skip_logs)
    })
    .await
    .map_err(|e| anyhow::anyhow!("复制任务失败：{e}"))??;

    if is_local_node(&node_id) {
        files::ensure_seed_files(&workdir, port, src.spec.eula_accepted)?;
        files::sync_port(&workdir, port)?;
    }

    let mut spec = src.spec.clone();
    spec.name = name;
    spec.workdir = workdir;
    spec.port = port;
    spec.node_id = node_id;
    spec.desired_running = false;
    let instance = Instance::with_id(new_id.clone(), spec);
    let view = instance.public_view();
    state
        .instances
        .write()
        .await
        .insert(new_id.clone(), instance);
    state.publish(InstanceEvent::StatusChanged {
        instance_id: new_id.clone(),
        status: InstanceStatus::Created,
        at: Utc::now(),
    });
    let _ = state.persist().await;
    let _ = crate::netops::try_apply(state).await;
    util::audit(
        "instance.clone",
        Some(&new_id),
        json!({ "from": id, "port": port, "copy_data": copy_data }),
        "api",
    );
    Ok(view)
}

async fn next_free_port(state: &AppState, node_id: &str, from: u16) -> anyhow::Result<u16> {
    let guard = state.instances.read().await;
    let used: Vec<u16> = guard
        .values()
        .filter(|i| {
            i.spec.node_id == node_id || (is_local_node(&i.spec.node_id) && is_local_node(node_id))
        })
        .map(|i| i.spec.port)
        .collect();
    drop(guard);
    let mut candidate = from.saturating_add(1).max(25566);
    for _ in 0..2000 {
        if !used.contains(&candidate) && !port_in_use(candidate) {
            return Ok(candidate);
        }
        candidate = candidate.saturating_add(1);
        if candidate == 0 {
            candidate = 25566;
        }
    }
    anyhow::bail!("找不到可用端口，请手动指定")
}

pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_err()
}

pub fn free_disk_bytes(path: &str) -> Option<u64> {
    let target = std::path::Path::new(path);
    let probe = if target.exists() {
        target.to_path_buf()
    } else {
        target
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    };
    let out = std::process::Command::new("df")
        .arg("-Pk")
        .arg(&probe)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let avail_kb: u64 = cols.get(3)?.parse().ok()?;
    Some(avail_kb.saturating_mul(1024))
}

fn dir_size_bytes(path: &str) -> u64 {
    fn walk(dir: &std::path::Path, total: &mut u64, depth: u32) {
        if depth > 24 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_symlink() {
                continue;
            }
            if p.is_dir() {
                walk(&p, total, depth + 1);
            } else if let Ok(meta) = p.metadata() {
                *total += meta.len();
            }
        }
    }
    let mut total = 0u64;
    walk(std::path::Path::new(path), &mut total, 0);
    total
}

pub async fn preflight_start(state: &AppState, id: &str) -> anyhow::Result<Vec<String>> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    let mut warnings = Vec::new();
    if view.spec.runtime == RuntimeKind::Process && !is_local_node(&view.spec.node_id) {
        warnings.push("远程节点实例的资源预检在 agent 侧执行".to_string());
        return Ok(warnings);
    }
    if view.spec.runtime == RuntimeKind::Process && port_in_use(view.spec.port) {
        let ours = {
            let guard = state.instances.read().await;
            guard
                .values()
                .filter(|i| i.spec.port == view.spec.port && i.id != view.id)
                .count()
        };
        if ours == 0 {
            warnings.push(format!(
                "端口 {} 已被本机其他进程占用，启动可能失败",
                view.spec.port
            ));
        }
    }
    let need = view.spec.memory_mib as u64 * 1024 * 1024;
    let workdir = view.spec.workdir.clone();
    let (free, used) =
        tokio::task::spawn_blocking(move || (free_disk_bytes(&workdir), dir_size_bytes(&workdir)))
            .await
            .unwrap_or((None, 0));
    if let Some(free) = free {
        if free < need {
            warnings.push(format!(
                "磁盘可用 {} MiB，低于实例内存上限 {} MiB，备份可能失败",
                free / (1024 * 1024),
                view.spec.memory_mib
            ));
        } else if free < need * 2 {
            warnings.push(format!(
                "磁盘可用 {} MiB 偏紧（当前工作目录已占 {} MiB）",
                free / (1024 * 1024),
                used / (1024 * 1024)
            ));
        }
    }
    if !view.spec.eula_accepted && view.spec.core != "demo" {
        warnings.push("EULA 尚未接受".to_string());
    }
    if view.spec.command.is_none() && !files::jar_exists(&view.spec.workdir, "server.jar") {
        warnings.push("未配置启动命令且找不到 server.jar".to_string());
    }
    Ok(warnings)
}

pub async fn detect_mc_version(state: &AppState, id: &str) -> anyhow::Result<Option<String>> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if !is_local_node(&view.spec.node_id) {
        return Ok(None);
    }
    let found = files::guess_mc_version(&view.spec.workdir);
    if let Some(v) = found.as_deref() {
        let mut guard = state.instances.write().await;
        if let Some(inst) = guard.get_mut(id) {
            if inst.spec.mc_version.as_deref() != Some(v) {
                inst.spec.mc_version = Some(v.to_string());
                inst.updated_at = Utc::now();
            }
        }
        drop(guard);
        let _ = state.persist().await;
    }
    Ok(found)
}

pub async fn preflight_report(state: &AppState, id: &str) -> anyhow::Result<PreflightReport> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    let warnings = preflight_start(state, id).await?;
    let workdir = view.spec.workdir.clone();
    let (free, used) =
        tokio::task::spawn_blocking(move || (free_disk_bytes(&workdir), dir_size_bytes(&workdir)))
            .await
            .unwrap_or((None, 0));
    let port_busy = view.spec.runtime == RuntimeKind::Process && port_in_use(view.spec.port);
    Ok(PreflightReport {
        instance_id: view.id,
        warnings,
        free_bytes: free,
        used_bytes: used,
        port_busy,
    })
}

pub async fn backup_preview(
    state: &AppState,
    id: &str,
    backup_id: &str,
) -> anyhow::Result<RestorePreview> {
    let workdir = workdir_of(state, id).await?;
    let meta = files::backup_meta(id, backup_id)?;
    let path = files::backup_path(id, backup_id)?;
    let target = path.clone();
    let scan = tokio::task::spawn_blocking(move || files::inspect_backup_zip(&target))
        .await
        .map_err(|e| anyhow::anyhow!("读取备份失败：{e}"))??;
    let mut warnings = Vec::new();
    if !scan.has_server_properties {
        warnings.push("备份内没有 server.properties，恢复后端口与配置可能被重置".to_string());
    }
    if !scan.has_level_dat {
        warnings.push("备份内没有 level.dat，可能不是完整的世界存档".to_string());
    }
    if scan.plugin_count == 0 {
        warnings.push("备份内没有插件，恢复后功能可能缺失".to_string());
    }
    let current = files::total_dir_bytes(&workdir);
    if current > scan.size_bytes.saturating_mul(4).max(64 * 1024 * 1024) {
        warnings.push(format!(
            "当前目录 {} MiB 明显大于备份 {} MiB，恢复会丢弃新增内容",
            current / (1024 * 1024),
            scan.size_bytes / (1024 * 1024)
        ));
    }
    Ok(RestorePreview {
        backup_id: backup_id.to_string(),
        size_bytes: meta.size_bytes,
        created_at: meta.created_at.to_rfc3339(),
        entries: scan.entries,
        world_size_bytes: scan.world_bytes,
        plugin_count: scan.plugin_count,
        has_server_properties: scan.has_server_properties,
        has_level_dat: scan.has_level_dat,
        warnings,
    })
}

pub async fn version_compare(state: &AppState, id: &str) -> anyhow::Result<VersionCompare> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    let current = view.spec.mc_version.clone();
    let core = view.spec.core.clone();
    if core == "custom" || core == "demo" {
        return Ok(VersionCompare {
            current,
            latest: None,
            behind: false,
            note: "自定义核心不参与版本比对".into(),
        });
    }
    let versions = super::versions::list_versions(&core)
        .await
        .unwrap_or_default();
    let latest = versions
        .iter()
        .find(|v| v.latest)
        .map(|v| v.id.clone())
        .or_else(|| versions.first().map(|v| v.id.clone()));
    let behind = match (current.as_deref(), latest.as_deref()) {
        (Some(c), Some(l)) => c != l,
        (None, Some(_)) => true,
        _ => false,
    };
    let note = match (current.as_deref(), latest.as_deref()) {
        (Some(c), Some(l)) if c == l => format!("已是最新（{c}）"),
        (Some(c), Some(l)) => format!("本机 {c}，仓库最新 {l}"),
        (None, Some(l)) => format!("未记录版本，仓库最新 {l}"),
        _ => "无法获取远端版本列表（检查代理/网络）".into(),
    };
    Ok(VersionCompare {
        current,
        latest,
        behind,
        note,
    })
}

pub async fn world_download(
    state: &AppState,
    id: &str,
    world: &str,
) -> anyhow::Result<(String, std::path::PathBuf)> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if !is_local_node(&view.spec.node_id) {
        anyhow::bail!("远程节点的世界导出请在该节点本机执行");
    }
    let safe = world
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let dir = std::path::PathBuf::from("data").join("backups").join(id);
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("{safe}-{stamp}.zip");
    let dest = dir.join(&filename);
    let workdir = view.spec.workdir.clone();
    let rel = world.to_string();
    let dest_clone = dest.clone();
    tokio::task::spawn_blocking(move || files::pack_subdir_zip(&workdir, &rel, &dest_clone))
        .await
        .map_err(|e| anyhow::anyhow!("打包失败：{e}"))??;
    util::audit(
        "world.download",
        Some(id),
        json!({ "world": world, "file": filename }),
        "api",
    );
    Ok((filename, dest))
}

pub async fn world_upload(
    state: &AppState,
    id: &str,
    world: &str,
    bytes: &[u8],
) -> anyhow::Result<u32> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("请先停止实例再导入世界");
    }
    if !is_local_node(&view.spec.node_id) {
        anyhow::bail!("远程节点的世界导入请在该节点本机执行");
    }
    let workdir = view.spec.workdir.clone();
    let rel = world.to_string();
    let owned = bytes.to_vec();
    let count =
        tokio::task::spawn_blocking(move || files::extract_zip_into(&workdir, &rel, &owned))
            .await
            .map_err(|e| anyhow::anyhow!("解压失败：{e}"))??;
    util::audit(
        "world.upload",
        Some(id),
        json!({ "world": world, "files": count }),
        "api",
    );
    Ok(count)
}

pub async fn rescan_version(state: &AppState, id: &str) -> anyhow::Result<Option<String>> {
    let found = detect_mc_version(state, id).await?;
    if let Some(v) = found.as_deref() {
        let view = get_instance(state, id).await;
        if let Some(view) = view {
            if is_local_node(&view.spec.node_id) {
                let _ = files::write_mc_version_marker(&view.spec.workdir, v);
            }
        }
    }
    Ok(found)
}

pub async fn prune_instance_backups(
    state: &AppState,
    id: &str,
    keep: u32,
) -> anyhow::Result<usize> {
    if get_instance(state, id).await.is_none() {
        anyhow::bail!("instance not found");
    }
    files::prune_backups(id, keep.max(1))
}

pub async fn bulk_action(state: &AppState, req: BulkActionRequest) -> BulkActionResult {
    let mut ok = Vec::new();
    let mut failed = Vec::new();
    for id in req.ids {
        let result = match req.action.as_str() {
            "start" => start_instance(state, &id).await.map(|_| ()),
            "stop" => stop_instance(state, &id).await.map(|_| ()),
            "restart" => restart_instance(state, &id).await.map(|_| ()),
            "delete" => delete_instance(state, &id).await,
            other => Err(anyhow::anyhow!("unknown bulk action: {other}")),
        };
        match result {
            Ok(()) => ok.push(id),
            Err(e) => failed.push(BulkFailure {
                id,
                error: e.to_string(),
            }),
        }
    }
    util::audit(
        "fleet.bulk",
        None,
        json!({ "action": req.action, "ok": ok.len(), "failed": failed.len() }),
        "api",
    );
    BulkActionResult { ok, failed }
}

#[cfg(test)]
mod tests {
    use super::ensure_port_free;
    use crate::instance::{Instance, InstanceSpec, InstanceStatus, RuntimeKind};
    use crate::state::test_state;

    fn spec(name: &str, port: u16, node_id: &str) -> InstanceSpec {
        InstanceSpec {
            name: name.into(),
            workdir: format!("data/instances/{name}"),
            command: None,
            args: Vec::new(),
            memory_mib: 1024,
            core: "vanilla".into(),
            port,
            auto_restart: false,
            eula_accepted: false,
            webhook_url: None,
            runtime: RuntimeKind::Process,
            docker_image: None,
            cpu_limit: None,
            tags: Vec::new(),
            group: None,
            node_id: node_id.into(),
            desired_running: false,
            backup_keep: 7,
            backup_hour: None,
            java_major: None,
            mc_version: None,
        }
    }

    async fn insert(
        state: &crate::state::SharedState,
        id: &str,
        port: u16,
        status: InstanceStatus,
    ) {
        let mut inst = Instance::with_id(id.into(), spec(id, port, "local"));
        inst.status = status;
        state.instances.write().await.insert(id.into(), inst);
    }

    #[tokio::test]
    async fn port_free_when_no_instances() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        ensure_port_free(&state, 25565, "local", None)
            .await
            .expect("port should be free");
    }

    #[tokio::test]
    async fn port_conflicts_with_assigned_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        insert(&state, "a", 25565, InstanceStatus::Stopped).await;
        let err = ensure_port_free(&state, 25565, "local", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already assigned"), "{err}");
    }

    #[tokio::test]
    async fn port_conflicts_with_running_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        insert(&state, "a", 25565, InstanceStatus::Running).await;
        let err = ensure_port_free(&state, 25565, "local", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already in use"), "{err}");
    }

    #[tokio::test]
    async fn own_port_is_exempt_when_updating() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        insert(&state, "a", 25565, InstanceStatus::Running).await;
        // the instance itself keeps its port during an update
        ensure_port_free(&state, 25565, "local", Some("a"))
            .await
            .expect("self port is exempt");
    }

    #[tokio::test]
    async fn same_port_on_other_node_is_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let mut inst = Instance::with_id("a".into(), spec("a", 25565, "agent-1"));
        inst.status = InstanceStatus::Running;
        state.instances.write().await.insert("a".into(), inst);
        ensure_port_free(&state, 25565, "local", None)
            .await
            .expect("different node, port is free");
    }
}
