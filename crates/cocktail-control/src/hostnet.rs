//! Host-wide NIC traffic and TCP state (not bound to a single game port).

use std::path::Path;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Default)]
pub struct HostNetPrev {
    pub rx: u64,
    pub tx: u64,
    pub rx_pkts: u64,
    pub tx_pkts: u64,
    pub drops: u64,
    pub errors: u64,
    pub at: Option<Instant>,
    pub peak_rx_bps: f32,
    pub peak_tx_bps: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct NicRow {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bps: f32,
    pub tx_bps: f32,
    pub drops: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceNetRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub port: u16,
    pub rx_bps: f32,
    pub tx_bps: f32,
    pub connections: u32,
    pub unique_ips: u32,
    pub alerts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostNetSample {
    pub ts: DateTime<Utc>,
    pub rx_bps: f32,
    pub tx_bps: f32,
    pub rx_pps: f32,
    pub tx_pps: f32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub peak_rx_bps: f32,
    pub peak_tx_bps: f32,
    pub drops: u64,
    pub errors: u64,
    pub tcp_estab: u32,
    pub syn_recv: u32,
    pub time_wait: u32,
    pub nics: Vec<NicRow>,
    pub instances: Vec<InstanceNetRow>,
    pub alerts: Vec<String>,
}

struct RawNic {
    name: String,
    rx: u64,
    tx: u64,
    rx_pkts: u64,
    tx_pkts: u64,
    drops: u64,
    errors: u64,
}

pub fn sample(prev: &HostNetPrev, rx_alert_bps: f32) -> (HostNetSample, HostNetPrev) {
    let nics = read_nics();
    let mut rx = 0u64;
    let mut tx = 0u64;
    let mut rx_pkts = 0u64;
    let mut tx_pkts = 0u64;
    let mut drops = 0u64;
    let mut errors = 0u64;
    for n in &nics {
        rx += n.rx;
        tx += n.tx;
        rx_pkts += n.rx_pkts;
        tx_pkts += n.tx_pkts;
        drops += n.drops;
        errors += n.errors;
    }
    let now = Instant::now();
    let (rx_bps, tx_bps, rx_pps, tx_pps) = if let Some(at) = prev.at {
        let dt = now.saturating_duration_since(at).as_secs_f32();
        if dt > 0.2 {
            (
                delta_rate(rx, prev.rx, dt),
                delta_rate(tx, prev.tx, dt),
                delta_rate(rx_pkts, prev.rx_pkts, dt),
                delta_rate(tx_pkts, prev.tx_pkts, dt),
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    let nic_rows: Vec<NicRow> = nics
        .iter()
        .map(|n| NicRow {
            name: n.name.clone(),
            rx_bytes: n.rx,
            tx_bytes: n.tx,
            rx_bps: if prev.at.is_some() {
                // Per-NIC rate is approximate from share of totals if we lack history.
                if rx > 0 {
                    rx_bps * (n.rx as f32 / rx as f32)
                } else {
                    0.0
                }
            } else {
                0.0
            },
            tx_bps: if prev.at.is_some() && tx > 0 {
                tx_bps * (n.tx as f32 / tx as f32)
            } else {
                0.0
            },
            drops: n.drops,
            errors: n.errors,
        })
        .collect();
    let (tcp_estab, syn_recv, time_wait) = tcp_states();
    let peak_rx = prev.peak_rx_bps.max(rx_bps);
    let peak_tx = prev.peak_tx_bps.max(tx_bps);
    let mut alerts = Vec::new();
    if syn_recv >= 32 {
        alerts.push(format!("主机 SYN-RECV {syn_recv}，可能正在被扫描"));
    }
    let thresh = if rx_alert_bps > 0.0 {
        rx_alert_bps
    } else {
        80.0 * 1024.0 * 1024.0
    };
    if prev.at.is_some() && rx_bps >= thresh {
        alerts.push(format!(
            "主机下行 {:.1} MiB/s，超过告警阈值",
            rx_bps / (1024.0 * 1024.0)
        ));
    }
    if prev.at.is_some() && drops > prev.drops {
        alerts.push(format!("主机网卡丢包 +{}", drops - prev.drops));
    }
    if prev.at.is_some() && errors > prev.errors {
        alerts.push(format!("主机网卡错误 +{}", errors - prev.errors));
    }
    let sample = HostNetSample {
        ts: Utc::now(),
        rx_bps,
        tx_bps,
        rx_pps,
        tx_pps,
        rx_bytes: rx,
        tx_bytes: tx,
        peak_rx_bps: peak_rx,
        peak_tx_bps: peak_tx,
        drops,
        errors,
        tcp_estab,
        syn_recv,
        time_wait,
        nics: nic_rows,
        instances: Vec::new(),
        alerts,
    };
    let next = HostNetPrev {
        rx,
        tx,
        rx_pkts,
        tx_pkts,
        drops,
        errors,
        at: Some(now),
        peak_rx_bps: peak_rx,
        peak_tx_bps: peak_tx,
    };
    (sample, next)
}

fn delta_rate(now: u64, prev: u64, dt: f32) -> f32 {
    if now >= prev {
        (now - prev) as f32 / dt
    } else {
        0.0
    }
}

fn read_nics() -> Vec<RawNic> {
    #[cfg(windows)]
    {
        return read_nics_sysinfo();
    }
    #[cfg(not(windows))]
    {
        read_nics_proc()
    }
}

#[cfg(windows)]
fn read_nics_sysinfo() -> Vec<RawNic> {
    use sysinfo::Networks;
    let networks = Networks::new_with_refreshed_list();
    let mut out = Vec::new();
    for (name, data) in &networks {
        let lname = name.to_ascii_lowercase();
        if lname.contains("loopback") || lname == "lo" {
            continue;
        }
        out.push(RawNic {
            name: name.to_string(),
            rx: data.total_received(),
            tx: data.total_transmitted(),
            rx_pkts: data.total_packets_received(),
            tx_pkts: data.total_packets_transmitted(),
            drops: data.total_errors_on_received().saturating_add(data.total_errors_on_transmitted()),
            errors: data.total_errors_on_received().saturating_add(data.total_errors_on_transmitted()),
        });
    }
    out.sort_by(|a, b| b.rx.cmp(&a.rx));
    out.truncate(12);
    out
}

#[cfg(not(windows))]
fn read_nics_proc() -> Vec<RawNic> {
    let path = Path::new("/proc/net/dev");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines().skip(2) {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let name = iface.trim();
        if name.is_empty() || name == "lo" {
            continue;
        }
        let cols: Vec<&str> = rest.split_whitespace().collect();
        if cols.len() < 16 {
            continue;
        }
        let rx = cols[0].parse().unwrap_or(0);
        let rx_pkts = cols[1].parse().unwrap_or(0);
        let errors = cols[2].parse::<u64>().unwrap_or(0) + cols[10].parse::<u64>().unwrap_or(0);
        let drops = cols[3].parse::<u64>().unwrap_or(0) + cols[11].parse::<u64>().unwrap_or(0);
        let tx = cols[8].parse().unwrap_or(0);
        let tx_pkts = cols[9].parse().unwrap_or(0);
        out.push(RawNic {
            name: name.to_string(),
            rx,
            tx,
            rx_pkts,
            tx_pkts,
            drops,
            errors,
        });
    }
    out.sort_by(|a, b| b.rx.cmp(&a.rx));
    out.truncate(12);
    out
}

fn tcp_states() -> (u32, u32, u32) {
    #[cfg(windows)]
    {
        return tcp_states_netstat();
    }
    #[cfg(not(windows))]
    {
        tcp_states_proc()
    }
}

#[cfg(windows)]
fn tcp_states_netstat() -> (u32, u32, u32) {
    let mut cmd = std::process::Command::new("netstat");
    cmd.args(["-ano", "-p", "tcp"]);
    crate::wincompat::hide_console_std(&mut cmd);
    let Ok(out) = cmd.output() else {
        return (0, 0, 0);
    };
    let mut estab = 0u32;
    let mut syn = 0u32;
    let mut tw = 0u32;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let u = line.to_ascii_uppercase();
        if u.contains("ESTAB") {
            estab += 1;
        } else if u.contains("SYN") {
            syn += 1;
        } else if u.contains("TIME_WAIT") || u.contains("TIME-WAIT") {
            tw += 1;
        }
    }
    (estab, syn, tw)
}

#[cfg(not(windows))]
fn tcp_states_proc() -> (u32, u32, u32) {
    let mut estab = 0u32;
    let mut syn = 0u32;
    let mut tw = 0u32;
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 4 {
                continue;
            }
            let Ok(st) = u8::from_str_radix(cols[3], 16) else {
                continue;
            };
            match st {
                0x01 => estab += 1,
                0x03 => syn += 1,
                0x06 => tw += 1,
                _ => {}
            }
        }
    }
    (estab, syn, tw)
}
