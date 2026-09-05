use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{error, info, warn};

use crate::util::{self, ParsedGameStats};

use super::model::{InstanceEvent, InstanceStatus, LogLine, MetricSample};

#[derive(Debug, Clone, Copy)]
pub enum StopMode {
    Graceful,
    Force,
}

#[derive(Debug, Default, Clone)]
struct LiveStats {
    game: ParsedGameStats,
}

#[derive(Debug)]
pub struct ProcessHandle {
    pub child_id: u32,
    stop_tx: mpsc::Sender<StopMode>,
    cmd_tx: mpsc::Sender<String>,
    /// Optional docker container name for cleanup.
    container_name: Option<String>,
}

impl ProcessHandle {
    pub async fn stop(self, mode: StopMode) {
        let _ = self.stop_tx.send(mode).await;
        if let Some(name) = self.container_name {
            // Ensure container is removed even if docker run hung.
            let _ = tokio::process::Command::new("docker")
                .args(["rm", "-f", &name])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        }
    }

    pub async fn send_command(&self, command: String) -> anyhow::Result<()> {
        self.cmd_tx
            .send(command)
            .await
            .map_err(|_| anyhow::anyhow!("process is not accepting commands"))
    }
}

pub async fn spawn_instance(
    instance_id: String,
    workdir: String,
    command: Option<String>,
    mut args: Vec<String>,
    memory_mib: u32,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    std::fs::create_dir_all(&workdir)?;

    if command.is_none() {
        return spawn_demo(instance_id, events).await;
    }

    let bin = command.as_ref().unwrap();
    if util::is_java_command(bin) {
        util::inject_jvm_memory(&mut args, memory_mib);
    }

    let child = build_command(bin, &args, &workdir)?;
    attach_child(instance_id, child, events, None).await
}

/// Spawn an arbitrary external command (used by Docker runtime).
pub async fn spawn_external_command(
    instance_id: String,
    bin: String,
    args: Vec<String>,
    events: broadcast::Sender<InstanceEvent>,
    container_name: Option<String>,
) -> anyhow::Result<ProcessHandle> {
    use std::process::Stdio;
    let child = Command::new(&bin)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    attach_child(instance_id, child, events, container_name).await
}

async fn attach_child(
    instance_id: String,
    mut child: Child,
    events: broadcast::Sender<InstanceEvent>,
    container_name: Option<String>,
) -> anyhow::Result<ProcessHandle> {
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdin = child.stdin.take().expect("stdin piped");
    let child_id = child.id().unwrap_or(0);

    let (stop_tx, stop_rx) = mpsc::channel::<StopMode>(1);
    let (stop_metrics_tx, stop_metrics_rx) = mpsc::channel::<()>(1);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(64);
    let live = Arc::new(Mutex::new(LiveStats::default()));

    tokio::spawn(pipe_lines(
        stdout,
        "stdout".into(),
        instance_id.clone(),
        events.clone(),
        Arc::clone(&live),
    ));
    tokio::spawn(pipe_lines(
        stderr,
        "stderr".into(),
        instance_id.clone(),
        events.clone(),
        Arc::clone(&live),
    ));

    let id_for_cmd = instance_id.clone();
    let events_for_cmd = events.clone();
    let stdin_shared = Arc::new(Mutex::new(stdin));
    let stdin_for_cmd = Arc::clone(&stdin_shared);
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            let _ = events_for_cmd.send(InstanceEvent::Log {
                instance_id: id_for_cmd.clone(),
                line: LogLine {
                    ts: Utc::now(),
                    stream: "stdin".into(),
                    line: format!("> {cmd}"),
                },
            });
            let line = format!("{cmd}\n");
            let mut guard = stdin_for_cmd.lock().await;
            if guard.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = guard.flush().await;
        }
    });

    tokio::spawn(supervise(
        child,
        stop_rx,
        Arc::clone(&stdin_shared),
        stop_metrics_tx,
        instance_id.clone(),
        events.clone(),
    ));
    tokio::spawn(metric_ticker(
        instance_id.clone(),
        child_id,
        events,
        stop_metrics_rx,
        live,
    ));

    info!(pid = child_id, "instance process spawned");
    Ok(ProcessHandle {
        child_id,
        stop_tx,
        cmd_tx,
        container_name,
    })
}

