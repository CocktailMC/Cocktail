use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::db;
use crate::instance::{self, Instance, InstanceEvent, InstanceStatus, LogLine, Schedule};

pub type SharedState = Arc<AppState>;

const LOG_BUFFER: usize = 500;
const STATE_PATH: &str = "data/state.json";

pub struct AppState {
    pub instances: RwLock<HashMap<String, Instance>>,
    pub schedules: RwLock<Vec<Schedule>>,
    pub events: broadcast::Sender<InstanceEvent>,
    pub log_buffers: RwLock<HashMap<String, VecDeque<LogLine>>>,
    pub db: Mutex<rusqlite::Connection>,
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
        let (instances, schedules) = load_from_disk().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to load persisted state");
            (HashMap::new(), Vec::new())
        });
        let db = db::open().expect("open sqlite database (data/cocktail.db)");
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
        let setup_pending = crate::auth::setup_required(&db).unwrap_or(true);
        let state = Self {
            instances: RwLock::new(instances),
            schedules: RwLock::new(schedules),
            events,
            log_buffers: RwLock::new(HashMap::new()),
            db: Mutex::new(db),
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
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
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

    pub async fn persist(&self) -> anyhow::Result<()> {
        let guard = self.instances.read().await;
        let instances: Vec<Instance> = guard
            .values()
            .map(|i| Instance {
                id: i.id.clone(),
                spec: i.spec.clone(),
                status: match i.status {
                    InstanceStatus::Running
                    | InstanceStatus::Starting
                    | InstanceStatus::Stopping => InstanceStatus::Stopped,
                    s => s,
                },
                created_at: i.created_at,
                updated_at: i.updated_at,
                last_metrics: None,
                last_players: Vec::new(),
                process: None,
            })
            .collect();
        drop(guard);
        let schedules = self.schedules.read().await.clone();

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
        if matches!(
            inst.status,
            InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
        ) {
            inst.status = InstanceStatus::Stopped;
        }
        inst.process = None;
        inst.last_metrics = None;
        inst.last_players.clear();
        map.insert(inst.id.clone(), inst);
    }
    Ok((map, persisted.schedules))
}
