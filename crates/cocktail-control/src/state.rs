use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

use crate::automations::PanelEvent;
use crate::db;
use crate::instance::{self, Instance, InstanceEvent, LogLine, Schedule};
use crate::ops::OpsRuntime;
use crate::proto::AgentDown;

pub type SharedState = Arc<AppState>;

#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeLive {
    pub cpu_pct: f32,
    pub memory_mib: f32,
    pub rx_bps: f32,
    pub tx_bps: f32,
}

pub(crate) const LOG_BUFFER: usize = 500;
pub(crate) const METRIC_BUFFER: usize = 120;
const STATE_PATH: &str = "data/state.json";

pub struct AppState {
    pub instances: RwLock<HashMap<String, Instance>>,
    pub schedules: RwLock<Vec<Schedule>>,
    pub events: broadcast::Sender<InstanceEvent>,
    pub log_buffers: RwLock<HashMap<String, VecDeque<LogLine>>>,
    pub metric_history: RwLock<HashMap<String, VecDeque<instance::MetricSample>>>,
    pub db: Mutex<rusqlite::Connection>,
    pub agents: Mutex<HashMap<String, mpsc::UnboundedSender<AgentDown>>>,
    pub http: reqwest::Client,
    pub ops: OpsRuntime,
    pub feed: RwLock<VecDeque<PanelEvent>>,
    pub node_live: RwLock<HashMap<String, NodeLive>>,
    pub plugin_host: String,
    pub plugin_token: String,
    pub env_api_token: Option<String>,
    pub env_webhook_url: Option<String>,
    pub bind: String,
}

#[derive(Serialize, Deserialize)]
struct PersistedState {
    instances: Vec<Instance>,
    #[serde(default)]
    schedules: Vec<Schedule>,
}