async fn spawn_demo(
    instance_id: String,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    let (stop_tx, mut stop_rx) = mpsc::channel::<StopMode>(1);
    let (stop_metrics_tx, stop_metrics_rx) = mpsc::channel::<()>(1);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(64);
    let live = Arc::new(Mutex::new(LiveStats::default()));

    let events_run = events.clone();
    let id_run = instance_id.clone();
    let live_demo = Arc::clone(&live);
    tokio::spawn(async move {
        let _ = events_run.send(InstanceEvent::StatusChanged {
            instance_id: id_run.clone(),
            status: InstanceStatus::Running,
            at: Utc::now(),
        });
        let _ = events_run.send(InstanceEvent::Log {
            instance_id: id_run.clone(),
            line: LogLine {
                ts: Utc::now(),
                stream: "stdout".into(),
                line: format!("[Demo] Cocktail instance {id_run} starting"),
            },
        });

        let mut tick = 0u32;
        let mut players = 0u32;
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                Some(mode) = stop_rx.recv() => {
                    if matches!(mode, StopMode::Graceful) {
                        let _ = events_run.send(InstanceEvent::Log {
                            instance_id: id_run.clone(),
                            line: LogLine {
                                ts: Utc::now(),
                                stream: "stdout".into(),
                                line: "[Demo] Stopping the server".into(),
                            },
                        });
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    let _ = stop_metrics_tx.send(()).await;
                    let _ = events_run.send(InstanceEvent::StatusChanged {
                        instance_id: id_run,
                        status: InstanceStatus::Stopped,
                        at: Utc::now(),
                    });
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    let _ = events_run.send(InstanceEvent::Log {
                        instance_id: id_run.clone(),
                        line: LogLine {
                            ts: Utc::now(),
                            stream: "stdin".into(),
                            line: format!("> {cmd}"),
                        },
                    });
                    let lower = cmd.to_ascii_lowercase();
                    if lower == "list" || lower.starts_with("list ") {
                        players = 2;
                        let line = format!("There are {players} of a max of 20 players online: Steve, Alex");
                        {
                            let mut g = live_demo.lock().await;
                            g.game.players = Some(players);
                        }
                        let _ = events_run.send(InstanceEvent::Log {
                            instance_id: id_run.clone(),
                            line: LogLine {
                                ts: Utc::now(),
                                stream: "stdout".into(),
                                line,
                            },
                        });
                    } else {
                        let _ = events_run.send(InstanceEvent::Log {
                            instance_id: id_run.clone(),
                            line: LogLine {
                                ts: Utc::now(),
                                stream: "stdout".into(),
                                line: format!("[Demo] executed: {cmd}"),
                            },
                        });
                    }
                }
                _ = interval.tick() => {
                    tick = tick.wrapping_add(1);
                    let tps = 20.0_f32;
                    {
                        let mut g = live_demo.lock().await;
                        g.game.tps = Some(tps);
                        g.game.players = Some(players);
                    }
                    let line = format!(
                        "[{}] [Server thread/INFO]: TPS={tps:.1} players={players} tick={tick}",
                        Utc::now().to_rfc3339()
                    );
                    let _ = events_run.send(InstanceEvent::Log {
                        instance_id: id_run.clone(),
                        line: LogLine {
                            ts: Utc::now(),
                            stream: "stdout".into(),
                            line,
                        },
                    });
                }
            }
        }
    });

    tokio::spawn(metric_ticker(
        instance_id,
        0,
        events,
        stop_metrics_rx,
        live,
    ));
    Ok(ProcessHandle {
        child_id: 0,
        stop_tx,
        cmd_tx,
        container_name: None,
    })
}

fn build_command(bin: &str, args: &[String], workdir: &str) -> anyhow::Result<Child> {
    use std::process::Stdio;

    Ok(Command::new(bin)
        .args(args)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?)
}

