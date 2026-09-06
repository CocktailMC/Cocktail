//! Game-port firewall and IP controls. Rules live in an isolated nft/iptables
//! chain named `cocktail` so the rest of the host firewall is left alone.

use std::net::IpAddr;
use std::process::{Command, Stdio};

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::NetopsRule;
use crate::instance::CommandRequest;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct NetopsStatus {
    pub backend: String,
    pub privileged: bool,
    pub nft: bool,
    pub iptables: bool,
    pub conntrack: bool,
    pub ss: bool,
    pub game_ports: Vec<u16>,
    pub hint: String,
    pub rules: Vec<NetopsRule>,
}

#[derive(Debug, Deserialize)]
pub struct CreateNetopsRequest {
    pub cidr: String,
    #[serde(default = "default_verdict")]
    pub verdict: String,
    #[serde(default = "default_proto")]
    pub proto: String,
    pub port: Option<u16>,
    pub instance_id: Option<String>,
    #[serde(default)]
    pub ttl_secs: u64,
    pub comment: Option<String>,
    #[serde(default = "default_true")]
    pub firewall: bool,
    #[serde(default = "default_true")]
    pub drop_conns: bool,
    #[serde(default)]
    pub game_ban: bool,
}

fn default_verdict() -> String {
    "drop".into()
}
fn default_proto() -> String {
    "both".into()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct KickRequest {
    pub cidr: String,
    pub port: Option<u16>,
}

struct Target {
    cidr: String,
    ip: IpAddr,
    prefix: u8,
}

pub async fn status(state: &AppState) -> NetopsStatus {
    let (nft, iptables, conntrack, ss, privileged) = tokio::task::spawn_blocking(|| {
        (
            has_cmd("nft"),
            has_cmd("iptables"),
            has_cmd("conntrack"),
            has_cmd("ss"),
            is_privileged(),
        )
    })
    .await
    .unwrap_or((false, false, false, false, false));
    let game_ports = game_ports(state).await;
    let rules = {
        let conn = state.db.lock().await;
        crate::db::list_netops(&conn).unwrap_or_default()
    };
    let backend = if nft {
        "nftables"
    } else if iptables {
        "iptables"
    } else {
        "none"
    };
    let hint = if !privileged {
        "控制面没有 NET_ADMIN/root，防火墙规则会记下来但无法写入内核；仍可踢连接（若有权限）和游戏 ban-ip。".into()
    } else if backend == "none" {
        "未找到 nft/iptables，只能做游戏 ban-ip 与 ss 踢连接。".into()
    } else {
        format!("规则写入独立 {backend} 对象 cocktail，只匹配下方游戏端口，不会改默认策略或 SSH。")
    };
    NetopsStatus {
        backend: backend.into(),
        privileged,
        nft,
        iptables,
        conntrack,
        ss,
        game_ports,
        hint,
        rules,
    }
}

pub async fn create(state: &AppState, req: CreateNetopsRequest) -> anyhow::Result<NetopsRule> {
    let target = parse_target(&req.cidr)?;
    let verdict = match req.verdict.to_ascii_lowercase().as_str() {
        "reject" | "rst" => "reject",
        _ => "drop",
    }
    .to_string();
    let proto = match req.proto.to_ascii_lowercase().as_str() {
        "tcp" => "tcp",
        "udp" => "udp",
        _ => "both",
    }
    .to_string();
    if !req.firewall && !req.drop_conns && !req.game_ban {
        anyhow::bail!("请至少选择：防火墙、踢连接、或游戏封禁");
    }
    let port = match (req.port, req.instance_id.as_deref()) {
        (Some(p), _) => {
            if p == 0 {
                anyhow::bail!("端口无效");
            }
            Some(p)
        }
        (None, Some(id)) => {
            let g = state.instances.read().await;
            let inst = g
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
            Some(inst.spec.port).filter(|p| *p > 0)
        }
        _ => None,
    };
    if let Some(id) = req.instance_id.as_deref() {
        if state.instances.read().await.get(id).is_none() {
            anyhow::bail!("instance not found");
        }
    }
    let comment = req
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(120).collect::<String>());
    let now = Utc::now();
    let ttl = if req.ttl_secs == 0 {
        0
    } else {
        req.ttl_secs.clamp(30, 30 * 24 * 3600)
    };
    let expires_at = if ttl > 0 {
        Some((now + Duration::seconds(ttl as i64)).to_rfc3339())
    } else {
        None
    };
    let mut rule = NetopsRule {
        id: Uuid::new_v4().to_string(),
        cidr: target.cidr.clone(),
        verdict,
        proto,
        port,
        instance_id: req.instance_id.clone(),
        ttl_secs: ttl,
        expires_at,
        comment,
        game_ban: req.game_ban,
        created_at: now.to_rfc3339(),
        applied: false,
        apply_error: None,
    };

    if req.drop_conns {
        let ports = resolve_ports(state, &rule).await;
        let cidr = target.cidr.clone();
        tokio::task::spawn_blocking(move || kick_conns(&cidr, &ports))
            .await
            .ok();
    }
    if req.game_ban {
        game_command(state, req.instance_id.as_deref(), &target, true).await;
    }

    if req.firewall {
        {
            let conn = state.db.lock().await;
            crate::db::insert_netops(&conn, &rule)?;
        }
        if let Err(e) = apply_all(state).await {
            tracing::warn!(error = %e, "netops firewall apply");
        }
        let conn = state.db.lock().await;
        rule = crate::db::list_netops(&conn)?
            .into_iter()
            .find(|r| r.id == rule.id)
            .unwrap_or(rule);
    } else if req.game_ban {
        let conn = state.db.lock().await;
        crate::db::insert_netops(&conn, &rule)?;
    }

    crate::util::audit(
        "netops.block",
        req.instance_id.as_deref(),
        serde_json::json!({
            "cidr": rule.cidr,
            "verdict": rule.verdict,
            "port": rule.port,
            "firewall": req.firewall,
            "game_ban": req.game_ban,
        }),
        "api",
    );
    Ok(rule)
}

