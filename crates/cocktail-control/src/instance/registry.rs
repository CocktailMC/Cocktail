use std::path::PathBuf;

use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::state::AppState;
use crate::util;

use super::files;
use super::model::{
    BackupInfo, BulkActionRequest, BulkActionResult, BulkFailure, CommandRequest,
    CreateInstanceRequest, CreateScheduleRequest, EulaRequest, FileContent, FileEntry, FleetSummary,
    GroupCount, Instance, InstanceEvent, InstanceSpec, InstanceStatus, InstanceView, PlayerInfo,
    PluginInfo, PropertyEntry, RuntimeCount, RuntimeKind, Schedule, ScheduleKind,
    UpdateInstanceRequest,
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
    ensure_port_free(state, req.port, None).await?;

    let workdir = req.workdir.unwrap_or_else(|| {
        PathBuf::from("data")
            .join("instances")
            .join(sanitize(&req.name))
            .to_string_lossy()
            .into_owned()
    });

    files::ensure_seed_files(&workdir, req.port, req.eula_accepted)?;

    let docker_image = match req.runtime {
        RuntimeKind::Docker => Some(
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
    };

    let instance = Instance::new(spec);
    let view = instance.public_view();
    let id = instance.id.clone();

    state.instances.write().await.insert(id.clone(), instance);
    state.publish(InstanceEvent::StatusChanged {
        instance_id: id.clone(),
        status: InstanceStatus::Created,
        at: Utc::now(),
    });
    let _ = state.persist().await;
    util::audit("instance.create", Some(&id), json!({ "name": view.spec.name }), "api");

    Ok(view)
}

pub async fn update_instance(
    state: &AppState,
    id: &str,
    req: UpdateInstanceRequest,
) -> anyhow::Result<InstanceView> {
    if let Some(port) = req.port {
        ensure_port_free(state, port, Some(id)).await?;
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
    instance.updated_at = Utc::now();
    let view = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
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
            ..DefaultUpdate::default()
        },
    )
    .await
}

struct DefaultUpdate;
impl DefaultUpdate {
    fn default() -> UpdateInstanceRequest {
        UpdateInstanceRequest {
            name: None,
            memory_mib: None,
            port: None,
            auto_restart: None,
            command: None,
            args: None,
            core: None,
            eula_accepted: None,
            webhook_url: None,
            runtime: None,
            docker_image: None,
            cpu_limit: None,
            tags: None,
            group: None,
        }
    }
}

pub async fn start_instance(state: &AppState, id: &str) -> anyhow::Result<InstanceView> {
    let (early_view, port, needs_eula, eula_ok) = {
        let guard = state.instances.read().await;
        let instance = guard
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

        match instance.status {
            InstanceStatus::Running | InstanceStatus::Starting => {
                return Ok(instance.public_view());
            }
            InstanceStatus::Stopping => {
                anyhow::bail!("instance is stopping");
            }
            _ => {}
        }

        let needs_eula = instance.spec.command.is_some()
            || matches!(
                instance.spec.core.as_str(),
                "paper" | "fabric" | "vanilla" | "spigot" | "purpur"
            );
        let eula_ok =
            instance.spec.eula_accepted || util::eula_is_accepted(&instance.spec.workdir);
        (
            instance.public_view(),
            instance.spec.port,
            needs_eula,
            eula_ok,
        )
    };

    if needs_eula && !eula_ok {
        anyhow::bail!("EULA not accepted; call POST /eula with {{\"accepted\":true}}");
    }
    let _ = early_view;
    ensure_port_free(state, port, Some(id)).await?;

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    if matches!(
        instance.status,
        InstanceStatus::Running | InstanceStatus::Starting
    ) {
        return Ok(instance.public_view());
    }

    instance.status = InstanceStatus::Starting;
    instance.updated_at = Utc::now();
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
    let events = state.events.clone();
    let instance_id = instance.id.clone();

    // Auto-wire java -jar if server.jar exists but command was never set.
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
        } else if runtime == RuntimeKind::Docker
            || matches!(
                instance.spec.core.as_str(),
                "paper" | "vanilla" | "fabric" | "spigot" | "purpur" | "custom"
            )
        {
            anyhow::bail!(
                "未配置启动命令且找不到 server.jar：请先在「版本安装」导入 jar 或下载核心"
            );
        }
    }

    // Docker maps hostPort:25565 — keep in-container listen port at 25565.
    let seed_port = match runtime {
        RuntimeKind::Docker => 25565,
        RuntimeKind::Process => port,
    };
    files::ensure_seed_files(&workdir, seed_port, eula)?;

    state.publish(InstanceEvent::StatusChanged {
        instance_id: instance_id.clone(),
        status: InstanceStatus::Starting,
        at: Utc::now(),
    });

    let handle = match runtime {
        RuntimeKind::Docker => {
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
                events,
            )
            .await?
        }
    };
    instance.process = Some(handle);
    instance.updated_at = Utc::now();
    let view = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit("instance.start", Some(&instance_id), json!({}), "api");
    Ok(view)
}