impl AppState {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(1024);
        let db = db::open().expect("open sqlite database (data/cocktail.db)");
        if let Err(e) = db::ensure_local_node(&db) {
            tracing::warn!(error = %e, "ensure local node");
        }
        let (instances, schedules) = hydrate_state(&db);
        let bind = std::env::var("COCKTAIL_BIND")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "0.0.0.0:11011".into());
        let env_api_token = std::env::var("COCKTAIL_API_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        let env_webhook_url = std::env::var("COCKTAIL_WEBHOOK_URL")
            .ok()
            .filter(|s| !s.is_empty());
        let plugin_host = crate::plugin_bridge::default_host_url();
        let plugin_token = crate::plugin_bridge::resolve_token();
        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let setup_pending = crate::auth::setup_required(&db).unwrap_or(true);
        let state = Self {
            instances: RwLock::new(instances),
            schedules: RwLock::new(schedules),
            events,
            log_buffers: RwLock::new(HashMap::new()),
            metric_history: RwLock::new(HashMap::new()),
            db: Mutex::new(db),
            agents: Mutex::new(HashMap::new()),
            http,
            ops: OpsRuntime::new(),
            feed: RwLock::new(VecDeque::new()),
            node_live: RwLock::new(HashMap::new()),
            plugin_host,
            plugin_token,
            env_api_token,
            env_webhook_url,
            bind,
        };
        if setup_pending {
            tracing::warn!("super-admin not initialized — first-run setup required");
        } else {
            tracing::info!("super-admin auth enabled (sqlite)");
        }
        if state.env_api_token.is_some() {
            tracing::info!("machine API token also enabled (COCKTAIL_API_TOKEN)");
        }
        state
    }

    pub async fn effective_webhook(&self) -> Option<String> {
        let conn = self.db.lock().await;
        let row = db::panel(&conn).ok();
        drop(conn);
        if let Some(url) = row
            .and_then(|r| r.webhook_url)
            .filter(|s| !s.is_empty())
        {
            return Some(url);
        }
        self.env_webhook_url.clone()
    }

    pub async fn bearer_ok(&self, token: &str) -> bool {
        if self
            .env_api_token
            .as_ref()
            .is_some_and(|expected| expected == token)
        {
            return true;
        }
        if !self.plugin_token.is_empty() && self.plugin_token == token {
            return true;
        }
        let conn = self.db.lock().await;
        db::session_admin(&conn, token)
            .ok()
            .flatten()
            .is_some()
    }

    pub fn publish(&self, event: InstanceEvent) {
        let _ = self.events.send(event);
    }

    pub fn spawn_event_applier(self: &Arc<Self>) {
        let state = Arc::clone(self);
        let mut rx = self.events.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        instance::apply_event(&state, &event).await;
                        if let InstanceEvent::Log {
                            instance_id,
                            line,
                        } = &event
                        {
                            let mut buffers = state.log_buffers.write().await;
                            let buf = buffers
                                .entry(instance_id.clone())
                                .or_insert_with(VecDeque::new);
                            buf.push_back(line.clone());
                            while buf.len() > LOG_BUFFER {
                                buf.pop_front();
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        instance::recover_stale_statuses(&state).await;
                        continue;
                    }
                    Err(_) => break,
                }
            }
        });
    }

    pub fn spawn_scheduler(self: &Arc<Self>) {
        let state = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                interval.tick().await;
                instance::run_due_schedules(&state).await;
            }
        });
    }

    pub fn spawn_reconciler(self: &Arc<Self>) {
        let state = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                instance::reconcile_local(&state).await;
            }
        });
    }

    pub async fn persist(&self) -> anyhow::Result<()> {
        let guard = self.instances.read().await;
        let instances: Vec<Instance> = guard.values().map(|i| i.persist_snapshot()).collect();
        drop(guard);
        let schedules = self.schedules.read().await.clone();

        {
            let conn = self.db.lock().await;
            db::replace_instances(&conn, &instances)?;
        }

        let path = PathBuf::from(STATE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(&PersistedState {
            instances,
            schedules,
        })?;
        fs::write(path, raw)?;
        Ok(())
    }

    pub async fn recent_logs(&self, id: &str) -> Vec<LogLine> {
        self.log_buffers
            .read()
            .await
            .get(id)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn recent_metrics(&self, id: &str) -> Vec<instance::MetricSample> {
        self.metric_history
            .read()
            .await
            .get(id)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }
}

fn hydrate_state(
    db: &rusqlite::Connection,
) -> (HashMap<String, Instance>, Vec<Schedule>) {
    let (json_map, schedules) = load_from_disk().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to load persisted state.json");
        (HashMap::new(), Vec::new())
    });
    match db::load_instances(db) {
        Ok(list) if !list.is_empty() => {
            let mut map = HashMap::new();
            for inst in list {
                map.insert(inst.id.clone(), inst);
            }
            (map, schedules)
        }
        Ok(_) => {
            if !json_map.is_empty() {
                let snap: Vec<_> = json_map.values().map(|i| i.persist_snapshot()).collect();
                if let Err(e) = db::replace_instances(db, &snap) {
                    tracing::warn!(error = %e, "migrate instances into sqlite");
                } else {
                    tracing::info!(
                        count = snap.len(),
                        "migrated instances from state.json into sqlite"
                    );
                }
            }
            (json_map, schedules)
        }
        Err(e) => {
            tracing::warn!(error = %e, "load instances from sqlite");
            (json_map, schedules)
        }
    }
}

fn load_from_disk() -> anyhow::Result<(HashMap<String, Instance>, Vec<Schedule>)> {
    let path = PathBuf::from(STATE_PATH);
    if !path.exists() {
        return Ok((HashMap::new(), Vec::new()));
    }
    let raw = fs::read_to_string(path)?;
    let persisted: PersistedState = serde_json::from_str(&raw)?;
    let mut map = HashMap::new();
    for mut inst in persisted.instances {
        inst.process = None;
        inst.last_metrics = None;
        inst.last_players.clear();
        if inst.spec.node_id.is_empty() {
            inst.spec.node_id = "local".into();
        }
        if inst.generation == 0 {
            inst.generation = 1;
        }
        map.insert(inst.id.clone(), inst);
    }
    Ok((map, persisted.schedules))
}
