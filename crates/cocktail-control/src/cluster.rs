use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::auth;
use crate::db;
use crate::instance::InstanceEvent;
use crate::proto::{AgentDown, AgentUp, ApplyInstance};
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct AgentWsQuery {
    pub token: String,
}

pub async fn agent_ws(
    ws: WebSocketUpgrade,
    Query(q): Query<AgentWsQuery>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let token = q.token;
    let conn = state.db.lock().await;
    let node = match find_agent_node(&conn, &token) {
        Ok(Some(n)) => n,
        _ => {
            drop(conn);
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
    };
    drop(conn);
    let node_id = node.id;
    ws.on_upgrade(move |socket| run_session(state, node_id, socket))
        .into_response()
}

#[derive(Clone)]
struct NodeAuth {
    id: String,
}

fn find_agent_node(conn: &rusqlite::Connection, token: &str) -> anyhow::Result<Option<NodeAuth>> {
    for row in db::list_nodes(conn)? {
        if row.kind != "agent" {
            continue;
        }
        let Some(hash) = row.token_hash.as_deref() else {
            continue;
        };
        if auth::verify_password(token, hash) {
            return Ok(Some(NodeAuth { id: row.id }));
        }
    }
    Ok(None)
}

async fn run_session(state: SharedState, node_id: String, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<AgentDown>();
    {
        let mut agents = state.agents.lock().await;
        agents.insert(node_id.clone(), tx);
    }
    tracing::info!(%node_id, "agent connected");

    let snapshot: Vec<ApplyInstance> = {
        let guard = state.instances.read().await;
        guard
            .values()
            .filter(|i| i.spec.node_id == node_id)
            .map(ApplyInstance::from)
            .collect()
    };
    let welcome = AgentDown::Welcome {
        node_id: node_id.clone(),
        instances: snapshot,
        protocol_version: crate::proto::PROTOCOL_VERSION,
    };

    if sink
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        let mut agents = state.agents.lock().await;
        agents.remove(&node_id);
        return;
    }

    let mut out_seq: u64 = 0;
    loop {
        tokio::select! {
            Some(down) = rx.recv() => {
                out_seq += 1;
                let down = down.with_seq(out_seq);
                let Ok(text) = serde_json::to_string(&down) else { continue };
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(up) = serde_json::from_str::<AgentUp>(text.as_str()) {
                            handle_up(&state, &node_id, up).await;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    let mut agents = state.agents.lock().await;
    agents.remove(&node_id);
    tracing::info!(%node_id, "agent disconnected");
}

async fn handle_up(state: &SharedState, node_id: &str, up: AgentUp) {
    match up {
        AgentUp::Hello {
            hostname,
            os,
            arch,
            protocol_version,
        } => {
            let conn = state.db.lock().await;
            let _ = db::set_node_protocol(&conn, node_id, protocol_version as i64);
            drop(conn);
            state
                .node_live
                .write()
                .await
                .entry(node_id.to_string())
                .or_default()
                .protocol_version = protocol_version;
            let conn = state.db.lock().await;
            let _ = db::touch_node(&conn, node_id, Some(&hostname), Some(&os), Some(&arch));
        }
        AgentUp::Heartbeat {
            cpu_pct,
            memory_mib,
            rx_bps,
            tx_bps,
            nic_stats,
            tcp_states,
        } => {
            let conn = state.db.lock().await;
            let _ = db::touch_node(&conn, node_id, None, None, None);
            drop(conn);
            let mut live = state.node_live.write().await;
            let entry = live.entry(node_id.to_string()).or_default();
            entry.cpu_pct = cpu_pct;
            entry.memory_mib = memory_mib;
            entry.rx_bps = rx_bps;
            entry.tx_bps = tx_bps;
            entry.nics = nic_stats;
            entry.tcp_estab = tcp_states.estab;
            entry.tcp_syn_recv = tcp_states.syn_recv;
            entry.tcp_time_wait = tcp_states.time_wait;
        }
        AgentUp::Status {
            instance_id,
            status,
            pid,
        } => {
            let mut guard = state.instances.write().await;
            if let Some(inst) = guard.get_mut(&instance_id) {
                if inst.spec.node_id != node_id {
                    return;
                }
                inst.status = status;
                inst.last_pid = pid;
                inst.updated_at = Utc::now();
            }
            drop(guard);
            state.publish(InstanceEvent::StatusChanged {
                instance_id,
                status,
                at: Utc::now(),
            });
            let _ = state.persist().await;
        }
        AgentUp::Log { instance_id, line } => {
            let ok = state
                .instances
                .read()
                .await
                .get(&instance_id)
                .is_some_and(|i| i.spec.node_id == node_id);
            if ok {
                state.publish(InstanceEvent::Log { instance_id, line });
            }
        }
        AgentUp::Metric {
            instance_id,
            sample,
        } => {
            let ok = state
                .instances
                .read()
                .await
                .get(&instance_id)
                .is_some_and(|i| i.spec.node_id == node_id);
            if ok {
                state.publish(InstanceEvent::Metric {
                    instance_id,
                    sample,
                });
            }
        }
        AgentUp::Ack { seq } => {
            let conn = state.db.lock().await;
            let _ = db::set_node_seq(&conn, node_id, seq as i64);
            drop(conn);
            if let Some(entry) = state.node_live.write().await.get_mut(node_id) {
                entry.last_ack_seq = seq;
            }
        }
    }
}

pub async fn send_down(
    state: &crate::state::AppState,
    node_id: &str,
    msg: AgentDown,
) -> anyhow::Result<()> {
    let agents = state.agents.lock().await;
    let tx = agents
        .get(node_id)
        .ok_or_else(|| anyhow::anyhow!("节点 {node_id} 离线：请在该机器上运行 cocktail-agent"))?;
    tx.send(msg)
        .map_err(|_| anyhow::anyhow!("节点通道已关闭"))?;
    Ok(())
}

pub async fn list_views(state: &crate::state::AppState) -> anyhow::Result<Vec<db::NodeView>> {
    let conn = state.db.lock().await;
    let mut nodes = db::list_nodes(&conn)?;
    drop(conn);
    let agents = state.agents.lock().await;
    let live = state.node_live.read().await;
    let inst_counts = {
        let g = state.instances.read().await;
        let mut m = std::collections::HashMap::<String, usize>::new();
        for i in g.values() {
            *m.entry(i.spec.node_id.clone()).or_insert(0) += 1;
        }
        m
    };
    let mut out = Vec::new();
    for n in nodes.drain(..) {
        let online = n.kind == "local" || agents.contains_key(&n.id);
        let mut v = n.into_view(online);
        if let Some(s) = live.get(&v.id) {
            v.cpu_pct = s.cpu_pct;
            v.memory_mib = s.memory_mib;
            v.rx_bps = s.rx_bps;
            v.tx_bps = s.tx_bps;
            v.nics = s.nics.clone();
            v.tcp_estab = s.tcp_estab;
            v.tcp_syn_recv = s.tcp_syn_recv;
            v.tcp_time_wait = s.tcp_time_wait;
            if s.protocol_version > 0 {
                v.protocol_version = s.protocol_version as i64;
            }
            v.last_seq = s.last_ack_seq as i64;
        }
        v.instance_count = inst_counts.get(&v.id).copied().unwrap_or(0);
        out.push(v);
    }
    Ok(out)
}

pub async fn create_node(
    state: &crate::state::AppState,
    name: &str,
) -> anyhow::Result<(db::NodeView, String)> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("节点名称不能为空");
    }
    let token = format!("cn_{}", uuid::Uuid::new_v4());
    let hash = auth::hash_password(&token)?;
    let id = uuid::Uuid::new_v4().to_string();
    let conn = state.db.lock().await;
    db::insert_agent_node(&conn, &id, name, &hash)?;
    let row = db::get_node(&conn, &id)?.ok_or_else(|| anyhow::anyhow!("node insert failed"))?;
    drop(conn);
    crate::util::audit(
        "node.create",
        None,
        serde_json::json!({ "id": id, "name": name }),
        "api",
    );
    Ok((row.into_view(false), token))
}

pub async fn delete_node(state: &crate::state::AppState, id: &str) -> anyhow::Result<()> {
    if crate::instance::is_local_node(id) {
        anyhow::bail!("不能删除本机节点");
    }
    let used = {
        let guard = state.instances.read().await;
        guard.values().any(|i| i.spec.node_id == id)
    };
    if used {
        anyhow::bail!("该节点上仍有实例，请先迁移或删除");
    }
    let conn = state.db.lock().await;
    db::delete_node(&conn, id)?;
    drop(conn);
    crate::util::audit("node.delete", None, serde_json::json!({ "id": id }), "api");
    Ok(())
}

pub async fn node_exists(state: &crate::state::AppState, id: &str) -> bool {
    if crate::instance::is_local_node(id) {
        return true;
    }
    let conn = state.db.lock().await;
    db::get_node(&conn, id).ok().flatten().is_some()
}
