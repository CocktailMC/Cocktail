// Docker/Podman container runtime abstraction using the bollard SDK.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::{
    ContainerCreateBody, ContainerStatsResponse, HostConfig, Mount, MountType, PortBinding,
    PortMap, RestartPolicy, RestartPolicyNameEnum,
};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, InspectContainerOptions, ListImagesOptions,
    LogsOptions, RemoveContainerOptions, RemoveImageOptions, StartContainerOptions, StatsOptions,
    StopContainerOptions,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use futures_util::{Stream, StreamExt};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerEngine {
    Docker,
    Podman,
}

impl ContainerEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContainerEngine::Docker => "docker",
            ContainerEngine::Podman => "podman",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerSpawnSpec {
    pub name: String,
    pub image: String,
    pub workdir: String,
    pub command: String,
    pub args: Vec<String>,
    pub memory_mib: u32,
    pub host_port: u16,
    pub container_port: u16,
    pub cpu_limit: Option<f32>,
    pub env: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpawnedContainer {
    pub pid: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub repo_tag: String,
    pub id: String,
    pub size: String,
}

#[derive(Debug, Clone, Default)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage: u64,
    pub memory_limit: u64,
    pub pids: Option<u64>,
}

pub struct ContainerRuntime {
    docker: Arc<Docker>,
    pub engine: ContainerEngine,
}

impl ContainerRuntime {
    pub async fn detect() -> Result<Self> {
        if let Ok(docker) = connect_docker_default() {
            if docker.ping().await.is_ok() {
                return Ok(Self {
                    docker: Arc::new(docker),
                    engine: ContainerEngine::Docker,
                });
            }
        }
        if let Ok(docker) = connect_podman_default() {
            if docker.ping().await.is_ok() {
                return Ok(Self {
                    docker: Arc::new(docker),
                    engine: ContainerEngine::Podman,
                });
            }
        }
        Err(anyhow!(
            "no container engine detected (tried docker and podman)"
        ))
    }

    pub async fn status(&self) -> RuntimeStatus {
        match self.docker.ping().await {
            Ok(_) => match self.docker.version().await {
                Ok(v) => RuntimeStatus {
                    available: true,
                    version: v.version,
                    message: format!("{} 运行正常", self.engine.as_str()),
                },
                Err(e) => RuntimeStatus {
                    available: false,
                    version: None,
                    message: format!("version 查询失败: {e}"),
                },
            },
            Err(e) => RuntimeStatus {
                available: false,
                version: None,
                message: format!("ping 失败: {e}"),
            },
        }
    }

    pub async fn spawn(&self, spec: ContainerSpawnSpec) -> Result<SpawnedContainer> {
        let port_key = format!("{}/tcp", spec.container_port);
        let mut port_bindings: PortMap = HashMap::new();
        port_bindings.insert(
            port_key.clone(),
            Some(vec![PortBinding {
                host_ip: None,
                host_port: Some(spec.host_port.to_string()),
            }]),
        );

        let mounts = vec![Mount {
            target: Some("/work".to_string()),
            source: Some(spec.workdir.clone()),
            typ: Some(MountType::BIND),
            read_only: Some(false),
            ..Default::default()
        }];

        let restart_policy = Some(RestartPolicy {
            name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
            maximum_retry_count: None,
        });

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            mounts: Some(mounts),
            network_mode: Some("bridge".into()),
            restart_policy,
            auto_remove: Some(false),
            privileged: Some(false),
            memory: Some((spec.memory_mib as i64) * 1024 * 1024),
            nano_cpus: spec.cpu_limit.map(|c| (c * 1_000_000_000.0) as i64),
            ..Default::default()
        };

        let mut full_cmd = vec![spec.command.clone()];
        full_cmd.extend(spec.args.iter().cloned());

        let config = ContainerCreateBody {
            image: Some(spec.image.clone()),
            cmd: Some(full_cmd),
            working_dir: Some("/work".into()),
            env: if spec.env.is_empty() {
                None
            } else {
                Some(spec.env.clone())
            },
            exposed_ports: Some(vec![port_key]),
            host_config: Some(host_config),
            attach_stdin: Some(false),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            tty: Some(false),
            open_stdin: Some(false),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: Some(spec.name.clone()),
            ..Default::default()
        };

        self.docker
            .create_container(Some(options), config)
            .await
            .with_context(|| format!("create container {}", spec.name))?;

        self.docker
            .start_container(&spec.name, None::<StartContainerOptions>)
            .await
            .with_context(|| format!("start container {}", spec.name))?;

