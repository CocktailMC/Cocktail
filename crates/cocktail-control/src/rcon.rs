use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RconConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RconResponse {
    pub ok: bool,
    pub response: String,
}

pub struct RconClient {
    stream: TcpStream,
    seq: i32,
}

impl RconClient {
    pub fn connect(host: &str, port: u16, password: &str) -> anyhow::Result<Self> {
        let mut s = TcpStream::connect_timeout(
            &std::net::SocketAddr::from((
                host.parse()
                    .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                port,
            )),
            Duration::from_secs(3),
        )?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        s.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut c = Self { stream: s, seq: 0 };
        c.send(3, password)?;
        let (id, _ty, _body) = c.recv()?;
        if id == -1 {
            anyhow::bail!("rcon auth failed");
        }
        Ok(c)
    }

    pub fn exec(&mut self, cmd: &str) -> anyhow::Result<String> {
        self.seq += 1;
        self.send(2, cmd)?;
        let (_id, _ty, body) = self.recv()?;
        Ok(body)
    }

    fn send(&mut self, ty: i32, body: &str) -> anyhow::Result<()> {
        let body_bytes = body.as_bytes();
        let len = (body_bytes.len() + 10) as i32;
        let mut buf = Vec::with_capacity(len as usize + 4);
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&self.seq.to_le_bytes());
        buf.extend_from_slice(&ty.to_le_bytes());
        buf.extend_from_slice(body_bytes);
        buf.push(0);
        buf.push(0);
        self.stream.write_all(&buf)?;
        Ok(())
    }

    fn recv(&mut self) -> anyhow::Result<(i32, i32, String)> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let len = i32::from_le_bytes(len_buf) as usize;
        if !(10..=4196).contains(&len) {
            anyhow::bail!("rcon invalid packet length: {len}");
        }
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf)?;
        let id = i32::from_le_bytes(buf[0..4].try_into().unwrap());
        let ty = i32::from_le_bytes(buf[4..8].try_into().unwrap());
        let body_end = len.saturating_sub(2);
        let body = String::from_utf8_lossy(&buf[8..body_end]).to_string();
        Ok((id, ty, body))
    }
}

pub fn extract_rcon_config(workdir: &str) -> Option<RconConfig> {
    let path = std::path::Path::new(workdir).join("server.properties");
    let raw = std::fs::read_to_string(path).ok()?;
    let mut enabled = false;
    let mut port = 25575u16;
    let mut password = String::new();
    let mut host = "127.0.0.1".to_string();
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim();
            match k {
                "enable-rcon" => enabled = v == "true",
                "rcon.port" => port = v.parse().unwrap_or(25575),
                "rcon.password" => password = v.to_string(),
                "server-ip" => {
                    if !v.is_empty() {
                        host = v.to_string();
                    }
                }
                _ => {}
            }
        }
    }
    if enabled && !password.is_empty() {
        Some(RconConfig {
            host,
            port,
            password,
        })
    } else {
        None
    }
}

pub fn try_rcon(workdir: &str, command: &str) -> Option<String> {
    let cfg = extract_rcon_config(workdir)?;
    let mut client = RconClient::connect(&cfg.host, cfg.port, &cfg.password).ok()?;
    client.exec(command).ok()
}
