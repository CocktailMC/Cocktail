#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const TCP_TABLE_OWNER_PID_ALL: i32 = 5;
const UDP_TABLE_OWNER_PID: i32 = 1;
const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const MIB_TCP_STATE_LISTEN: u32 = 2;
const MIB_TCP_STATE_SYN_SENT: u32 = 3;
const MIB_TCP_STATE_SYN_RCVD: u32 = 4;
const MIB_TCP_STATE_ESTAB: u32 = 5;
const MIB_TCP_STATE_FIN_WAIT1: u32 = 6;
const MIB_TCP_STATE_FIN_WAIT2: u32 = 7;
const MIB_TCP_STATE_TIME_WAIT: u32 = 11;
const MIB_TCP_STATE_DELETE_TCB: u32 = 12;
const TCP_ESTATS_DATA: i32 = 1;

#[derive(Debug, Clone)]
pub struct WinSock {
    pub local_ip: IpAddr,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub listen: bool,
    pub established: bool,
    pub syn_recv: bool,
    pub time_wait: bool,
    pub fin_wait: bool,
    pub udp: bool,
}

#[cfg(windows)]
mod ffi {
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MibTcpRow {
        pub dw_state: u32,
        pub dw_local_addr: u32,
        pub dw_local_port: u32,
        pub dw_remote_addr: u32,
        pub dw_remote_port: u32,
    }

    #[repr(C)]
    pub struct TcpEstatsDataRw {
        pub enable_collection: u8,
    }

    #[repr(C)]
    pub struct TcpEstatsDataRod {
        pub data_bytes_out: u64,
        pub data_segs_out: u64,
        pub data_bytes_in: u64,
        pub data_segs_in: u64,
        pub segs_out: u64,
        pub segs_in: u64,
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        pub fn GetExtendedTcpTable(
            table: *mut u8,
            size: *mut u32,
            order: i32,
            af: u32,
            table_class: i32,
            reserved: u32,
        ) -> u32;
        pub fn GetExtendedUdpTable(
            table: *mut u8,
            size: *mut u32,
            order: i32,
            af: u32,
            table_class: i32,
            reserved: u32,
        ) -> u32;
        pub fn SetTcpEntry(row: *mut MibTcpRow) -> u32;
        pub fn SetPerTcpConnectionEStats(
            row: *mut MibTcpRow,
            estats_type: i32,
            rw: *mut u8,
            rw_version: u32,
            rw_size: u32,
            offset: u32,
        ) -> u32;
        pub fn GetPerTcpConnectionEStats(
            row: *mut MibTcpRow,
            estats_type: i32,
            rw: *mut u8,
            rw_version: u32,
            rw_size: u32,
            ros: *mut u8,
            ros_version: u32,
            ros_size: u32,
            rod: *mut u8,
            rod_version: u32,
            rod_size: u32,
        ) -> u32;
    }
}

fn mib_port(dw: u32) -> u16 {
    u16::from_be(dw as u16)
}

fn ipv4_from_mib(dw: u32) -> Ipv4Addr {
    Ipv4Addr::from(dw.to_le_bytes())
}

#[cfg(windows)]
fn read_table(
    af: u32,
    class: i32,
    getter: unsafe extern "system" fn(*mut u8, *mut u32, i32, u32, i32, u32) -> u32,
) -> Option<Vec<u8>> {
    unsafe {
        let mut size = 0u32;
        let st = getter(std::ptr::null_mut(), &mut size, 1, af, class, 0);
        if st != ERROR_INSUFFICIENT_BUFFER && st != NO_ERROR {
            return None;
        }
        if size < 4 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let st = getter(buf.as_mut_ptr(), &mut size, 1, af, class, 0);
        if st != NO_ERROR {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }
}

#[cfg(windows)]
fn tcp_v4(port: Option<u16>) -> Vec<(WinSock, ffi::MibTcpRow)> {
    let Some(buf) = read_table(AF_INET, TCP_TABLE_OWNER_PID_ALL, ffi::GetExtendedTcpTable) else {
        return Vec::new();
    };
    if buf.len() < 4 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
    let row_size = 24;
    let mut out = Vec::new();
    for i in 0..n {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        let state = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let local_addr = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
        let local_port = u32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap());
        let remote_addr = u32::from_le_bytes(buf[off + 12..off + 16].try_into().unwrap());
        let remote_port = u32::from_le_bytes(buf[off + 16..off + 20].try_into().unwrap());
        let lp = mib_port(local_port);
        if let Some(want) = port {
            if lp != want {
                continue;
            }
        }
        let sock = WinSock {
            local_ip: IpAddr::V4(ipv4_from_mib(local_addr)),
            remote_ip: IpAddr::V4(ipv4_from_mib(remote_addr)),
            remote_port: mib_port(remote_port),
            listen: state == MIB_TCP_STATE_LISTEN,
            established: state == MIB_TCP_STATE_ESTAB,
            syn_recv: state == MIB_TCP_STATE_SYN_RCVD || state == MIB_TCP_STATE_SYN_SENT,
            time_wait: state == MIB_TCP_STATE_TIME_WAIT,
            fin_wait: state == MIB_TCP_STATE_FIN_WAIT1 || state == MIB_TCP_STATE_FIN_WAIT2,
            udp: false,
        };
        let row = ffi::MibTcpRow {
            dw_state: state,
            dw_local_addr: local_addr,
            dw_local_port: local_port,
            dw_remote_addr: remote_addr,
            dw_remote_port: remote_port,
        };
        out.push((sock, row));
    }
    out
}

