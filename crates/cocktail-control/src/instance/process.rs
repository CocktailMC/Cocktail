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
    pub reattached: bool,
    stop_tx: mpsc::Sender<StopMode>,
    cmd_tx: mpsc::Sender<String>,
    /// Optional docker container name for cleanup.
    pub(crate) container_name: Option<String>,
}

impl ProcessHandle {
    pub async fn stop(self, mode: StopMode) {
        let _ = tokio::time::timeout(Duration::from_secs(5), self.stop_tx.send(mode)).await;
        if let Some(name) = self.container_name {
            // Ensure container is removed even if docker run hung.
            let _ = tokio::time::timeout(Duration::from_secs(20), async {
                tokio::process::Command::new("docker")
                    .args(["rm", "-f", &name])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await
            })
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
    port: u16,
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

    let child = build_command(bin, &args, &workdir, &instance_id)?;
    attach_child(instance_id, child, events, None, Some(workdir), false, port).await
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
        .kill_on_drop(false)
        .spawn()?;
    attach_child(instance_id, child, events, container_name, None, false, 0).await
}

async fn attach_child(
    instance_id: String,
    mut child: Child,
    events: broadcast::Sender<InstanceEvent>,
    container_name: Option<String>,
    workdir: Option<String>,
    reattached: bool,
    port: u16,
) -> anyhow::Result<ProcessHandle> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = child.stdin.take();
    let child_id = child.id().unwrap_or(0);

    let (stop_tx, stop_rx) = mpsc::channel::<StopMode>(1);
    let (stop_metrics_tx, stop_metrics_rx) = mpsc::channel::<()>(1);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(64);
    let live = Arc::new(Mutex::new(LiveStats::default()));
    let (stop_logs_tx, stop_logs_rx) = mpsc::channel::<()>(1);

    if let Some(stdout) = stdout {
        tokio::spawn(pipe_lines(
            stdout,
            "stdout".into(),
            instance_id.clone(),
            events.clone(),
            Arc::clone(&live),
        ));
    }
    if let Some(stderr) = stderr {
        tokio::spawn(pipe_lines(
            stderr,
            "stderr".into(),
            instance_id.clone(),
            events.clone(),
            Arc::clone(&live),
        ));
    }

    let fifo_file = workdir
        .as_deref()
        .and_then(|dir| open_cmd_fifo(dir).ok())
        .map(|f| Arc::new(Mutex::new(f)));
    let docker_name = container_name.clone();
    let id_for_cmd = instance_id.clone();
    let events_for_cmd = events.clone();
    let stdin_shared = stdin.map(|s| Arc::new(Mutex::new(s)));
    let stdin_for_cmd = stdin_shared.clone();
    let fifo_for_cmd = fifo_file.clone();
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
            if let Some(name) = docker_name.as_ref() {
                let _ = docker_write_stdin(name, &cmd).await;
                continue;
            }
            if let Some(fifo) = fifo_for_cmd.as_ref() {
                let line = format!("{cmd}\n");
                let mut guard = fifo.lock().await;
                if std::io::Write::write_all(&mut *guard, line.as_bytes()).is_err() {
                    break;
                }
                let _ = std::io::Write::flush(&mut *guard);
                continue;
            }
            if let Some(stdin) = stdin_for_cmd.as_ref() {
                let line = format!("{cmd}\n");
                let mut guard = stdin.lock().await;
                if guard.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                let _ = guard.flush().await;
            }
        }
    });

    tokio::spawn(follow_file(
        console_log_path(&instance_id),
        true,
        "stdout".into(),
        instance_id.clone(),
        events.clone(),
        Arc::clone(&live),
        stop_logs_rx,
    ));

    tokio::spawn(supervise(
        child,
        stop_rx,
        stdin_shared,
        fifo_file,
        container_name.clone(),
        stop_metrics_tx,
        stop_logs_tx,
        instance_id.clone(),
        events.clone(),
        child_id,
        reattached,
    ));
    tokio::spawn(metric_ticker(
        instance_id.clone(),
        child_id,
        port,
        container_name.is_some(),
        events,
        stop_metrics_rx,
        live,
    ));

    info!(pid = child_id, reattached, "instance process spawned");
    Ok(ProcessHandle {
        child_id,
        reattached,
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
        25565,
        false,
        events,
        stop_metrics_rx,
        live,
    ));
    Ok(ProcessHandle {
        child_id: 0,
        reattached: false,
        stop_tx,
        cmd_tx,
        container_name: None,
    })
}

