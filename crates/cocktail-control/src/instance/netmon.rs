//! Per-instance network snapshot: sockets on the game port, rates, status ping, alerts.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::model::NetPeer;

#[derive(Debug, Clone, Default)]
pub struct NetCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_pkts: u64,
    pub tx_pkts: u64,
    pub baseline_rx: u64,
    pub baseline_tx: u64,
    pub peak_rx_bps: f32,
    pub peak_tx_bps: f32,
    pub at: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct NetSample {
    pub listen: Option<String>,
    pub connections: u32,
    pub unique_ips: u32,
    pub syn_recv: u32,
    pub time_wait: u32,
    pub fin_wait: u32,
    pub udp: u32,
    pub rx_bps: f32,
    pub tx_bps: f32,
    pub rx_pps: f32,
    pub tx_pps: f32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub session_rx: u64,
    pub session_tx: u64,
    pub peak_rx_bps: f32,
    pub peak_tx_bps: f32,
    pub drops: u64,
    pub errors: u64,
    pub rtt_ms: Option<f32>,
    pub ping_online: Option<u32>,
    pub ping_max: Option<u32>,
    pub ping_version: Option<String>,
    pub source: &'static str,
    pub alerts: Vec<String>,
    pub peers: Vec<NetPeer>,
    pub counters: NetCounters,
}

struct Sock {
    local_ip: IpAddr,
    remote_ip: IpAddr,
    #[allow(dead_code)]
    remote_port: u16,
    listen: bool,
    established: bool,
    syn_recv: bool,
    time_wait: bool,
    fin_wait: bool,
    udp: bool,
}