#[cfg(windows)]
fn tcp_v6(port: Option<u16>) -> Vec<WinSock> {
    let Some(buf) = read_table(AF_INET6, TCP_TABLE_OWNER_PID_ALL, ffi::GetExtendedTcpTable) else {
        return Vec::new();
    };
    if buf.len() < 4 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;

    let row_size = 16 + 4 + 4 + 16 + 4 + 4 + 4 + 4;
    let mut out = Vec::new();
    for i in 0..n {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        let mut local = [0u8; 16];
        local.copy_from_slice(&buf[off..off + 16]);
        let local_port = u32::from_le_bytes(buf[off + 20..off + 24].try_into().unwrap());
        let lp = mib_port(local_port);
        if let Some(want) = port {
            if lp != want {
                continue;
            }
        }
        let mut remote = [0u8; 16];
        remote.copy_from_slice(&buf[off + 24..off + 40]);
        let remote_port = u32::from_le_bytes(buf[off + 44..off + 48].try_into().unwrap());
        let state = u32::from_le_bytes(buf[off + 48..off + 52].try_into().unwrap());
        out.push(WinSock {
            local_ip: IpAddr::V6(Ipv6Addr::from(local)),
            remote_ip: IpAddr::V6(Ipv6Addr::from(remote)),
            remote_port: mib_port(remote_port),
            listen: state == MIB_TCP_STATE_LISTEN,
            established: state == MIB_TCP_STATE_ESTAB,
            syn_recv: state == MIB_TCP_STATE_SYN_RCVD || state == MIB_TCP_STATE_SYN_SENT,
            time_wait: state == MIB_TCP_STATE_TIME_WAIT,
            fin_wait: state == MIB_TCP_STATE_FIN_WAIT1 || state == MIB_TCP_STATE_FIN_WAIT2,
            udp: false,
        });
    }
    out
}

#[cfg(windows)]
fn udp_v4(port: u16) -> Vec<WinSock> {
    let Some(buf) = read_table(AF_INET, UDP_TABLE_OWNER_PID, ffi::GetExtendedUdpTable) else {
        return Vec::new();
    };
    if buf.len() < 4 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
    let row_size = 12;
    let mut out = Vec::new();
    for i in 0..n {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        let local_addr = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let local_port = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
        if mib_port(local_port) != port {
            continue;
        }
        out.push(WinSock {
            local_ip: IpAddr::V4(ipv4_from_mib(local_addr)),
            remote_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            remote_port: 0,
            listen: true,
            established: false,
            syn_recv: false,
            time_wait: false,
            fin_wait: false,
            udp: true,
        });
    }
    out
}

#[cfg(windows)]
pub fn sockets_on_port(port: u16) -> Vec<WinSock> {
    let mut out: Vec<WinSock> = tcp_v4(Some(port)).into_iter().map(|(s, _)| s).collect();
    out.extend(tcp_v6(Some(port)));
    out.extend(udp_v4(port));
    out
}

#[cfg(not(windows))]
pub fn sockets_on_port(_port: u16) -> Vec<WinSock> {
    Vec::new()
}

