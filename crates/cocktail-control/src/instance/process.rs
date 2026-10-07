//! Runtime RPC proxies; child processes and command channels live in init.
use super::runtime::{self, Connection};
use cocktail_shared::model::{InstanceEvent, InstanceStatus};
pub use cocktail_shared::runtime::StopMode;
use cocktail_shared::runtime::*;
use tokio::sync::{broadcast, watch};

#[derive(Debug)]
pub struct ProcessHandle {
    pub child_id: u32,
    pub reattached: bool,
    pub(crate) container_name: Option<String>,
    pub(crate) start_time: Option<u64>,
    token: String,
    connection: Connection,
    terminal: watch::Receiver<bool>,
    relay: tokio::task::JoinHandle<()>,
}
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        self.relay.abort();
    }
}

impl ProcessHandle {
    pub async fn stop(&self, mode: StopMode) -> anyhow::Result<()> {
        if *self.terminal.borrow() {
            return Ok(());
        }
        self.connection
            .call::<_, ()>(
                "process.stop",
                StopRequest {
                    token: self.token.clone(),
                    mode,
                },
            )
            .await?;
        let mut terminal = self.terminal.clone();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !*terminal.borrow_and_update() {
                terminal.changed().await?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("stop confirmed but terminal event not received"))??;
        Ok(())
    }
    pub async fn send_command(&self, command: String) -> anyhow::Result<()> {
        self.connection
            .call(
                "process.command",
                cocktail_shared::runtime::CommandRequest {
                    token: self.token.clone(),
                    command,
                },
            )
            .await
    }
    pub(crate) async fn available(&self) -> bool {
        self.connection
            .call::<_, Option<HandleInfo>>(
                "process.status",
                HandleRequest {
                    token: self.token.clone(),
                },
            )
            .await
            .is_ok_and(|info| info.is_some())
    }
}

pub(crate) async fn launch(
    instance_id: String,
    workdir: String,
    port: u16,
    kind: LaunchKind,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    let connection = Connection::current().await?;
    let token = uuid::Uuid::new_v4().to_string();
    // Subscribe before the RPC so fast startup/exit events cannot race the response.
    let mut rx = connection.subscribe();
    let adopting = matches!(&kind, LaunchKind::Adopt { .. });
    let info: HandleInfo = connection
        .call(
            "process.launch",
            LaunchRequest {
                token: token.clone(),
                instance_id,
                workdir,
                port,
                kind,
            },
        )
        .await?;
    anyhow::ensure!(
        adopting || info.token == token,
        "runtime returned a mismatched handle token"
    );
    let token = info.token.clone();
    let relay_connection = connection.clone();
    let (terminal_tx, terminal) = watch::channel(false);
    let relay = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = relay_connection.closed() => break,
                event = rx.recv() => match event {
                    Ok(pushed) if pushed.token == token => {
                        let terminal = matches!(&pushed.event, InstanceEvent::StatusChanged {
                            status: InstanceStatus::Stopped | InstanceStatus::Crashed, ..
                        });
                        let _ = events.send(pushed.event);
                        if terminal { terminal_tx.send_replace(true); break; }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(n, "init instance event relay lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
    Ok(ProcessHandle {
        child_id: info.child_id,
        reattached: info.reattached,
        container_name: info.container_name,
        start_time: info.start_time,
        token: info.token,
        connection,
        terminal,
        relay,
    })
}

pub async fn spawn_instance(
    instance_id: String,
    workdir: String,
    command: Option<String>,
    args: Vec<String>,
    memory_mib: u32,
    port: u16,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    launch(
        instance_id,
        workdir,
        port,
        LaunchKind::Process {
            command,
            args,
            memory_mib,
        },
        events,
    )
    .await
}

pub async fn adopt_running(
    instance_id: String,
    pid: u32,
    expected_start: Option<u64>,
    workdir: String,
    events: broadcast::Sender<InstanceEvent>,
    container_name: Option<String>,
    reattached: bool,
    port: u16,
) -> anyhow::Result<ProcessHandle> {
    launch(
        instance_id,
        workdir,
        port,
        LaunchKind::Adopt {
            pid,
            expected_start,
            container_name,
            reattached,
        },
        events,
    )
    .await
}

pub async fn process_matches(
    pid: u32,
    expected_start: Option<u64>,
    workdir: &str,
) -> anyhow::Result<bool> {
    runtime::call(
        "process.matches",
        MatchRequest {
            pid,
            expected_start,
            workdir: workdir.into(),
        },
    )
    .await
}
pub async fn pid_is_alive(pid: u32) -> anyhow::Result<bool> {
    runtime::call("process.alive", PidRequest { pid }).await
}
pub async fn docker_container_running(name: &str) -> anyhow::Result<bool> {
    runtime::call("container.running", NameRequest { name: name.into() }).await
}
pub async fn docker_container_pid(name: &str) -> anyhow::Result<Option<u32>> {
    runtime::call("container.pid", NameRequest { name: name.into() }).await
}

pub(crate) async fn lookup(instance_id: &str) -> anyhow::Result<Option<HandleInfo>> {
    runtime::call(
        "process.lookup",
        InstanceIdRequest {
            instance_id: instance_id.into(),
        },
    )
    .await
}