pub fn sample(port: u16, pid: u32, docker: bool, prev: &NetCounters) -> NetSample {
    if port == 0 {
        return NetSample::default();
    }
    let sockets = collect_sockets(port, pid, docker);
    let listen = sockets
        .iter()
        .find(|s| s.listen && !s.udp)
        .map(|s| format!("{}:{}", display_ip(s.local_ip), port));

    let mut by_ip: HashMap<IpAddr, (u32, bool)> = HashMap::new();
    let mut connections = 0u32;
    let mut syn_recv = 0u32;
    let mut time_wait = 0u32;
    let mut fin_wait = 0u32;
    let mut udp = 0u32;
    for s in &sockets {
        if s.udp {
            udp += 1;
            continue;
        }
        if s.syn_recv {
            syn_recv += 1;
        }
        if s.time_wait {
            time_wait += 1;
        }
        if s.fin_wait {
            fin_wait += 1;
        }
        if s.established && !s.remote_ip.is_unspecified() {
            connections += 1;
            let e = by_ip.entry(s.remote_ip).or_insert((0, s.remote_ip.is_ipv6()));
            e.0 += 1;
        }
    }
    let mut peers: Vec<NetPeer> = by_ip
        .into_iter()
        .map(|(ip, (n, v6))| NetPeer {
            ip: display_ip(ip),
            connections: n,
            scope: ip_scope(ip).into(),
            ipv6: v6,
        })
        .collect();
    peers.sort_by(|a, b| b.connections.cmp(&a.connections).then(a.ip.cmp(&b.ip)));
    let unique_ips = peers.len() as u32;
    peers.truncate(48);

    let traffic = byte_counters(port, pid, docker);
    let now = Instant::now();
    let primed = prev.at.is_some();
    let (rx_bps, tx_bps, rx_pps, tx_pps) = if let Some(prev_at) = prev.at {
        let dt = now.saturating_duration_since(prev_at).as_secs_f32();
        if dt > 0.2 && traffic.rx >= prev.rx_bytes && traffic.tx >= prev.tx_bytes {
            (
                (traffic.rx.saturating_sub(prev.rx_bytes) as f32) / dt,
                (traffic.tx.saturating_sub(prev.tx_bytes) as f32) / dt,
                (traffic.rx_pkts.saturating_sub(prev.rx_pkts) as f32) / dt,
                (traffic.tx_pkts.saturating_sub(prev.tx_pkts) as f32) / dt,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    let baseline_rx = if primed {
        prev.baseline_rx
    } else {
        traffic.rx
    };
    let baseline_tx = if primed {
        prev.baseline_tx
    } else {
        traffic.tx
    };
    let peak_rx = prev.peak_rx_bps.max(rx_bps);
    let peak_tx = prev.peak_tx_bps.max(tx_bps);
    let ping = status_ping(port);
    let source = if docker && pid > 0 {
        "container"
    } else {
        "port"
    };
    let mut alerts = Vec::new();
    if syn_recv >= 16 {
        alerts.push(format!("半开连接 {syn_recv}，可能在被扫描或 SYN 洪水"));
    }
    if unique_ips >= 40 && connections >= 40 {
        alerts.push(format!("独立 IP {unique_ips}、连接 {connections}，注意是否被扫服"));
    }
    if connections >= 256 {
        alerts.push(format!("TCP 连接 {connections}，接近常见代理/人数上限"));
    }
    if let Some(rtt) = ping.as_ref().and_then(|p| p.rtt_ms) {
        if rtt >= 250.0 {
            alerts.push(format!("本机 status ping {rtt:.0} ms，服务端应答偏慢"));
        }
    }
    if traffic.drops > 0 && primed {
        alerts.push(format!("网卡丢包累计 {}", traffic.drops));
    }

    NetSample {
        listen,
        connections,
        unique_ips,
        syn_recv,
        time_wait,
        fin_wait,
        udp,
        rx_bps,
        tx_bps,
        rx_pps,
        tx_pps,
        rx_bytes: traffic.rx,
        tx_bytes: traffic.tx,
        session_rx: traffic.rx.saturating_sub(baseline_rx),
        session_tx: traffic.tx.saturating_sub(baseline_tx),
        peak_rx_bps: peak_rx,
        peak_tx_bps: peak_tx,
        drops: traffic.drops,
        errors: traffic.errors,
        rtt_ms: ping.as_ref().and_then(|p| p.rtt_ms),
        ping_online: ping.as_ref().and_then(|p| p.online),
        ping_max: ping.as_ref().and_then(|p| p.max),
        ping_version: ping.as_ref().and_then(|p| p.version.clone()),
        source,
        alerts,
        peers,
        counters: NetCounters {
            rx_bytes: traffic.rx,
            tx_bytes: traffic.tx,
            rx_pkts: traffic.rx_pkts,
            tx_pkts: traffic.tx_pkts,
            baseline_rx,
            baseline_tx,
            peak_rx_bps: peak_rx,
            peak_tx_bps: peak_tx,
            at: Some(now),
        },
    }
}

struct Traffic {
    rx: u64,
    tx: u64,
    rx_pkts: u64,
    tx_pkts: u64,
    drops: u64,
    errors: u64,
}

fn collect_sockets(port: u16, pid: u32, docker: bool) -> Vec<Sock> {
    let mut out = Vec::new();
    #[cfg(unix)]
    {
        let inner_port = if docker { 25565 } else { port };
        if docker && pid > 0 {
            let base = Path::new("/proc").join(pid.to_string()).join("net");
            parse_proc_table(&base.join("tcp"), inner_port, false, &mut out);
            parse_proc_table(&base.join("tcp6"), inner_port, false, &mut out);
            parse_proc_table(&base.join("udp"), inner_port, true, &mut out);
            parse_proc_table(&base.join("udp6"), inner_port, true, &mut out);
        }
        parse_proc_table(Path::new("/proc/net/tcp"), port, false, &mut out);
        parse_proc_table(Path::new("/proc/net/tcp6"), port, false, &mut out);
        parse_proc_table(Path::new("/proc/net/udp"), port, true, &mut out);
        parse_proc_table(Path::new("/proc/net/udp6"), port, true, &mut out);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, docker);
        if let Some(extra) = ss_sockets(port) {
            out.extend(extra);
        }
    }
    out
}

fn byte_counters(port: u16, pid: u32, docker: bool) -> Traffic {
    #[cfg(unix)]
    {
        if docker && pid > 0 {
            if let Some(v) =
                read_proc_net_dev(&Path::new("/proc").join(pid.to_string()).join("net/dev"))
            {
                return v;
            }
        }
        if let Some((rx, tx)) = ss_bytes(port) {
            return Traffic {
                rx,
                tx,
                rx_pkts: 0,
                tx_pkts: 0,
                drops: 0,
                errors: 0,
            };
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (port, pid, docker);
    }
    Traffic {
        rx: 0,
        tx: 0,
        rx_pkts: 0,
        tx_pkts: 0,
        drops: 0,
        errors: 0,
    }
}

fn parse_proc_table(path: &Path, port: u16, udp: bool, out: &mut Vec<Sock>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        let Some((local_ip, local_port)) = parse_addr(cols[1]) else {
            continue;
        };
        if local_port != port {
            continue;
        }
        let Some((remote_ip, remote_port)) = parse_addr(cols[2]) else {
            continue;
        };
        let Ok(st) = u8::from_str_radix(cols[3], 16) else {
            continue;
        };
        out.push(Sock {
            local_ip,
            remote_ip,
            remote_port,
            listen: !udp && st == 0x0A,
            established: !udp && st == 0x01,
            syn_recv: !udp && st == 0x03,
            time_wait: !udp && st == 0x06,
            fin_wait: !udp && (st == 0x04 || st == 0x05),
            udp,
        });
    }
}

fn parse_addr(raw: &str) -> Option<(IpAddr, u16)> {
    let (ip, port) = raw.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    if ip.len() == 8 {
        let n = u32::from_str_radix(ip, 16).ok()?;
        let b = n.to_le_bytes();
        Some((IpAddr::V4(Ipv4Addr::new(b[0], b[1], b[2], b[3])), port))
    } else if ip.len() == 32 {
        let mut bytes = [0u8; 16];
        for i in 0..4 {
            let word = u32::from_str_radix(ip.get(i * 8..i * 8 + 8)?, 16).ok()?;
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        Some((IpAddr::V6(Ipv6Addr::from(bytes)), port))
    } else {
        None
    }
}

fn read_proc_net_dev(path: &Path) -> Option<Traffic> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut t = Traffic {
        rx: 0,
        tx: 0,
        rx_pkts: 0,
        tx_pkts: 0,
        drops: 0,
        errors: 0,
    };
    for line in text.lines().skip(2) {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        if iface.trim() == "lo" {
            continue;
        }
        let cols: Vec<&str> = rest.split_whitespace().collect();
        if cols.len() < 16 {
            continue;
        }
        t.rx += cols[0].parse::<u64>().ok()?;
        t.rx_pkts += cols[1].parse::<u64>().ok()?;
        t.errors += cols[2].parse::<u64>().ok()?;
        t.drops += cols[3].parse::<u64>().ok()?;
        t.tx += cols[8].parse::<u64>().ok()?;
        t.tx_pkts += cols[9].parse::<u64>().ok()?;
        t.errors += cols[10].parse::<u64>().ok()?;
        t.drops += cols[11].parse::<u64>().ok()?;
    }
    Some(t)
}

fn ss_bytes(port: u16) -> Option<(u64, u64)> {
    let out = std::process::Command::new("ss")
        .args([
            "-H",
            "-ti",
            "state",
            "established",
            "sport",
            "=",
            &format!(":{port}"),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut rx = 0u64;
    let mut tx = 0u64;
    let mut any = false;
    for part in text.split_whitespace() {
        if let Some(v) = part.strip_prefix("bytes_received:") {
            if let Ok(n) = v.parse::<u64>() {
                rx += n;
                any = true;
            }
        }
        if let Some(v) = part.strip_prefix("bytes_acked:") {
            if let Ok(n) = v.parse::<u64>() {
                tx += n;
                any = true;
            }
        }
    }
    any.then_some((rx, tx))
}

struct StatusPing {
    rtt_ms: Option<f32>,
    online: Option<u32>,
    max: Option<u32>,
    version: Option<String>,
}

fn status_ping(port: u16) -> Option<StatusPing> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let start = Instant::now();
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(350)).ok()?;
    let _ = s.set_nodelay(true);
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(350)));
    let mut hs = Vec::new();
    write_varint(&mut hs, 0);
    write_varint(&mut hs, 767);
    write_mc_string(&mut hs, "127.0.0.1");
    hs.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut hs, 1);
    let mut pkt = Vec::new();
    write_varint(&mut pkt, hs.len() as i32);
    pkt.extend(hs);
    s.write_all(&pkt).ok()?;
    s.write_all(&[0x01, 0x00]).ok()?;
    s.flush().ok()?;
    let len = read_varint(&mut s)? as usize;
    if !(8..=262_144).contains(&len) {
        return None;
    }
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).ok()?;
    let mut cur = body.as_slice();
    let id = read_varint_slice(&mut cur)?;
    if id != 0 {
        return None;
    }
    let json = read_mc_string_slice(&mut cur)?;
    let rtt = start.elapsed().as_secs_f32() * 1000.0;
    let v: Value = serde_json::from_str(&json).ok()?;
    Some(StatusPing {
        rtt_ms: Some(rtt),
        online: v.pointer("/players/online").and_then(|x| x.as_u64()).map(|n| n as u32),
        max: v.pointer("/players/max").and_then(|x| x.as_u64()).map(|n| n as u32),
        version: v
            .pointer("/version/name")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
    })
}