#[cfg(windows)]
pub fn tcp_state_counts() -> (u32, u32, u32) {
    let mut estab = 0u32;
    let mut syn = 0u32;
    let mut tw = 0u32;
    for (s, _) in tcp_v4(None) {
        if s.established {
            estab += 1;
        } else if s.syn_recv {
            syn += 1;
        } else if s.time_wait {
            tw += 1;
        }
    }
    for s in tcp_v6(None) {
        if s.established {
            estab += 1;
        } else if s.syn_recv {
            syn += 1;
        } else if s.time_wait {
            tw += 1;
        }
    }
    (estab, syn, tw)
}

#[cfg(not(windows))]
pub fn tcp_state_counts() -> (u32, u32, u32) {
    (0, 0, 0)
}

fn ip_in_cidr(ip: IpAddr, cidr: &str) -> bool {
    let (addr_s, prefix_s) = cidr.split_once('/').unwrap_or((cidr, ""));
    let Ok(net) = addr_s.parse::<IpAddr>() else {
        return ip.to_string() == addr_s;
    };
    let prefix: u8 = prefix_s
        .parse()
        .unwrap_or(if net.is_ipv4() { 32 } else { 128 });
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => {
            let shift = 32u32.saturating_sub(u32::from(prefix.min(32)));
            let mask = if shift >= 32 { 0 } else { u32::MAX << shift };
            (u32::from(a) & mask) == (u32::from(n) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            let p = u32::from(prefix.min(128));
            let aa = u128::from(a);
            let nn = u128::from(n);
            let shift = 128u32.saturating_sub(p);
            let mask = if shift >= 128 { 0 } else { u128::MAX << shift };
            (aa & mask) == (nn & mask)
        }
        _ => false,
    }
}

#[cfg(windows)]
pub fn kick_conns(cidr: &str, ports: &[u16]) -> u32 {
    let mut killed = 0u32;
    for (sock, mut row) in tcp_v4(None) {
        if !sock.established {
            continue;
        }
        if !ports.is_empty() {
            let local_port = mib_port(row.dw_local_port);
            if !ports.contains(&local_port) {
                continue;
            }
        }
        if !ip_in_cidr(sock.remote_ip, cidr) && !ip_in_cidr(sock.local_ip, cidr) {
            continue;
        }
        row.dw_state = MIB_TCP_STATE_DELETE_TCB;
        let st = unsafe { ffi::SetTcpEntry(&mut row) };
        if st == NO_ERROR {
            killed += 1;
        }
    }
    killed
}

#[cfg(not(windows))]
pub fn kick_conns(_cidr: &str, _ports: &[u16]) -> u32 {
    0
}

#[cfg(windows)]
pub fn tcp_bytes_on_port(port: u16) -> Option<(u64, u64)> {
    let mut rx = 0u64;
    let mut tx = 0u64;
    let mut any = false;
    for (sock, mut row) in tcp_v4(Some(port)) {
        if !sock.established {
            continue;
        }
        let mut rw = ffi::TcpEstatsDataRw {
            enable_collection: 1,
        };
        unsafe {
            let _ = ffi::SetPerTcpConnectionEStats(
                &mut row,
                TCP_ESTATS_DATA,
                (&mut rw as *mut ffi::TcpEstatsDataRw).cast(),
                0,
                std::mem::size_of::<ffi::TcpEstatsDataRw>() as u32,
                0,
            );
            let mut rod = ffi::TcpEstatsDataRod {
                data_bytes_out: 0,
                data_segs_out: 0,
                data_bytes_in: 0,
                data_segs_in: 0,
                segs_out: 0,
                segs_in: 0,
            };
            let st = ffi::GetPerTcpConnectionEStats(
                &mut row,
                TCP_ESTATS_DATA,
                std::ptr::null_mut(),
                0,
                0,
                std::ptr::null_mut(),
                0,
                0,
                (&mut rod as *mut ffi::TcpEstatsDataRod).cast(),
                0,
                std::mem::size_of::<ffi::TcpEstatsDataRod>() as u32,
            );
            if st == NO_ERROR {
                rx += rod.data_bytes_in;
                tx += rod.data_bytes_out;
                any = true;
            }
        }
    }
    any.then_some((rx, tx))
}

#[cfg(not(windows))]
pub fn tcp_bytes_on_port(_port: u16) -> Option<(u64, u64)> {
    None
}