fn build_command(bin: &str, args: &[String], workdir: &str, instance_id: &str) -> anyhow::Result<Child> {
    use std::process::Stdio;

    let log = open_console_log(instance_id)?;
    let err = log.try_clone()?;
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(workdir)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .kill_on_drop(false);

    #[cfg(unix)]
    {
        ensure_fifo(&fifo_path(workdir))?;
        let fifo = open_cmd_fifo(workdir)?;
        cmd.stdin(Stdio::from(fifo));
    }
    #[cfg(not(unix))]
    {
        cmd.stdin(Stdio::piped());
    }

    Ok(cmd.spawn()?)
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
    stdin: Option<Arc<Mutex<tokio::process::ChildStdin>>>,
    fifo: Option<Arc<Mutex<std::fs::File>>>,
    container_name: Option<String>,
    stop_metrics_tx: mpsc::Sender<()>,
    stop_logs_tx: mpsc::Sender<()>,
    instance_id: String,
    events: broadcast::Sender<InstanceEvent>,
    child_id: u32,
    reattached: bool,
) {
    let _ = events.send(InstanceEvent::StatusChanged {
        instance_id: instance_id.clone(),
        status: InstanceStatus::Running,
        at: Utc::now(),
    });
    if reattached {
        let _ = events.send(InstanceEvent::Log {
            instance_id: instance_id.clone(),
            line: LogLine {
                ts: Utc::now(),
                stream: "system".into(),
                line: format!("已接管仍在运行的进程 pid={child_id}"),
            },
        });
    }

    tokio::select! {
        Some(mode) = stop_rx.recv() => {
            match mode {
                StopMode::Graceful => {
                    request_graceful_stop(stdin.as_ref(), fifo.as_ref(), container_name.as_deref()).await;
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
                            force_kill(child_id, container_name.as_deref()).await;
                            let _ = child.start_kill();
                            let _ = child.wait().await;
                        }
                    }
                }
                StopMode::Force => {
                    force_kill(child_id, container_name.as_deref()).await;
                    if let Err(e) = child.kill().await {
                        warn!(error = %e, "failed to kill child");
                    }
                    let _ = child.wait().await;
                }
            }
            let _ = stop_metrics_tx.send(()).await;
            let _ = stop_logs_tx.send(()).await;
            let _ = events.send(InstanceEvent::StatusChanged {
                instance_id,
                status: InstanceStatus::Stopped,
                at: Utc::now(),
            });
        }
        status = child.wait() => {
            let _ = stop_metrics_tx.send(()).await;
            let _ = stop_logs_tx.send(()).await;
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

/// Reattach to a process that survived a control-plane restart.
pub async fn adopt_running(
    instance_id: String,
    pid: u32,
    workdir: String,
    events: broadcast::Sender<InstanceEvent>,
    container_name: Option<String>,
    reattached: bool,
    port: u16,
) -> anyhow::Result<ProcessHandle> {
    if container_name.is_none() && (pid == 0 || !pid_is_alive(pid)) {
        anyhow::bail!("process {pid} is not running");
    }

    let (stop_tx, mut stop_rx) = mpsc::channel::<StopMode>(1);
    let (stop_metrics_tx, stop_metrics_rx) = mpsc::channel::<()>(1);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(64);
    let live = Arc::new(Mutex::new(LiveStats::default()));
    let (stop_logs_tx, stop_logs_rx) = mpsc::channel::<()>(1);

    let fifo_file = open_cmd_fifo(&workdir).ok().map(|f| Arc::new(Mutex::new(f)));
    let docker_name = container_name.clone();
    let id_for_cmd = instance_id.clone();
    let events_for_cmd = events.clone();
    let fifo_for_cmd = fifo_file.clone();
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
            if let Some(name) = docker_name.as_ref() {
                let _ = docker_write_stdin(name, &cmd).await;
                continue;
            }
            if let Some(fifo) = fifo_for_cmd.as_ref() {
                let line = format!("{cmd}\n");
                let mut guard = fifo.lock().await;
                let _ = std::io::Write::write_all(&mut *guard, line.as_bytes());
                let _ = std::io::Write::flush(&mut *guard);
            }
        }
    });

    if container_name.is_some() {
        if let Some(name) = container_name.clone() {
            tokio::spawn(follow_docker_logs(
                name,
                instance_id.clone(),
                events.clone(),
                Arc::clone(&live),
                stop_logs_rx,
            ));
        }
    } else {
        tokio::spawn(follow_file(
            console_log_path(&instance_id),
            true,
            "stdout".into(),
            instance_id.clone(),
            events.clone(),
            Arc::clone(&live),
            stop_logs_rx,
        ));
    }
    let latest = std::path::PathBuf::from(&workdir).join("logs").join("latest.log");
    let (stop_latest_tx, stop_latest_rx) = mpsc::channel::<()>(1);
    tokio::spawn(follow_file(
        latest,
        true,
        "stdout".into(),
        instance_id.clone(),
        events.clone(),
        Arc::clone(&live),
        stop_latest_rx,
    ));

    let events_sup = events.clone();
    let id_sup = instance_id.clone();
    let container_sup = container_name.clone();
    tokio::spawn(async move {
        let _ = events_sup.send(InstanceEvent::StatusChanged {
            instance_id: id_sup.clone(),
            status: InstanceStatus::Running,
            at: Utc::now(),
        });
        if reattached {
            let _ = events_sup.send(InstanceEvent::Log {
                instance_id: id_sup.clone(),
                line: LogLine {
                    ts: Utc::now(),
                    stream: "system".into(),
                    line: format!("已接管仍在运行的进程 pid={pid}"),
                },
            });
        }
        loop {
            tokio::select! {
                Some(mode) = stop_rx.recv() => {
                    match mode {
                        StopMode::Graceful => {
                            request_graceful_stop(None, fifo_file.as_ref(), container_sup.as_deref()).await;
                            if !wait_pid_exit(pid, Duration::from_secs(30)).await {
                                warn!(%id_sup, pid, "graceful stop timed out; force killing");
                                force_kill(pid, container_sup.as_deref()).await;
                                let _ = wait_pid_exit(pid, Duration::from_secs(5)).await;
                            }
                        }
                        StopMode::Force => {
                            force_kill(pid, container_sup.as_deref()).await;
                            let _ = wait_pid_exit(pid, Duration::from_secs(5)).await;
                        }
                    }
                    let _ = stop_metrics_tx.send(()).await;
                    let _ = stop_logs_tx.send(()).await;
                    let _ = stop_latest_tx.send(()).await;
                    let _ = events_sup.send(InstanceEvent::StatusChanged {
                        instance_id: id_sup,
                        status: InstanceStatus::Stopped,
                        at: Utc::now(),
                    });
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    let alive = if let Some(name) = container_sup.as_deref() {
                        docker_container_running(name).await
                    } else {
                        pid_is_alive(pid)
                    };
                    if !alive {
                        let _ = stop_metrics_tx.send(()).await;
                        let _ = stop_logs_tx.send(()).await;
                        let _ = stop_latest_tx.send(()).await;
                        let _ = events_sup.send(InstanceEvent::StatusChanged {
                            instance_id: id_sup,
                            status: InstanceStatus::Stopped,
                            at: Utc::now(),
                        });
                        break;
                    }
                }
            }
        }
    });

    tokio::spawn(metric_ticker(
        instance_id,
        pid,
        port,
        container_name.is_some(),
        events,
        stop_metrics_rx,
        live,
    ));

    info!(pid, "instance process adopted");
    Ok(ProcessHandle {
        child_id: pid,
        reattached,
        stop_tx,
        cmd_tx,
        container_name,
    })
}

