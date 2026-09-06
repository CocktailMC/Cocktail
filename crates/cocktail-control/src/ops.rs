//! Host network ticker, QQ alerts, and periodic status digest.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::hostnet::{self, HostNetPrev, HostNetSample, InstanceNetRow};
use crate::instance::InstanceStatus;
use crate::qqbot::{QqClient, QqConfig};
use crate::state::SharedState;

pub(crate) const HOST_NET_BUFFER: usize = 180;

pub struct OpsRuntime {
    pub history: tokio::sync::RwLock<VecDeque<HostNetSample>>,
    pub latest: tokio::sync::RwLock<Option<HostNetSample>>,
    qq: Arc<QqClient>,
    prev: Mutex<HostNetPrev>,
    alert_seen: Mutex<HashMap<String, Instant>>,
    last_status: Mutex<Option<Instant>>,
    pub auto_hold: Mutex<HashMap<String, Instant>>,
}

impl OpsRuntime {
    pub fn new() -> Self {
        Self {
            history: tokio::sync::RwLock::new(VecDeque::new()),
            latest: tokio::sync::RwLock::new(None),
            qq: Arc::new(QqClient::new()),
            prev: Mutex::new(HostNetPrev::default()),
            alert_seen: Mutex::new(HashMap::new()),
            last_status: Mutex::new(None),
            auto_hold: Mutex::new(HashMap::new()),
        }
    }
}

pub fn spawn(state: &SharedState) {
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let _ = crate::netops::expire_now(&state).await;
            crate::automations::tick(&state).await;
            crate::automations::run_backup_hours(&state).await;
            sample_local_node(&state).await;
            if let Err(e) = tick(&state).await {
                tracing::debug!(error = %e, "ops tick");
            }
        }
    });
}

async fn tick(state: &SharedState) -> anyhow::Result<()> {
    let cfg = {
        let conn = state.db.lock().await;
        crate::db::panel(&conn)?
    };
    let prev = state.ops.prev.lock().await.clone();
    let thresh = cfg.net_alert_rx_bps;
    let (mut sample, next) = tokio::task::spawn_blocking(move || hostnet::sample(&prev, thresh))
        .await?;
    *state.ops.prev.lock().await = next;

    let instances = {
        let guard = state.instances.read().await;
        guard
            .values()
            .map(|inst| InstanceNetRow {
                id: inst.id.clone(),
                name: inst.spec.name.clone(),
                status: status_slug(inst.status),
                port: inst.spec.port,
                rx_bps: inst.last_metrics.as_ref().map(|m| m.net_rx_bps).unwrap_or(0.0),
                tx_bps: inst.last_metrics.as_ref().map(|m| m.net_tx_bps).unwrap_or(0.0),
                connections: inst
                    .last_metrics
                    .as_ref()
                    .map(|m| m.net_connections)
                    .unwrap_or(0),
                unique_ips: inst
                    .last_metrics
                    .as_ref()
                    .map(|m| m.net_unique_ips)
                    .unwrap_or(0),
                alerts: inst
                    .last_metrics
                    .as_ref()
                    .map(|m| m.net_alerts.clone())
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>()
    };
    for row in &instances {
        for a in &row.alerts {
            sample.alerts.push(format!("{}: {a}", row.name));
        }
    }
    sample.instances = instances;

    {
        let mut hist = state.ops.history.write().await;
        hist.push_back(sample.clone());
        while hist.len() > HOST_NET_BUFFER {
            hist.pop_front();
        }
    }
    *state.ops.latest.write().await = Some(sample.clone());

    let qq = QqConfig {
        app_id: cfg.qq_app_id.clone(),
        app_secret: cfg.qq_app_secret.clone(),
        group_openid: cfg.qq_group_openid.clone(),
        user_openid: cfg.qq_user_openid.clone(),
        sandbox: cfg.qq_sandbox,
    };

    if cfg.qq_alerts && qq.ready() && !sample.alerts.is_empty() {
        for a in &sample.alerts {
            if should_emit(&state.ops, a).await {
                let text = format!("【Cocktail 告警】\n{a}");
                let client = Arc::clone(&state.ops.qq);
                let http = state.http.clone();
                let qq = qq.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.send_text(&http, &qq, &text).await {
                        tracing::warn!(error = %e, "qq alert failed");
                    }
                });
            }
        }
    }

    if cfg.qq_status_secs > 0 && qq.ready() {
        let every = Duration::from_secs(cfg.qq_status_secs.max(60));
        let due = {
            let mut last = state.ops.last_status.lock().await;
            match *last {
                None => {
                    *last = Some(Instant::now());
                    false
                }
                Some(t) if t.elapsed() < every => false,
                _ => {
                    *last = Some(Instant::now());
                    true
                }
            }
        };
        if due {
            let digest = status_digest(&sample, &cfg.panel_name);
            let client = Arc::clone(&state.ops.qq);
            let http = state.http.clone();
            let qq = qq.clone();
            tokio::spawn(async move {
                if let Err(e) = client.send_text(&http, &qq, &digest).await {
                    tracing::warn!(error = %e, "qq status failed");
                }
            });
        }
    }
    Ok(())
}

