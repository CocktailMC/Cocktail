use std::path::PathBuf;

use anyhow::Context;
use tokio::sync::broadcast;

use super::process::{self, ProcessHandle};
use super::runtime::{self, ContainerSpawnSpec};
use cocktail_shared::model::InstanceEvent;

pub use cocktail_shared::runtime::DockerStatus;

pub async fn docker_status() -> DockerStatus {
    match runtime::require_runtime().await {
        Ok(rt) => {
            let s = rt.status().await;
            DockerStatus {
                available: s.available,
                version: s.version,
                message: s.message,
            }
        }
        Err(_) => DockerStatus {
            available: false,
            version: None,
            message: "容器运行时未初始化(未检测到 Docker 或 Podman)".into(),
        },
    }
}

pub fn container_name(instance_id: &str) -> String {
    let short = instance_id.chars().take(8).collect::<String>();
    format!("cocktail-{short}")
}

pub async fn spawn_docker_instance(
    instance_id: String,
    workdir: String,
    command: Option<String>,
    mut args: Vec<String>,
    memory_mib: u32,
    port: u16,
    cpu_limit: Option<f32>,
    image: &str,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    let rt = runtime::require_runtime().await?;

    std::fs::create_dir_all(&workdir)?;
    let name = container_name(&instance_id);

    let bin = command.unwrap_or_else(|| "java".into());
    if crate::util::is_java_command(&bin) {
        if !args.iter().any(|a| a == "-jar") {
            anyhow::bail!("Docker 启动缺少 -jar:请先导入 server.jar 或设置启动命令");
        }
        crate::util::inject_jvm_memory(&mut args, memory_mib);
    }

    let spec = ContainerSpawnSpec {
        name: name.clone(),
        image: image.to_string(),
        workdir: workdir.clone(),
        command: bin.clone(),
        args,
        memory_mib,
        host_port: port,
        container_port: 25565,
        cpu_limit,
        env: Vec::new(),
    };

    let handle = rt.spawn(spec).await.context("spawn container")?;

    process::adopt_running(
        instance_id,
        handle.pid,
        workdir,
        events,
        Some(name),
        false,
        port,
    )
    .await
}

pub use cocktail_shared::runtime::DockerImage;

pub async fn list_images() -> anyhow::Result<Vec<DockerImage>> {
    let rt = runtime::require_runtime().await?;
    let images = rt.list_images().await?;
    Ok(images
        .into_iter()
        .map(|i| DockerImage {
            repo_tag: i.repo_tag,
            id: i.id,
            size: i.size,
        })
        .collect())
}

pub async fn pull_image(image: &str) -> anyhow::Result<()> {
    let rt = runtime::require_runtime().await?;
    rt.pull_image(image).await
}

pub(crate) fn docker_mount_path(abs: &PathBuf) -> String {
    let mut s = abs.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }

    #[cfg(windows)]
    {
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' {
            let drive = s.chars().next().unwrap().to_ascii_lowercase();
            let rest = s[2..].replace('\\', "/");
            return format!("/{drive}{rest}");
        }
    }
    s.replace('\\', "/")
}
