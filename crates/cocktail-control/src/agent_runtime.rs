//! Remote node agent — runs instances on behalf of the control plane.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use crate::instance::{
    files, InstanceEvent, InstanceSpec, InstanceStatus, RuntimeKind,
};
use crate::instance::process::{self, ProcessHandle, StopMode};
use crate::proto::{AgentDown, AgentUp, ApplyInstance};
use crate::util;

struct Live {
    spec: InstanceSpec,
    generation: u64,
    handle: Option<ProcessHandle>,
}

pub async fn run_agent() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("cocktail_control=info")
            }),
        )
        .init();

    let plane = std::env::var("COCKTAIL_PLANE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:11011".into());
    let token = std::env::var("COCKTAIL_NODE_TOKEN").map_err(|_| {
        anyhow::anyhow!("需要环境变量 COCKTAIL_NODE_TOKEN（在控制面「节点」页创建节点时生成）")
    })?;
    let ws = plane_to_ws(&plane, &token)?;
    info!(%ws, "cocktail-agent connecting");

    loop {
        if let Err(e) = serve_once(&ws).await {
            warn!(error = %e, "agent session ended, retry in 3s");
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

fn plane_to_ws(plane: &str, token: &str) -> anyhow::Result<String> {
    let base = plane.trim_end_matches('/');
    let ws_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = base.strip_prefix("wss://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("ws://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{base}")
    };
    let enc = urlencoding_lite(token);
    Ok(format!("{ws_base}/api/v1/agent/ws?token={enc}"))
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn serve_once(url: &str) -> anyhow::Result<()> {
    let (ws, _) = tokio_tungstenite::connect_async(url).await?;
    let (mut sink, mut stream) = ws.split();
    let (events, _) = broadcast::channel::<InstanceEvent>(512);
    let mut lives: HashMap<String, Live> = HashMap::new();

    let hello = AgentUp::Hello {
        hostname: hostname(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    };
    sink.send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await?;

    let mut ev_rx = events.subscribe();
    let mut beat = tokio::time::interval(Duration::from_secs(15));

    loop {
        tokio::select! {
            _ = beat.tick() => {
                sink.send(Message::Text(serde_json::to_string(&AgentUp::Heartbeat)?.into()))
                    .await?;
            }
            ev = ev_rx.recv() => {
                match ev {
                    Ok(InstanceEvent::Log { instance_id, line }) => {
                        let up = AgentUp::Log { instance_id, line };
                        sink.send(Message::Text(serde_json::to_string(&up)?.into())).await?;
                    }
                    Ok(InstanceEvent::Metric { instance_id, sample }) => {
                        let up = AgentUp::Metric { instance_id, sample };
                        sink.send(Message::Text(serde_json::to_string(&up)?.into())).await?;
                    }
                    Ok(InstanceEvent::StatusChanged { instance_id, status, .. }) => {
                        let pid = lives.get(&instance_id).and_then(|l| l.handle.as_ref()).map(|h| h.child_id).filter(|p| *p > 0);
                        let up = AgentUp::Status { instance_id, status, pid };
                        sink.send(Message::Text(serde_json::to_string(&up)?.into())).await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let down: AgentDown = serde_json::from_str(&text)?;
                        apply_down(&mut lives, down, events.clone()).await;
                    }
                    Some(Ok(Message::Ping(p))) => {
                        sink.send(Message::Pong(p)).await?;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

async fn apply_down(
    lives: &mut HashMap<String, Live>,
    down: AgentDown,
    events: broadcast::Sender<InstanceEvent>,
) {
    match down {
        AgentDown::Welcome { instances, .. } => {
            for inst in instances {
                apply_one(lives, inst, events.clone()).await;
            }
        }
        AgentDown::Apply { instance } => {
            apply_one(lives, instance, events).await;
        }
        AgentDown::Stop { instance_id } => {
            if let Some(live) = lives.get_mut(&instance_id) {
                live.spec.desired_running = false;
                stop_live(live).await;
                let _ = events.send(InstanceEvent::StatusChanged {
                    instance_id,
                    status: InstanceStatus::Stopped,
                    at: chrono::Utc::now(),
                });
            }
        }
        AgentDown::Command {
            instance_id,
            command,
        } => {
            if let Some(live) = lives.get(&instance_id) {
                if let Some(h) = live.handle.as_ref() {
                    let _ = h.send_command(command).await;
                }
            }
        }
    }
}

async fn apply_one(
    lives: &mut HashMap<String, Live>,
    inst: ApplyInstance,
    events: broadcast::Sender<InstanceEvent>,
) {
    let id = inst.id.clone();
    let desired = inst.spec.desired_running;
    let entry = lives.entry(id.clone()).or_insert(Live {
        spec: inst.spec.clone(),
        generation: inst.generation,
        handle: None,
    });
    entry.spec = inst.spec;
    entry.generation = inst.generation;
    if desired {
        if entry.handle.is_none() {
            match spawn_live(&id, &entry.spec, events.clone()).await {
                Ok(handle) => {
                    entry.handle = Some(handle);
                    let _ = events.send(InstanceEvent::StatusChanged {
                        instance_id: id,
                        status: InstanceStatus::Starting,
                        at: chrono::Utc::now(),
                    });
                }
                Err(e) => {
                    error!(error = %e, %id, "agent failed to start instance");
                    let _ = events.send(InstanceEvent::StatusChanged {
                        instance_id: id,
                        status: InstanceStatus::Crashed,
                        at: chrono::Utc::now(),
                    });
                }
            }
        }
    } else {
        stop_live(entry).await;
        let _ = events.send(InstanceEvent::StatusChanged {
            instance_id: id,
            status: InstanceStatus::Stopped,
            at: chrono::Utc::now(),
        });
    }
}

async fn stop_live(live: &mut Live) {
    if let Some(handle) = live.handle.take() {
        handle.stop(StopMode::Graceful).await;
    }
}

async fn spawn_live(
    id: &str,
    spec: &InstanceSpec,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    let workdir = spec.workdir.clone();
    let mut command = spec.command.clone();
    let mut args = spec.args.clone();
    if command.is_none() || (command.as_deref() == Some("java") && args.is_empty()) {
        if files::jar_exists(&workdir, "server.jar") {
            let (cmd, a) = util::java_jar_startup("server.jar");
            command = Some(cmd);
            args = a;
        } else if spec.core != "demo" {
            anyhow::bail!("未配置启动命令且找不到 server.jar");
        }
    }
    let seed_port = match spec.runtime {
        RuntimeKind::Docker => 25565,
        RuntimeKind::Process => spec.port,
    };
    files::ensure_seed_files(&workdir, seed_port, spec.eula_accepted)?;
    match spec.runtime {
        RuntimeKind::Docker => {
            crate::instance::container::spawn_docker_instance(
                id.to_string(),
                workdir,
                command,
                args,
                spec.memory_mib,
                spec.port,
                spec.cpu_limit,
                spec.docker_image
                    .as_deref()
                    .unwrap_or("eclipse-temurin:21-jre"),
                events,
            )
            .await
        }
        RuntimeKind::Process => {
            process::spawn_instance(
                id.to_string(),
                workdir,
                command,
                args,
                spec.memory_mib,
                events,
            )
            .await
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "agent".into())
}