pub fn process_snapshot(pid: u32) -> Option<(u64, Option<std::path::PathBuf>)> {
    if pid == 0 {
        return None;
    }
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let proc = sys.process(pid)?;
    Some((proc.start_time(), proc.cwd().map(|p| p.to_path_buf())))
}

pub fn process_matches(pid: u32, expected_start: Option<u64>, workdir: &str) -> bool {
    let Some((start, cwd)) = process_snapshot(pid) else {
        return false;
    };
    if let Some(expected) = expected_start {
        if expected > 0 && start.abs_diff(expected) > 2 {
            return false;
        }
    }
    let Ok(want) = std::fs::canonicalize(workdir) else {
        return true;
    };
    if let Some(cwd) = cwd {
        if let Ok(got) = std::fs::canonicalize(&cwd) {
            return got == want;
        }
    }
    true
}

pub fn pid_is_alive(pid: u32) -> bool {
    process_snapshot(pid).is_some()
}

async fn wait_pid_exit(pid: u32, timeout: Duration) -> bool {
    let start = tokio::time::Instant::now();
    while start.elapsed() < timeout {
        if !pid_is_alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    !pid_is_alive(pid)
}

fn fifo_path(workdir: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(workdir).join(".cocktail").join("stdin")
}

pub fn console_log_path(instance_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("data")
        .join("logs")
        .join(format!("{instance_id}.console.log"))
}

fn open_console_log(instance_id: &str) -> anyhow::Result<std::fs::File> {
    let path = console_log_path(instance_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?)
}

#[cfg(unix)]
fn ensure_fifo(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::FileTypeExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let ft = std::fs::metadata(path)?.file_type();
        if ft.is_fifo() {
            return Ok(());
        }
        std::fs::remove_file(path)?;
    }
    let s = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("fifo path is not utf-8"))?;
    let cstr = std::ffi::CString::new(s)?;
    let rc = unsafe { libc::mkfifo(cstr.as_ptr(), 0o600) };
    if rc != 0 {
        anyhow::bail!("mkfifo {}: {}", path.display(), std::io::Error::last_os_error());
    }
    Ok(())
}

