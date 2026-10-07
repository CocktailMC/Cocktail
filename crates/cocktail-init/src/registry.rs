//! Init owns live handles. Control owns configuration, authorization and placement.
use crate::{
    container, process, proto,
    server::{Server, parse_params},
};
use cocktail_shared::{
    model::{InstanceEvent, InstanceStatus},
    runtime::*,
};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::{Mutex, broadcast, watch};

struct Entry {
    info: HandleInfo,
    handle: Option<process::ProcessHandle>,
    done: watch::Sender<bool>,
}

pub struct Registry {
    entries: Mutex<HashMap<String, Entry>>,
    events: broadcast::Sender<RuntimeEvent>,
    pending: std::sync::Mutex<HashMap<String, String>>,
}

struct Reservation {
    registry: Arc<Registry>,
    instance_id: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.registry
            .pending
            .lock()
            .unwrap()
            .remove(&self.instance_id);
    }
}

impl Registry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
            events: broadcast::channel(4096).0,
            pending: std::sync::Mutex::new(HashMap::new()),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.events.subscribe()
    }

    pub async fn launch(self: &Arc<Self>, request: LaunchRequest) -> anyhow::Result<HandleInfo> {
        let LaunchRequest {
            token,
            instance_id,
            workdir,
            port,
            kind,
        } = request;
        anyhow::ensure!(
            !token.is_empty() && !instance_id.is_empty(),
            "empty runtime identity"
        );
        let entries = self.entries.lock().await;
        if let Some(entry) = entries.get(&instance_id) {
            if let LaunchKind::Adopt {
                pid,
                expected_start,
                container_name,
                ..
            } = &kind
            {
                anyhow::ensure!(entry.handle.is_some(), "instance is stopping");
                anyhow::ensure!(
                    entry.info.child_id == *pid && entry.info.container_name == *container_name,
                    "managed instance identity mismatch"
                );
                anyhow::ensure!(
                    expected_start.is_none() || entry.info.start_time == *expected_start,
                    "managed instance start time mismatch"
                );
                return Ok(entry.info.clone());
            }
            anyhow::bail!("instance already managed or stopping");
        }
        anyhow::ensure!(
            !entries.values().any(|e| e.info.token == token),
            "duplicate runtime token"
        );
        {
            let mut pending = self.pending.lock().unwrap();
            anyhow::ensure!(
                !pending.contains_key(&instance_id),
                "instance launch in progress"
            );
            anyhow::ensure!(
                !pending.values().any(|t| t == &token),
                "runtime token launch in progress"
            );
            pending.insert(instance_id.clone(), token.clone());
        }
        let _reservation = Reservation {
            registry: Arc::clone(self),
            instance_id: instance_id.clone(),
        };
        drop(entries);
        // Subscribe before launch: even an immediately exiting child must be observed.
        let (events, mut rx) = broadcast::channel(4096);
        let handle = match kind {
            LaunchKind::Process {
                command,
                args,
                memory_mib,
            } => {
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
            LaunchKind::Container {
                command,
                args,
                memory_mib,
                cpu_limit,
                image,
            } => {
                container::spawn_docker_instance(
                    instance_id.clone(),
                    workdir,
                    command,
                    args,
                    memory_mib,
                    port,
                    cpu_limit,
                    &image,
                    events,
                )
                .await?
            }
            LaunchKind::Adopt {
                pid,
                expected_start,
                container_name,
                reattached,
            } => {
                if let Some(name) = &container_name {
                    let rt = crate::runtime::require_runtime().await?;
                    anyhow::ensure!(rt.running(name).await, "container is not running");
                } else {
                    anyhow::ensure!(
                        process::process_matches(pid, expected_start, &workdir),
                        "process identity changed"
                    );
                }
                process::adopt_running(
                    instance_id.clone(),
                    pid,
                    workdir,
                    events,
                    container_name,
                    reattached,
                    port,
                )
                .await?
            }
        };
        let info = HandleInfo {
            token: token.clone(),
            child_id: handle.child_id,
            reattached: handle.reattached,
            container_name: handle.container_name.clone(),
            start_time: process::process_snapshot(handle.child_id).map(|(t, _)| t),
        };
        let (done, _) = watch::channel(false);
        let mut entries = self.entries.lock().await;
        entries.insert(
            instance_id.clone(),
            Entry {
                info: info.clone(),
                handle: Some(handle),
                done,
            },
        );
        drop(entries);
        let registry = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(instance_id, n, "runtime event receiver lagged");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let terminal = matches!(
                    &event,
                    InstanceEvent::StatusChanged {
                        status: InstanceStatus::Stopped | InstanceStatus::Crashed,
                        ..
                    }
                );
                if terminal {
                    let mut entries = registry.entries.lock().await;
                    if entries
                        .get(&instance_id)
                        .is_some_and(|e| e.info.token == token)
                    {
                        let entry = entries.remove(&instance_id).unwrap();
                        // Container removal is deferred until the monitor confirms exit.
                        if entry.handle.is_none() {
                            if let Some(name) = &entry.info.container_name {
                                if let Some(rt) = crate::runtime::runtime() {
                                    if let Err(e) = rt.remove(name).await {
                                        tracing::warn!(error = %e, name, "container cleanup failed");
                                    }
                                }
                            }
                        }
                        let _ = entry.done.send(true);
                    }
                }
                let pushed = RuntimeEvent {
                    token: token.clone(),
                    event,
                };
                let _ = registry.events.send(pushed.clone());
                if let Ok(params) = serde_json::to_value(pushed) {
                    crate::events::emit(INSTANCE_EVENT, params);
                }
                if terminal {
                    break;
                }
            }
        });
        Ok(info)
    }

    pub async fn stop(&self, request: StopRequest) -> anyhow::Result<()> {
        let (handle, mut done) = {
            let mut entries = self.entries.lock().await;
            let entry = entries
                .values_mut()
                .find(|e| e.info.token == request.token)
                .ok_or_else(|| anyhow::anyhow!("runtime handle expired"))?;
            (entry.handle.take(), entry.done.subscribe())
        };
        if let Some(handle) = handle {
            handle.stop(request.mode).await;
        }
        tokio::time::timeout(Duration::from_secs(50), async {
            while !*done.borrow_and_update() {
                done.changed().await?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("instance stop not confirmed within 50s"))??;
        Ok(())
    }

    pub async fn command(&self, request: CommandRequest) -> anyhow::Result<()> {
        let entries = self.entries.lock().await;
        let entry = entries
            .values()
            .find(|e| e.info.token == request.token)
            .ok_or_else(|| anyhow::anyhow!("runtime handle expired"))?;
        let handle = entry
            .handle
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("instance is stopping"))?
            .clone();
        drop(entries);
        handle.send_command(request.command).await
    }

    pub async fn lookup(&self, instance_id: &str) -> Option<HandleInfo> {
        self.entries
            .lock()
            .await
            .get(instance_id)
            .map(|entry| entry.info.clone())
    }

    pub async fn status(&self, token: &str) -> Option<HandleInfo> {
        self.entries
            .lock()
            .await
            .values()
            .find(|e| e.info.token == token)
            .map(|e| e.info.clone())
    }
}