pub async fn stop_instance(state: &AppState, id: &str) -> anyhow::Result<InstanceView> {
    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

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
            if inst.status == InstanceStatus::Stopping {
                inst.status = InstanceStatus::Stopped;
                inst.updated_at = Utc::now();
            }
            let view = inst.public_view();
            drop(guard);
            let _ = state.persist().await;
            util::audit("instance.stop", Some(id), json!({ "mode": "graceful" }), "api");
            return Ok(view);
        }
        anyhow::bail!("instance not found after stop");
    }

    instance.status = InstanceStatus::Stopped;
    instance.updated_at = Utc::now();
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

pub async fn send_command(
    state: &AppState,
    id: &str,
    req: CommandRequest,
) -> anyhow::Result<()> {
    let cmd = req.command.trim().to_string();
    if cmd.is_empty() {
        anyhow::bail!("command is empty");
    }
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
    util::audit("instance.command", Some(id), json!({ "command": cmd }), "api");
    Ok(())
}

pub async fn list_files(
    state: &AppState,
    id: &str,
    path: &str,
) -> anyhow::Result<Vec<FileEntry>> {
    let workdir = workdir_of(state, id).await?;
    files::list_files(&workdir, path)
}

pub async fn read_file(state: &AppState, id: &str, path: &str) -> anyhow::Result<FileContent> {
    let workdir = workdir_of(state, id).await?;
    files::read_file(&workdir, path)
}

pub async fn read_bytes(state: &AppState, id: &str, path: &str) -> anyhow::Result<(String, Vec<u8>)> {
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

/// Upload a server jar and auto-configure `java -jar <path> nogui`.
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

/// Point startup at an existing jar under the instance workdir.
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

pub async fn restore_backup(
    state: &AppState,
    id: &str,
    backup_id: &str,
) -> anyhow::Result<()> {
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
    util::audit("backup.restore", Some(id), json!({ "id": backup_id }), "api");
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
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_string();

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
    let target = super::modrinth::infer_target(
        project_type,
        &version.loaders,
        req.target.as_deref(),
    );
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

    let bytes = super::modrinth::download_bytes(&version.primary_url).await?;
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
    let bytes = super::hangar::download_bytes(&version.download_url).await?;
    let safe_name = version
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("hangar.jar")
        .to_string();
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
                    inst.status = *status;
                    inst.updated_at = *at;
                    if *status == InstanceStatus::Crashed && inst.spec.auto_restart {
                        should_restart = true;
                    }
                    if matches!(
                        *status,
                        InstanceStatus::Stopped | InstanceStatus::Crashed
                    ) {
                        inst.process = None;
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
                    tokio::spawn(async move {
                        util::notify_webhook(&url, &id, "crashed", &name).await;
                    });
                }
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
        }
        InstanceEvent::Log {
            instance_id,
            line,
        } => {
            util::append_instance_log(instance_id, &line.stream, &line.line);
            if let Some(names) = super::players::parse_online_players(&line.line) {
                let mut guard = state.instances.write().await;
                if let Some(inst) = guard.get_mut(instance_id) {
                    inst.last_players = names;
                }
            }
        }
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

    let (command, args) =
        super::versions::download_and_install(&view.spec.workdir, &req.core, &req.version).await?;

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    instance.spec.command = Some(command);
    instance.spec.args = args;
    instance.spec.core = req.core.clone();
    instance.updated_at = Utc::now();
    let out = instance.public_view();
    drop(guard);
    let _ = state.persist().await;
    util::audit(
        "core.install",
        Some(id),
        json!({ "core": req.core, "version": req.version }),
        "api",
    );
    Ok(out)
}

pub async fn list_players(state: &AppState, id: &str) -> anyhow::Result<Vec<PlayerInfo>> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    Ok(view
        .last_players
        .into_iter()
        .map(|name| PlayerInfo { name })
        .collect())
}

/// Optionally probe the server with `list` (writes to console). Prefer cached names.
pub async fn probe_players(state: &AppState, id: &str) -> anyhow::Result<Vec<PlayerInfo>> {
    let view = get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if view.status == InstanceStatus::Running {
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
        other => anyhow::bail!("unknown action: {other}"),
    };
    send_command(
        state,
        id,
        CommandRequest { command: cmd },
    )
    .await?;
    util::audit(
        "player.action",
        Some(id),
        json!({ "player": name, "action": action }),
        "api",
    );
    Ok(())
}

pub async fn list_worlds(state: &AppState, id: &str) -> anyhow::Result<Vec<super::worlds::WorldInfo>> {
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

pub async fn export_world(
    state: &AppState,
    id: &str,
    world: &str,
) -> anyhow::Result<BackupInfo> {
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
    state
        .instances
        .read()
        .await
        .get(id)
        .map(|i| i.spec.workdir.clone())
        .ok_or_else(|| anyhow::anyhow!("instance not found"))
}

async fn ensure_port_free(
    state: &AppState,
    port: u16,
    except_id: Option<&str>,
) -> anyhow::Result<()> {
    let guard = state.instances.read().await;
    for inst in guard.values() {
        if Some(inst.id.as_str()) == except_id {
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
            // Also warn on assigned but stopped? Allow reuse when stopped.
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

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
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
        let g = inst
            .spec
            .group
            .clone()
            .unwrap_or_else(|| "default".into());
        *groups.entry(g).or_default() += 1;
        let rt = match inst.spec.runtime {
            RuntimeKind::Docker => "docker",
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

pub async fn bulk_action(
    state: &AppState,
    req: BulkActionRequest,
) -> BulkActionResult {
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