fn open_cmd_fifo(workdir: &str) -> anyhow::Result<std::fs::File> {
    #[cfg(unix)]
    {
        let path = fifo_path(workdir);
        ensure_fifo(&path)?;
        Ok(std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?)
    }
    #[cfg(not(unix))]
    {
        let _ = workdir;
        anyhow::bail!("command fifo is not supported on this platform")
    }
}

async fn request_graceful_stop(
    stdin: Option<&Arc<Mutex<tokio::process::ChildStdin>>>,
    fifo: Option<&Arc<Mutex<std::fs::File>>>,
    container_name: Option<&str>,
) {
    if let Some(name) = container_name {
        let _ = docker_write_stdin(name, "stop").await;
        return;
    }
    if let Some(fifo) = fifo {
        let mut guard = fifo.lock().await;
        let _ = std::io::Write::write_all(&mut *guard, b"stop\n");
        let _ = std::io::Write::flush(&mut *guard);
        return;
    }
    if let Some(stdin) = stdin {
        let mut guard = stdin.lock().await;
        let _ = guard.write_all(b"stop\n").await;
        let _ = guard.flush().await;
    }
}

async fn force_kill(pid: u32, container_name: Option<&str>) {
    if let Some(name) = container_name {
        let _ = tokio::process::Command::new("docker")
            .args(["stop", "-t", "2", name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
        return;
    }
    if pid == 0 {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .await;
    }
}

pub(crate) async fn docker_container_running(name: &str) -> bool {
    let Ok(out) = tokio::process::Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", name])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
    else {
        return false;
    };
    out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true"
}

pub async fn docker_container_pid(name: &str) -> Option<u32> {
    let out = tokio::process::Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}} {{.State.Pid}}", name])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let running = parts.next()? == "true";
    let pid: u32 = parts.next()?.parse().ok()?;
    if running && pid > 0 {
        Some(pid)
    } else {
        None
    }
}