pub async fn delete(state: &AppState, id: &str) -> anyhow::Result<()> {
    let rule = {
        let conn = state.db.lock().await;
        crate::db::delete_netops(&conn, id)?
    };
    let Some(rule) = rule else {
        anyhow::bail!("rule not found");
    };
    if rule.game_ban {
        if let Ok(target) = parse_target(&rule.cidr) {
            game_command(state, rule.instance_id.as_deref(), &target, false).await;
        }
    }
    apply_all(state).await?;
    crate::util::audit(
        "netops.unblock",
        rule.instance_id.as_deref(),
        serde_json::json!({ "cidr": rule.cidr, "id": id }),
        "api",
    );
    Ok(())
}

pub async fn kick(state: &AppState, req: KickRequest) -> anyhow::Result<()> {
    let target = parse_target(&req.cidr)?;
    let ports = if let Some(p) = req.port.filter(|p| *p > 0) {
        vec![p]
    } else {
        game_ports(state).await
    };
    let cidr = target.cidr.clone();
    tokio::task::spawn_blocking(move || kick_conns(&cidr, &ports))
        .await
        .ok();
    crate::util::audit(
        "netops.kick",
        None,
        serde_json::json!({ "cidr": target.cidr }),
        "api",
    );
    Ok(())
}

pub async fn resync(state: &AppState) -> anyhow::Result<NetopsStatus> {
    expire_now(state).await?;
    apply_all(state).await?;
    Ok(status(state).await)
}

pub async fn try_apply(state: &AppState) -> anyhow::Result<()> {
    apply_all(state).await
}

pub async fn expire_now(state: &AppState) -> anyhow::Result<bool> {
    let n = {
        let conn = state.db.lock().await;
        crate::db::expire_netops(&conn, &Utc::now().to_rfc3339())?
    };
    if n > 0 {
        apply_all(state).await?;
        return Ok(true);
    }
    Ok(false)
}