pub fn registry() -> &'static Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

fn encode<T: serde::Serialize>(result: anyhow::Result<T>) -> proto::RpcResult {
    result
        .and_then(|value| Ok(serde_json::to_value(value)?))
        .map_err(|e| proto::Error::internal(format!("{e:#}")))
}

pub fn register(server: &mut Server) {
    server.register("process.launch", |p| async move {
        encode(registry().launch(parse_params(p)?).await)
    });
    server.register("process.stop", |p| async move {
        encode(registry().stop(parse_params(p)?).await)
    });
    server.register("process.command", |p| async move {
        encode(registry().command(parse_params(p)?).await)
    });
    server.register("process.lookup", |p| async move {
        let p: InstanceIdRequest = parse_params(p)?;
        encode(Ok(registry().lookup(&p.instance_id).await))
    });
    server.register("process.status", |p| async move {
        let p: HandleRequest = parse_params(p)?;
        encode(Ok(registry().status(&p.token).await))
    });
    server.register("process.snapshot", |p| async move {
        let p: PidRequest = parse_params(p)?;
        encode(Ok(process::process_snapshot(p.pid)))
    });
    server.register("process.matches", |p| async move {
        let p: MatchRequest = parse_params(p)?;
        encode(Ok(process::process_matches(
            p.pid,
            p.expected_start,
            &p.workdir,
        )))
    });
    server.register("process.alive", |p| async move {
        let p: PidRequest = parse_params(p)?;
        encode(Ok(process::pid_is_alive(p.pid)))
    });
    server.register("container.status", |_| async {
        encode(Ok(container::docker_status().await))
    });
    server.register("container.images", |_| async {
        encode(container::list_images().await)
    });
    server.register("container.pull", |p| async move {
        let p: NameRequest = parse_params(p)?;
        encode(container::pull_image(&p.name).await)
    });
    server.register("container.pid", |p| async move {
        let p: NameRequest = parse_params(p)?;
        encode(match crate::runtime::require_runtime().await {
            Ok(rt) => Ok(rt.pid_of(&p.name).await),
            Err(e) => Err(e),
        })
    });
    server.register("container.running", |p| async move {
        let p: NameRequest = parse_params(p)?;
        encode(match crate::runtime::require_runtime().await {
            Ok(rt) => Ok(rt.running(&p.name).await),
            Err(e) => Err(e),
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo(id: &str, workdir: &std::path::Path) -> LaunchRequest {
        LaunchRequest {
            token: uuid::Uuid::new_v4().to_string(),
            instance_id: id.into(),
            workdir: workdir.to_string_lossy().into_owned(),
            port: 25565,
            kind: LaunchKind::Process {
                command: None,
                args: vec![],
                memory_mib: 512,
            },
        }
    }

    #[tokio::test]
    async fn lifecycle_events_commands_and_stale_tokens() {
        let registry = Registry::new();
        let dir = tempfile::tempdir().unwrap();
        let mut events = registry.subscribe();
        let first = registry
            .launch(demo("lifecycle", dir.path()))
            .await
            .unwrap();
        assert!(
            registry
                .launch(demo("lifecycle", dir.path()))
                .await
                .is_err()
        );
        registry
            .command(CommandRequest {
                token: first.token.clone(),
                command: "say batch2".into(),
            })
            .await
            .unwrap();
        let mut running = false;
        let mut command = false;
        tokio::time::timeout(Duration::from_secs(5), async {
            while !running || !command {
                let pushed = events.recv().await.unwrap();
                assert_eq!(pushed.token, first.token);
                match pushed.event {
                    InstanceEvent::StatusChanged {
                        status: InstanceStatus::Running,
                        ..
                    } => running = true,
                    InstanceEvent::Log { line, .. } if line.line.contains("batch2") => {
                        command = true
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        registry
            .stop(StopRequest {
                token: first.token.clone(),
                mode: StopMode::Graceful,
            })
            .await
            .unwrap();
        assert!(registry.status(&first.token).await.is_none());
        let second = registry
            .launch(demo("lifecycle", dir.path()))
            .await
            .unwrap();
        assert_ne!(first.token, second.token);
        assert!(
            registry
                .command(CommandRequest {
                    token: first.token.clone(),
                    command: "stop".into()
                })
                .await
                .is_err()
        );
        assert!(
            registry
                .stop(StopRequest {
                    token: first.token,
                    mode: StopMode::Force
                })
                .await
                .is_err()
        );
        assert!(registry.status(&second.token).await.is_some());
        registry
            .stop(StopRequest {
                token: second.token,
                mode: StopMode::Force,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn adoption_reuses_monitors_and_rejects_wrong_identity() {
        let registry = Registry::new();
        let dir = tempfile::tempdir().unwrap();
        let first = registry.launch(demo("adopt", dir.path())).await.unwrap();
        let mut request = demo("adopt", dir.path());
        request.kind = LaunchKind::Adopt {
            pid: first.child_id,
            expected_start: first.start_time,
            container_name: None,
            reattached: true,
        };
        let adopted = registry.launch(request).await.unwrap();
        assert_eq!(adopted.token, first.token);
        let mut wrong = demo("adopt", dir.path());
        wrong.kind = LaunchKind::Adopt {
            pid: 42,
            expected_start: None,
            container_name: None,
            reattached: true,
        };
        assert!(registry.launch(wrong).await.is_err());
        registry
            .stop(StopRequest {
                token: first.token,
                mode: StopMode::Force,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn failed_launch_releases_reservation_and_rpc_validates_params() {
        let registry = Registry::new();
        let dir = tempfile::tempdir().unwrap();
        let mut request = demo("retry", dir.path());
        request.kind = LaunchKind::Adopt {
            pid: 0,
            expected_start: None,
            container_name: None,
            reattached: true,
        };
        assert!(registry.launch(request).await.is_err());
        let handle = registry.launch(demo("retry", dir.path())).await.unwrap();
        registry
            .stop(StopRequest {
                token: handle.token,
                mode: StopMode::Force,
            })
            .await
            .unwrap();
        let mut server = Server::new();
        register(&mut server);
        assert_eq!(
            server
                .call("process.launch", serde_json::json!({}))
                .await
                .unwrap_err()
                .code,
            -32602
        );
        assert!(
            server
                .call(
                    "process.command",
                    serde_json::json!({"token":"missing", "command":"stop"})
                )
                .await
                .is_err()
        );
    }
}
