//! Docker container runtime for instances (CLI-based; works with Docker Desktop / Engine).

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Context;
use tokio::process::Command;
use tracing::info;

use super::process::{self, ProcessHandle};
use super::model::InstanceEvent;
use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DockerStatus {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

pub async fn docker_status() -> DockerStatus {
    match Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            DockerStatus {
                available: !version.is_empty(),
                version: Some(version.clone()),
                message: if version.is_empty() {
                    "docker CLI ok but no server version".into()
                } else {
                    format!("Docker Engine {version}")
                },
            }
        }
        Ok(out) => DockerStatus {
            available: false,
            version: None,
            message: format!(
                "docker not ready: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        },
        Err(e) => DockerStatus {
            available: false,
            version: None,
            message: format!("docker not found: {e}"),
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
    let status = docker_status().await;
    if !status.available {
        anyhow::bail!("Docker unavailable: {}", status.message);
    }

    std::fs::create_dir_all(&workdir)?;
    let abs = std::fs::canonicalize(&workdir)
        .with_context(|| format!("canonicalize workdir {workdir}"))?;
    let mount = docker_mount_path(&abs);

    let name = container_name(&instance_id);
    // Best-effort cleanup leftover container with same name.
    let _ = Command::new("docker")
        .args(["rm", "-f", &name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    let bin = command.unwrap_or_else(|| "java".into());
    if crate::util::is_java_command(&bin) {
        if !args.iter().any(|a| a == "-jar") {
            anyhow::bail!(
                "Docker 启动缺少 -jar：请先导入 server.jar 或设置启动命令"
            );
        }
        crate::util::inject_jvm_memory(&mut args, memory_mib);
    }

    let mut docker_args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "-i".into(),
        "--name".into(),
        name.clone(),
        "-v".into(),
        format!("{mount}:/data"),
        "-w".into(),
        "/data".into(),
        "-p".into(),
        format!("{port}:25565"),
        "--memory".into(),
        format!("{memory_mib}m"),
    ];
    if let Some(cpus) = cpu_limit {
        if cpus > 0.0 {
            docker_args.push("--cpus".into());
            docker_args.push(format!("{cpus:.2}"));
        }
    }
    docker_args.push(image.to_string());
    docker_args.push(bin.clone());
    docker_args.extend(args);

    info!(%name, %image, %mount, bin = %bin, "starting docker container instance");

    let out = Command::new("docker")
        .args(&docker_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "docker run failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let pid = process::docker_container_pid(&name)
        .await
        .ok_or_else(|| anyhow::anyhow!("docker container started but pid is unavailable"))?;
    process::adopt_running(
        instance_id,
        pid,
        workdir,
        events,
        Some(name),
        false,
        port,
    )
    .await
}

fn docker_mount_path(abs: &PathBuf) -> String {
    let s = abs.to_string_lossy().to_string();
    // Docker Desktop on Windows accepts either D:\path or /d/path.
    #[cfg(windows)]
    {
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            let drive = s.chars().next().unwrap().to_ascii_lowercase();
            let rest = s[2..].replace('\\', "/");
            return format!("/{drive}{rest}");
        }
    }
    s.replace('\\', "/")
}