async fn apply_all(state: &AppState) -> anyhow::Result<()> {
    let rules = {
        let conn = state.db.lock().await;
        crate::db::list_netops(&conn)?
    };
    let ports_all = game_ports(state).await;
    let mut expanded = Vec::new();
    for rule in &rules {
        let ports = if let Some(p) = rule.port {
            vec![p]
        } else {
            ports_all.clone()
        };
        expanded.push((rule.clone(), ports));
    }
    let result = tokio::task::spawn_blocking(move || apply_firewall(&expanded))
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())));
    let conn = state.db.lock().await;
    match result {
        Ok(()) => crate::db::mark_netops_applied(&conn, true, None)?,
        Err(e) => {
            let msg = e.to_string();
            crate::db::mark_netops_applied(&conn, false, Some(&msg))?;
            anyhow::bail!(msg);
        }
    }
    Ok(())
}

async fn game_ports(state: &AppState) -> Vec<u16> {
    let g = state.instances.read().await;
    let mut ports: Vec<u16> = g
        .values()
        .map(|i| i.spec.port)
        .filter(|p| *p > 0)
        .collect();
    ports.sort();
    ports.dedup();
    ports
}

async fn resolve_ports(state: &AppState, rule: &NetopsRule) -> Vec<u16> {
    if let Some(p) = rule.port {
        return vec![p];
    }
    game_ports(state).await
}

async fn game_command(state: &AppState, instance_id: Option<&str>, target: &Target, ban: bool) {
    if target.prefix != 32 && target.prefix != 128 {
        return;
    }
    let ip = display_ip(target.ip);
    let cmd = if ban {
        format!("ban-ip {ip}")
    } else {
        format!("pardon-ip {ip}")
    };
    let ids: Vec<String> = if let Some(id) = instance_id {
        vec![id.to_string()]
    } else {
        state
            .instances
            .read()
            .await
            .values()
            .filter(|i| i.status == crate::instance::InstanceStatus::Running)
            .map(|i| i.id.clone())
            .collect()
    };
    for id in ids {
        let _ = crate::instance::send_command(
            state,
            &id,
            CommandRequest { command: cmd.clone() },
        )
        .await;
    }
}

fn parse_target(raw: &str) -> anyhow::Result<Target> {
    let s = raw.trim();
    if s.is_empty() {
        anyhow::bail!("请填写 IP 或 CIDR");
    }
    let (ip_s, prefix_s) = match s.split_once('/') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (s, None),
    };
    let ip: IpAddr = ip_s
        .parse()
        .map_err(|_| anyhow::anyhow!("无法解析地址 {ip_s}"))?;
    let prefix = if let Some(p) = prefix_s {
        p.parse::<u8>()
            .map_err(|_| anyhow::anyhow!("前缀长度无效"))?
    } else if ip.is_ipv4() {
        32
    } else {
        128
    };
    match ip {
        IpAddr::V4(v) => {
            if prefix < 16 || prefix > 32 {
                anyhow::bail!("IPv4 前缀只允许 /16 到 /32，避免误封整段公网");
            }
            if v.is_unspecified() || v.is_multicast() || v.is_broadcast() {
                anyhow::bail!("不能拉黑未指定/组播/广播地址");
            }
            if v.is_loopback() {
                anyhow::bail!("不能拉黑回环地址（本机 status ping 会断）");
            }
            if v.is_link_local() {
                anyhow::bail!("不能拉黑链路本地地址");
            }
        }
        IpAddr::V6(v) => {
            if prefix < 32 || prefix > 128 {
                anyhow::bail!("IPv6 前缀只允许 /32 到 /128");
            }
            if v.is_unspecified() || v.is_multicast() || v.is_loopback() {
                anyhow::bail!("不能拉黑未指定/组播/回环 IPv6");
            }
        }
    }
    let cidr = format!("{}/{prefix}", display_ip(ip));
    Ok(Target { cidr, ip, prefix })
}

fn display_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v) => v.to_string(),
        IpAddr::V6(v) => v.to_string(),
    }
}

fn apply_firewall(rules: &[(NetopsRule, Vec<u16>)]) -> anyhow::Result<()> {
    if has_cmd("nft") {
        apply_nft(rules)
    } else if has_cmd("iptables") {
        apply_iptables(rules)
    } else if rules.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("系统没有 nftables/iptables")
    }
}