fn write_varint(buf: &mut Vec<u8>, mut n: i32) {
    loop {
        let mut b = (n as u8) & 0x7f;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        buf.push(b);
        if n == 0 {
            break;
        }
    }
}

fn write_mc_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

fn read_varint<R: Read>(r: &mut R) -> Option<i32> {
    let mut n = 0i32;
    let mut shift = 0;
    for _ in 0..5 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b).ok()?;
        n |= i32::from(b[0] & 0x7f) << shift;
        if b[0] & 0x80 == 0 {
            return Some(n);
        }
        shift += 7;
    }
    None
}

fn read_varint_slice(cur: &mut &[u8]) -> Option<i32> {
    let mut n = 0i32;
    let mut shift = 0;
    for _ in 0..5 {
        if cur.is_empty() {
            return None;
        }
        let b = cur[0];
        *cur = &cur[1..];
        n |= i32::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(n);
        }
        shift += 7;
    }
    None
}

fn read_mc_string_slice(cur: &mut &[u8]) -> Option<String> {
    let len = read_varint_slice(cur)? as usize;
    if len > cur.len() {
        return None;
    }
    let s = std::str::from_utf8(&cur[..len]).ok()?.to_string();
    *cur = &cur[len..];
    Some(s)
}

fn ip_scope(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(v) => {
            if v.is_loopback() {
                "loopback"
            } else if v.is_private() || v.is_link_local() {
                "private"
            } else {
                "public"
            }
        }
        IpAddr::V6(v) => {
            if v.is_loopback() {
                "loopback"
            } else if v.to_ipv4_mapped().is_some_and(|v4| v4.is_private() || v4.is_loopback()) {
                if v.to_ipv4_mapped().unwrap().is_loopback() {
                    "loopback"
                } else {
                    "private"
                }
            } else if (v.segments()[0] & 0xfe00) == 0xfc00 {
                "private"
            } else {
                "public"
            }
        }
    }
}