async fn pipe_lines<R>(
    reader: R,
    stream: String,
    instance_id: String,
    events: broadcast::Sender<InstanceEvent>,
    live: Arc<Mutex<LiveStats>>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let parsed = util::parse_game_stats(&line);
        if parsed.tps.is_some() || parsed.players.is_some() {
            let mut g = live.lock().await;
            if let Some(tps) = parsed.tps {
                g.game.tps = Some(tps);
            }
            if let Some(players) = parsed.players {
                g.game.players = Some(players);
            }
        }
        let _ = events.send(InstanceEvent::Log {
            instance_id: instance_id.clone(),
            line: LogLine {
                ts: Utc::now(),
                stream: stream.clone(),
                line,
            },
        });
    }
}

async fn supervise(
    mut child: Child,
    mut stop_rx: mpsc::Receiver<StopMode>,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    stop_metrics_tx: mpsc::Sender<()>,
    instance_id: String,
    events: broadcast::Sender<InstanceEvent>,
) {
    let _ = events.send(InstanceEvent::StatusChanged {
        instance_id: instance_id.clone(),
        status: InstanceStatus::Running,
        at: Utc::now(),
    });

    tokio::select! {
        Some(mode) = stop_rx.recv() => {
            match mode {
                StopMode::Graceful => {
                    {
                        let mut guard = stdin.lock().await;
                        let _ = guard.write_all(b"stop\n").await;
                        let _ = guard.flush().await;
                    }
                    let _ = events.send(InstanceEvent::Log {
                        instance_id: instance_id.clone(),
                        line: LogLine {
                            ts: Utc::now(),
                            stream: "system".into(),
                            line: "graceful stop requested (stop)".into(),
                        },
                    });
                    match tokio::time::timeout(Duration::from_secs(30), child.wait()).await {
                        Ok(_) => {}
                        Err(_) => {
                            warn!(%instance_id, "graceful stop timed out; force killing");
                            let _ = child.start_kill();
                            let _ = child.wait().await;
                        }
                    }
                }
                StopMode::Force => {
                    if let Err(e) = child.kill().await {
                        warn!(error = %e, "failed to kill child");
                    }
                    let _ = child.wait().await;
                }
            }
            let _ = stop_metrics_tx.send(()).await;
            let _ = events.send(InstanceEvent::StatusChanged {
                instance_id,
                status: InstanceStatus::Stopped,
                at: Utc::now(),
            });
        }
        status = child.wait() => {
            let _ = stop_metrics_tx.send(()).await;
            match status {
                Ok(s) if s.success() => {
                    let _ = events.send(InstanceEvent::StatusChanged {
                        instance_id,
                        status: InstanceStatus::Stopped,
                        at: Utc::now(),
                    });
                }
                Ok(_) | Err(_) => {
                    error!(%instance_id, "instance process exited abnormally");
                    let _ = events.send(InstanceEvent::StatusChanged {
                        instance_id,
                        status: InstanceStatus::Crashed,
                        at: Utc::now(),
                    });
                }
            }
        }
    }
}

async fn metric_ticker(
    instance_id: String,
    child_id: u32,
    events: broadcast::Sender<InstanceEvent>,
    mut stop_rx: mpsc::Receiver<()>,
    live: Arc<Mutex<LiveStats>>,
) {
    let mut sys = System::new();
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    // Prime CPU measurement.
    if child_id != 0 {
        sys.refresh_processes(ProcessesToUpdate::All, true);
    }
    loop {
        tokio::select! {
            _ = stop_rx.recv() => break,
            _ = interval.tick() => {
                let (cpu_pct, memory_mib) = if child_id == 0 {
                    // Demo: report light synthetic host-like usage still labeled demo via game stats.
                    (1.5_f32, 64.0_f32)
                } else {
                    let pid = Pid::from_u32(child_id);
                    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                    if let Some(proc) = sys.process(pid) {
                        let cpu = proc.cpu_usage();
                        let mem_mib = proc.memory() as f32 / (1024.0 * 1024.0);
                        (cpu, mem_mib)
                    } else {
                        (0.0, 0.0)
                    }
                };
                let game = live.lock().await.game.clone();
                let sample = MetricSample {
                    ts: Utc::now(),
                    cpu_pct,
                    memory_mib,
                    tps: game.tps,
                    players: game.players.unwrap_or(0),
                };
                if events
                    .send(InstanceEvent::Metric {
                        instance_id: instance_id.clone(),
                        sample,
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}