fn apply_nft(rules: &[(NetopsRule, Vec<u16>)]) -> anyhow::Result<()> {
    let _ = run_nft("add table inet cocktail\n");
    let _ = run_nft(
        "add chain inet cocktail inbound { type filter hook input priority -5; policy accept; }\n",
    );
    let mut script = String::from("flush chain inet cocktail inbound\n");
    for (rule, ports) in rules {
        let v4 = !rule.cidr.contains(':');
        let family = if v4 { "ip" } else { "ip6" };
        let verdict = if rule.verdict == "reject" {
            if v4 {
                "reject with icmp type host-prohibited"
            } else {
                "reject with icmpv6 type admin-prohibited"
            }
        } else {
            "drop"
        };
        let protos: &[&str] = match rule.proto.as_str() {
            "tcp" => &["tcp"],
            "udp" => &["udp"],
            _ => &["tcp", "udp"],
        };
        let tag = rule.id.chars().take(8).collect::<String>();
        for port in ports {
            for proto in protos {
                script.push_str(&format!(
                    "add rule inet cocktail inbound {family} saddr {} {proto} dport {port} {verdict} comment \"c:{tag}\"\n",
                    rule.cidr
                ));
            }
        }
    }
    run_nft(&script)
}

fn run_nft(script: &str) -> anyhow::Result<()> {
    let mut child = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("无法执行 nft: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(script.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("nft: {err}");
    }
    Ok(())
}

fn apply_iptables(rules: &[(NetopsRule, Vec<u16>)]) -> anyhow::Result<()> {
    ensure_ipt_chain("iptables")?;
    if has_cmd("ip6tables") {
        let _ = ensure_ipt_chain("ip6tables");
    }
    for (rule, ports) in rules {
        let tool = if rule.cidr.contains(':') {
            "ip6tables"
        } else {
            "iptables"
        };
        let jump = if rule.verdict == "reject" {
            vec!["-j", "REJECT"]
        } else {
            vec!["-j", "DROP"]
        };
        let protos: &[&str] = match rule.proto.as_str() {
            "tcp" => &["tcp"],
            "udp" => &["udp"],
            _ => &["tcp", "udp"],
        };
        for port in ports {
            for proto in protos {
                let mut args = vec![
                    "-A".into(),
                    "COCKTAIL".into(),
                    "-s".into(),
                    rule.cidr.clone(),
                    "-p".into(),
                    proto.to_string(),
                    "--dport".into(),
                    port.to_string(),
                ];
                args.extend(jump.iter().map(|s| (*s).to_string()));
                let status = Command::new(tool).args(&args).output()?;
                if !status.status.success() {
                    anyhow::bail!(
                        "{tool}: {}",
                        String::from_utf8_lossy(&status.stderr)
                    );
                }
            }
        }
    }
    Ok(())
}

fn ensure_ipt_chain(tool: &str) -> anyhow::Result<()> {
    let _ = Command::new(tool).args(["-N", "COCKTAIL"]).output();
    let _ = Command::new(tool).args(["-F", "COCKTAIL"]).output();
    let check = Command::new(tool)
        .args(["-C", "INPUT", "-j", "COCKTAIL"])
        .output()?;
    if !check.status.success() {
        let ins = Command::new(tool)
            .args(["-I", "INPUT", "1", "-j", "COCKTAIL"])
            .output()?;
        if !ins.status.success() {
            anyhow::bail!(
                "{tool} 无法挂接 COCKTAIL 链: {}",
                String::from_utf8_lossy(&ins.stderr)
            );
        }
    }
    Ok(())
}

fn kick_conns(cidr: &str, ports: &[u16]) {
    let ip = cidr.split_once('/').map(|(a, _)| a).unwrap_or(cidr);
    if has_cmd("conntrack") {
        let _ = Command::new("conntrack")
            .args(["-D", "-s", ip])
            .output();
        let _ = Command::new("conntrack")
            .args(["-D", "-d", ip])
            .output();
    }
    if has_cmd("ss") {
        for port in ports {
            let _ = Command::new("ss")
                .args(["-K", "dst", ip, "dport", "=", &format!(":{port}")])
                .output();
        }
        if ports.is_empty() {
            let _ = Command::new("ss").args(["-K", "dst", ip]).output();
        }
    }
}

fn has_cmd(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_privileged() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}