#[cfg(not(unix))]
fn ss_sockets(port: u16) -> Option<Vec<Sock>> {
    let out = std::process::Command::new("ss")
        .args(["-H", "-tn", "sport", "=", &format!(":{port}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut socks = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        let state = cols[0];
        let Some(local) = parse_ss_endpoint(cols[3]) else {
            continue;
        };
        let remote = parse_ss_endpoint(cols[4]);
        socks.push(Sock {
            local_ip: local.0,
            remote_ip: remote.map(|r| r.0).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            remote_port: remote.map(|r| r.1).unwrap_or(0),
            listen: state.eq_ignore_ascii_case("LISTEN"),
            established: state.eq_ignore_ascii_case("ESTAB")
                || state.eq_ignore_ascii_case("ESTABLISHED"),
            syn_recv: state.eq_ignore_ascii_case("SYN-RECV"),
            time_wait: state.eq_ignore_ascii_case("TIME-WAIT"),
            fin_wait: state.to_ascii_uppercase().contains("FIN-WAIT"),
            udp: false,
        });
    }
    Some(socks)
}

#[cfg(not(unix))]
fn parse_ss_endpoint(s: &str) -> Option<(IpAddr, u16)> {
    let (ip, port) = s.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let ip = ip.trim_matches(|c| c == '[' || c == ']').parse().ok()?;
    Some((ip, port))
}

fn display_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v) => v.to_string(),
        IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v4| v4.to_string())
            .unwrap_or_else(|| v.to_string()),
    }
}