async fn should_emit(ops: &OpsRuntime, key: &str) -> bool {
    let mut map = ops.alert_seen.lock().await;
    let now = Instant::now();
    map.retain(|_, at| now.saturating_duration_since(*at) < Duration::from_secs(12 * 3600));
    if let Some(at) = map.get(key) {
        if now.saturating_duration_since(*at) < Duration::from_secs(15 * 60) {
            return false;
        }
    }
    map.insert(key.to_string(), now);
    true
}

fn status_slug(status: InstanceStatus) -> String {
    match status {
        InstanceStatus::Created => "created",
        InstanceStatus::Starting => "starting",
        InstanceStatus::Running => "running",
        InstanceStatus::Stopping => "stopping",
        InstanceStatus::Stopped => "stopped",
        InstanceStatus::Crashed => "crashed",
    }
    .into()
}

fn status_digest(sample: &HostNetSample, panel: &str) -> String {
    let running = sample
        .instances
        .iter()
        .filter(|i| i.status == "running")
        .count();
    let crashed = sample
        .instances
        .iter()
        .filter(|i| i.status == "crashed")
        .count();
    let players: u32 = sample.instances.iter().map(|i| i.connections).sum();
    format!(
        "【Cocktail 状态】{panel}\n主机 ↓ {}  ↑ {}\nTCP ESTAB {} · SYN-RECV {}\n实例 {} 运行 / {} 崩溃 · 游戏端口连接 {}\n{}",
        fmt_bps(sample.rx_bps),
        fmt_bps(sample.tx_bps),
        sample.tcp_estab,
        sample.syn_recv,
        running,
        crashed,
        players,
        chrono::Local::now().format("%F %T")
    )
}

fn fmt_bps(n: f32) -> String {
    if n < 1024.0 {
        format!("{n:.0} B/s")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.1} KiB/s", n / 1024.0)
    } else {
        format!("{:.2} MiB/s", n / (1024.0 * 1024.0))
    }
}

pub async fn send_now(state: &SharedState, text: &str) -> anyhow::Result<()> {
    let cfg = {
        let conn = state.db.lock().await;
        crate::db::panel(&conn)?
    };
    let qq = QqConfig {
        app_id: cfg.qq_app_id,
        app_secret: cfg.qq_app_secret,
        group_openid: cfg.qq_group_openid,
        user_openid: cfg.qq_user_openid,
        sandbox: cfg.qq_sandbox,
    };
    state.ops.qq.send_text(&state.http, &qq, text).await
}

pub async fn notify_event(state: &SharedState, title: &str, body: &str) {
    let cfg = {
        let conn = state.db.lock().await;
        crate::db::panel(&conn).ok()
    };
    let Some(cfg) = cfg else {
        return;
    };
    if !cfg.qq_alerts {
        return;
    }
    let qq = QqConfig {
        app_id: cfg.qq_app_id,
        app_secret: cfg.qq_app_secret,
        group_openid: cfg.qq_group_openid,
        user_openid: cfg.qq_user_openid,
        sandbox: cfg.qq_sandbox,
    };
    if !qq.ready() {
        return;
    }
    let key = format!("{title}:{body}");
    if !should_emit(&state.ops, &key).await {
        return;
    }
    let text = format!("【Cocktail {title}】\n{body}");
    if let Err(e) = state.ops.qq.send_text(&state.http, &qq, &text).await {
        tracing::warn!(error = %e, "qq event notify failed");
    }
}

async fn sample_local_node(state: &SharedState) {
    let (cpu_pct, memory_mib) = tokio::task::spawn_blocking(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        (
            sys.global_cpu_usage(),
            (sys.used_memory() as f32) / (1024.0 * 1024.0),
        )
    })
    .await
    .unwrap_or((0.0, 0.0));
    let host = state.ops.latest.read().await;
    let (rx_bps, tx_bps) = host
        .as_ref()
        .map(|s| (s.rx_bps, s.tx_bps))
        .unwrap_or((0.0, 0.0));
    drop(host);
    state.node_live.write().await.insert(
        "local".into(),
        crate::state::NodeLive {
            cpu_pct,
            memory_mib,
            rx_bps,
            tx_bps,
        },
    );
}