async fn docker_write_stdin(name: &str, command: &str) -> anyhow::Result<()> {
    use std::process::Stdio;
    let mut child = tokio::process::Command::new("docker")
        .args(["exec", "-i", name, "sh", "-c", "cat > /proc/1/fd/0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(format!("{command}\n").as_bytes()).await?;
        stdin.flush().await?;
    }
    let _ = child.wait().await;
    Ok(())
}

async fn follow_docker_logs(
    name: String,
    instance_id: String,
    events: broadcast::Sender<InstanceEvent>,
    live: Arc<Mutex<LiveStats>>,
    mut stop_rx: mpsc::Receiver<()>,
) {
    use std::process::Stdio;
    let Ok(mut child) = tokio::process::Command::new("docker")
        .args(["logs", "-f", "--tail", "20", &name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    else {
        return;
    };
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(pipe_lines(
            stdout,
            "stdout".into(),
            instance_id.clone(),
            events.clone(),
            Arc::clone(&live),
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(pipe_lines(
            stderr,
            "stderr".into(),
            instance_id,
            events,
            live,
        ));
    }
    let _ = stop_rx.recv().await;
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn follow_file(
    path: std::path::PathBuf,
    from_end: bool,
    stream: String,
    instance_id: String,
    events: broadcast::Sender<InstanceEvent>,
    live: Arc<Mutex<LiveStats>>,
    mut stop_rx: mpsc::Receiver<()>,
) {
    use tokio::io::AsyncReadExt;
    let mut offset = 0u64;
    if from_end {
        if let Ok(meta) = std::fs::metadata(&path) {
            offset = meta.len();
        }
    }
    let mut buf = String::new();
    loop {
        tokio::select! {
            _ = stop_rx.recv() => break,
            _ = tokio::time::sleep(Duration::from_millis(400)) => {
                let Ok(mut file) = tokio::fs::File::open(&path).await else {
                    continue;
                };
                let Ok(meta) = file.metadata().await else { continue };
                let len = meta.len();
                if len < offset {
                    offset = 0;
                }
                if len == offset {
                    continue;
                }
                if tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(offset)).await.is_err() {
                    continue;
                }
                let mut chunk = Vec::new();
                if file.read_to_end(&mut chunk).await.is_err() {
                    continue;
                }
                offset = len;
                buf.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(idx) = buf.find('\n') {
                    let mut line = buf[..idx].to_string();
                    buf = buf[idx + 1..].to_string();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                    if line.is_empty() {
                        continue;
                    }
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
        }
    }
}

async fn metric_ticker(
    instance_id: String,
    child_id: u32,
    port: u16,
    docker: bool,
    events: broadcast::Sender<InstanceEvent>,
    mut stop_rx: mpsc::Receiver<()>,
    live: Arc<Mutex<LiveStats>>,
) {
    let mut sys = System::new();
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    let mut net_prev = super::netmon::NetCounters::default();
    let mut demo_tick = 0u32;
    // Prime CPU measurement.
    if child_id != 0 {
        sys.refresh_processes(ProcessesToUpdate::All, true);
    }
    loop {
        tokio::select! {
            _ = stop_rx.recv() => break,
            _ = interval.tick() => {
                let (cpu_pct, memory_mib) = if child_id == 0 {
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
                let net = if child_id == 0 {
                    demo_tick = demo_tick.wrapping_add(1);
                    super::netmon::demo_sample(demo_tick, &net_prev)
                } else {
                    let pid = child_id;
                    let prev = net_prev.clone();
                    tokio::task::spawn_blocking(move || {
                        super::netmon::sample(port, pid, docker, &prev)
                    })
                    .await
                    .unwrap_or_else(|_| super::netmon::NetSample::default())
                };
                net_prev = net.counters.clone();
                let sample = MetricSample {
                    ts: Utc::now(),
                    cpu_pct,
                    memory_mib,
                    tps: game.tps,
                    players: game.players.unwrap_or(0),
                    net_rx_bps: net.rx_bps,
                    net_tx_bps: net.tx_bps,
                    net_connections: net.connections,
                    net_unique_ips: net.unique_ips,
                    net_listen: net.listen,
                    net_peers: net.peers,
                    net_syn_recv: net.syn_recv,
                    net_time_wait: net.time_wait,
                    net_fin_wait: net.fin_wait,
                    net_udp: net.udp,
                    net_rx_pps: net.rx_pps,
                    net_tx_pps: net.tx_pps,
                    net_rx_bytes: net.rx_bytes,
                    net_tx_bytes: net.tx_bytes,
                    net_session_rx: net.session_rx,
                    net_session_tx: net.session_tx,
                    net_peak_rx_bps: net.peak_rx_bps,
                    net_peak_tx_bps: net.peak_tx_bps,
                    net_drops: net.drops,
                    net_errors: net.errors,
                    net_rtt_ms: net.rtt_ms,
                    net_ping_online: net.ping_online,
                    net_ping_max: net.ping_max,
                    net_ping_version: net.ping_version,
                    net_source: Some(net.source.into()),
                    net_alerts: net.alerts,
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