pub fn demo_sample(tick: u32, prev: &NetCounters) -> NetSample {
    let rx = 12_000 + u64::from(tick) * 800;
    let tx = 48_000 + u64::from(tick) * 2_400;
    let now = Instant::now();
    let dt = prev
        .at
        .map(|t| now.saturating_duration_since(t).as_secs_f32())
        .unwrap_or(3.0)
        .max(0.2);
    let rx_bps = (rx.saturating_sub(prev.rx_bytes) as f32) / dt;
    let tx_bps = (tx.saturating_sub(prev.tx_bytes) as f32) / dt;
    NetSample {
        listen: Some("0.0.0.0:25565".into()),
        connections: 2,
        unique_ips: 2,
        syn_recv: 0,
        time_wait: 1,
        fin_wait: 0,
        udp: 1,
        rx_bps,
        tx_bps,
        rx_pps: 12.0,
        tx_pps: 28.0,
        rx_bytes: rx,
        tx_bytes: tx,
        session_rx: rx.saturating_sub(if prev.at.is_some() { prev.baseline_rx } else { rx }),
        session_tx: tx.saturating_sub(if prev.at.is_some() { prev.baseline_tx } else { tx }),
        peak_rx_bps: prev.peak_rx_bps.max(rx_bps),
        peak_tx_bps: prev.peak_tx_bps.max(tx_bps),
        drops: 0,
        errors: 0,
        rtt_ms: Some(4.0),
        ping_online: Some(2),
        ping_max: Some(20),
        ping_version: Some("Demo 1.21".into()),
        source: "port",
        alerts: Vec::new(),
        peers: vec![
            NetPeer {
                ip: "127.0.0.1".into(),
                connections: 1,
                scope: "loopback".into(),
                ipv6: false,
            },
            NetPeer {
                ip: "10.0.0.20".into(),
                connections: 1,
                scope: "private".into(),
                ipv6: false,
            },
        ],
        counters: NetCounters {
            rx_bytes: rx,
            tx_bytes: tx,
            rx_pkts: u64::from(tick) * 40,
            tx_pkts: u64::from(tick) * 90,
            baseline_rx: if prev.at.is_some() { prev.baseline_rx } else { rx },
            baseline_tx: if prev.at.is_some() { prev.baseline_tx } else { tx },
            peak_rx_bps: prev.peak_rx_bps.max(rx_bps),
            peak_tx_bps: prev.peak_tx_bps.max(tx_bps),
            at: Some(now),
        },
    }
}