        // Use a synthetic pid; the container is tracked by name.
        Ok(SpawnedContainer { pid: 0 })
    }

    pub async fn stop(&self, name: &str, timeout_secs: u64) -> Result<()> {
        let options = StopContainerOptions {
            t: Some(timeout_secs as i32),
            ..Default::default()
        };
        self.docker
            .stop_container(name, Some(options))
            .await
            .with_context(|| format!("stop container {}", name))?;
        Ok(())
    }

    pub async fn remove(&self, name: &str) -> Result<()> {
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        self.docker
            .remove_container(name, Some(options))
            .await
            .with_context(|| format!("remove container {}", name))?;
        Ok(())
    }

    pub async fn running(&self, name: &str) -> bool {
        match self
            .docker
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => info.state.and_then(|s| s.running).unwrap_or(false),
            Err(_) => false,
        }
    }

    pub async fn pid_of(&self, name: &str) -> Option<u32> {
        let info = self
            .docker
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
            .ok()?;
        let pid = info.state?.pid?;
        if pid > 0 {
            Some(pid as u32)
        } else {
            None
        }
    }

    pub async fn stats(&self, name: &str) -> Result<ContainerStats> {
        let options = StatsOptions {
            stream: false,
            one_shot: true,
        };
        let mut stream = self.docker.stats(name, Some(options));
        let first = stream
            .next()
            .await
            .ok_or_else(|| anyhow!("no stats returned"))??;
        Ok(extract_stats(&first))
    }

    pub fn stats_stream(
        &self,
        name: &str,
    ) -> impl Stream<Item = Result<ContainerStats>> + Send + '_ {
        let options = StatsOptions {
            stream: true,
            one_shot: false,
        };
        self.docker
            .stats(name, Some(options))
            .map(|r| r.map_err(Into::into).map(|s| extract_stats(&s)))
    }

    pub fn logs_stream(
        &self,
        name: &str,
        tail: usize,
    ) -> impl Stream<Item = Result<Vec<u8>>> + Send + '_ {
        let options = LogsOptions {
            follow: true,
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };
        self.docker
            .logs(name, Some(options))
            .map(|r| r.map_err(Into::into).map(|o| log_output_bytes(&o)))
    }

    pub async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        let options = ListImagesOptions {
            all: true,
            ..Default::default()
        };
        let images = self
            .docker
            .list_images(Some(options))
            .await
            .context("list images")?;
        Ok(images
            .into_iter()
            .map(|i| ImageInfo {
                repo_tag: i.repo_tags.first().cloned().unwrap_or_else(|| "<none>".into()),
                id: i.id,
                size: format_size(i.size),
            })
            .collect())
    }

    pub async fn pull_image(&self, image: &str) -> Result<()> {
        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };
        let mut stream = self.docker.create_image(Some(options), None, None);
        while let Some(item) = stream.next().await {
            item.context("pull image")?;
        }
        Ok(())
    }

    pub async fn remove_image(&self, name: &str) -> Result<()> {
        let options = RemoveImageOptions {
            force: true,
            ..Default::default()
        };
        self.docker
            .remove_image(name, Some(options), None)
            .await
            .with_context(|| format!("remove image {}", name))?;
        Ok(())
    }

    pub async fn exec(&self, name: &str, cmd: &[&str]) -> Result<String> {
        let config = CreateExecOptions {
            cmd: Some(cmd.iter().map(|s| s.to_string()).collect()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        let result = self
            .docker
            .create_exec(name, config)
            .await
            .with_context(|| format!("create exec in {}", name))?;
        let start = self
            .docker
            .start_exec(&result.id, None::<StartExecOptions>)
            .await
            .with_context(|| format!("start exec {}", result.id))?;

        let mut output = String::new();
        if let StartExecResults::Attached { output: mut stream, .. } = start {
            while let Some(chunk) = stream.next().await {
                if let Ok(lo) = chunk {
                    output.push_str(&String::from_utf8_lossy(&log_output_bytes(&lo)));
                }
            }
        }
        Ok(output)
    }

    pub async fn write_stdin_exec(&self, name: &str, command: &str) -> Result<()> {
        // Use the container's exec to send the command to the server process.
        // Minecraft servers read commands from stdin; we use `sh -c` to echo
        // the command into the process via its attached stdin is not possible
        // without attach, so we rely on the server's own console if it has
        // rcon; otherwise we exec a best-effort command.
        let line = format!("{}\n", command);
        let config = CreateExecOptions {
            cmd: Some(vec!["sh".to_string(), "-c".into(), line]),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            ..Default::default()
        };
        let result = self
            .docker
            .create_exec(name, config)
            .await
            .with_context(|| format!("create exec for stdin in {}", name))?;
        self.docker
            .start_exec(
                &result.id,
                Some(StartExecOptions {
                    detach: true,
                    tty: false,
                    output_capacity: None,
                }),
            )
            .await
            .with_context(|| format!("start exec {}", result.id))?;
        Ok(())
    }

    pub async fn attach(&self, name: &str) -> Result<ContainerAttach> {
        use bollard::query_parameters::AttachContainerOptions;
        let options = AttachContainerOptions {
            stdin: true,
            stdout: true,
            stderr: true,
            stream: true,
            logs: false,
            ..Default::default()
        };
        let res = self
            .docker
            .attach_container(name, Some(options))
            .await
            .with_context(|| format!("attach container {}", name))?;
        Ok(ContainerAttach {
            output: res.output,
            input: Arc::new(Mutex::new(res.input)),
        })
    }
}

pub struct ContainerAttach {
    pub output: std::pin::Pin<
        Box<dyn Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>,
    >,
    pub input: Arc<Mutex<std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>>>,
}

impl ContainerAttach {
    pub async fn write_stdin(&self, data: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut writer = self.input.lock().await;
        writer.write_all(data).await.context("write stdin")?;
        writer.flush().await.context("flush stdin")?;
        Ok(())
    }
}

fn log_output_bytes(lo: &LogOutput) -> Vec<u8> {
    match lo {
        LogOutput::StdOut { message }
        | LogOutput::StdErr { message }
        | LogOutput::StdIn { message }
        | LogOutput::Console { message } => message.to_vec(),
    }
}

fn extract_stats(s: &ContainerStatsResponse) -> ContainerStats {
    let (cpu_percent, memory_usage, memory_limit) = extract_cpu_mem(s);
    ContainerStats {
        cpu_percent,
        memory_usage,
        memory_limit,
        pids: s.pids_stats.as_ref().and_then(|p| p.current),
    }
}

fn extract_cpu_mem(s: &ContainerStatsResponse) -> (f64, u64, u64) {
    let cpu = s.cpu_stats.as_ref();
    let mem = s.memory_stats.as_ref();
    let cpu_usage = cpu.and_then(|c| c.cpu_usage.as_ref());
    let total = cpu_usage.and_then(|u| u.total_usage).unwrap_or(0);
    let system = cpu.and_then(|c| c.system_cpu_usage).unwrap_or(0);
    let mem_usage = mem.and_then(|m| m.usage).unwrap_or(0);
    let mem_limit = mem.and_then(|m| m.limit).unwrap_or(0);
    let cpu_percent = if system > 0 {
        (total as f64 / system as f64) * 100.0
    } else {
        0.0
    };
    (cpu_percent, mem_usage, mem_limit)
}

fn format_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn connect_docker_default() -> Result<Docker> {
    if let Ok(host) = std::env::var("DOCKER_HOST") {
        return Docker::connect_with_local(&host, 120, API_DEFAULT_VERSION)
            .context("connect docker via DOCKER_HOST");
    }
    Docker::connect_with_local_defaults().context("connect docker defaults")
}

#[cfg(unix)]
fn connect_podman_default() -> Result<Docker> {
    let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let candidates = [
        format!("{}/podman/podman.sock", xdg),
        "/run/podman/podman.sock".to_string(),
        "/var/run/podman/podman.sock".to_string(),
    ];
    for path in candidates {
        if std::path::Path::new(&path).exists() {
            return Docker::connect_with_local(
                &format!("unix://{}", path),
                120,
                API_DEFAULT_VERSION,
            )
            .with_context(|| format!("connect podman at {}", path));
        }
    }
    Docker::connect_with_podman_defaults().context("connect podman defaults")
}

#[cfg(not(unix))]
fn connect_podman_default() -> Result<Docker> {
    let candidates = [
        "npipe:////./pipe/podman",
        "npipe:////./pipe/podman-machine-default",
    ];
    for path in candidates {
        if let Ok(d) = Docker::connect_with_local(path, 120, API_DEFAULT_VERSION) {
            return Ok(d);
        }
    }
    Err(anyhow!("no podman socket found"))
}

static GLOBAL_RUNTIME: std::sync::OnceLock<Result<Arc<ContainerRuntime>, String>> =
    std::sync::OnceLock::new();

/// Store an already-detected runtime into the global slot.
pub fn set_runtime(rt: ContainerRuntime) {
    let _ = GLOBAL_RUNTIME.set(Ok(Arc::new(rt)));
}

/// Get the already-initialised runtime, or None if unavailable.
pub fn runtime() -> Option<Arc<ContainerRuntime>> {
    GLOBAL_RUNTIME.get().and_then(|r| r.as_ref().ok()).map(Arc::clone)
}

/// Initialise (or reuse) the global container runtime. Returns an error if
/// no engine is available.
pub async fn require_runtime() -> Result<Arc<ContainerRuntime>> {
    if let Some(existing) = GLOBAL_RUNTIME.get() {
        return existing
            .as_ref()
            .map(Arc::clone)
            .map_err(|e| anyhow!(e.clone()));
    }
    let rt = ContainerRuntime::detect().await;
    let result = match rt {
        Ok(r) => Ok(Arc::new(r)),
        Err(e) => Err(e.to_string()),
    };
    let stored = GLOBAL_RUNTIME.get_or_init(|| match &result {
        Ok(r) => Ok(Arc::clone(r)),
        Err(e) => Err(e.clone()),
    });
    stored
        .as_ref()
        .map(Arc::clone)
        .map_err(|e| anyhow!(e.clone()))
}
