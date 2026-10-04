use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Cidr {
    net: u32,
    mask: u32,
    v6: Option<([u8; 16], u8)>,
}

impl Cidr {
    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim();
        if text.is_empty() {
            return None;
        }
        let (addr_part, bits_part) = match text.split_once('/') {
            Some((a, b)) => (a, Some(b)),
            None => (text, None),
        };
        if addr_part.contains(':') {
            let addr: std::net::Ipv6Addr = addr_part.parse().ok()?;
            let bits: u8 = match bits_part {
                Some(b) => b.parse().ok()?,
                None => 128,
            };
            if bits > 128 {
                return None;
            }
            return Some(Cidr {
                net: 0,
                mask: 0,
                v6: Some((addr.octets(), bits)),
            });
        }
        let addr: Ipv4Addr = addr_part.parse().ok()?;
        let bits: u32 = match bits_part {
            Some(b) => b.parse().ok()?,
            None => 32,
        };
        if bits > 32 {
            return None;
        }
        let net = u32::from(addr);
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits)
        };
        Some(Cidr {
            net: net & mask,
            mask,
            v6: None,
        })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.v6, ip) {
            (Some((net, bits)), IpAddr::V6(v6)) => {
                let octets = v6.octets();
                let full = bits as usize / 8;
                let rem = bits as usize % 8;
                if octets[..full] != net[..full] {
                    return false;
                }
                if rem == 0 {
                    return true;
                }
                let mask = 0xffu8 << (8 - rem);
                (octets[full] & mask) == (net[full] & mask)
            }
            (None, IpAddr::V4(v4)) => {
                let value = u32::from(v4);
                (value & self.mask) == self.net
            }
            (Some((net, bits)), IpAddr::V4(v4)) => {
                let mapped = v4.to_ipv6_mapped().octets();
                let full = bits as usize / 8;
                let rem = bits as usize % 8;
                if mapped[..full] != net[..full] {
                    return false;
                }
                if rem == 0 {
                    return true;
                }
                let mask = 0xffu8 << (8 - rem);
                (mapped[full] & mask) == (net[full] & mask)
            }
            (None, IpAddr::V6(_)) => false,
        }
    }
}

pub fn parse_allowlist(raw: &str) -> Vec<Cidr> {
    raw.split([',', '\n', ' ', ';'])
        .filter_map(|part| {
            let t = part.trim();
            if t.is_empty() { None } else { Cidr::parse(t) }
        })
        .collect()
}

pub fn client_ip(xff: Option<&str>, real_ip: Option<&str>, peer: Option<&str>) -> String {
    if let Some(ip) = real_ip {
        let t = ip.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Some(list) = xff {
        if let Some(first) = list.split(',').next() {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    peer.unwrap_or("local").to_string()
}

pub fn ip_allowed(allow: &[Cidr], ip: &str) -> bool {
    if allow.is_empty() {
        return true;
    }
    let trimmed = ip.trim();
    if trimmed.eq_ignore_ascii_case("local") {
        return true;
    }
    let cleaned = trimmed.trim_start_matches("::ffff:").to_string();
    let parsed: Option<IpAddr> = cleaned.parse().ok().or_else(|| trimmed.parse().ok());
    match parsed {
        Some(addr) => allow.iter().any(|c| c.contains(addr)),
        None => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RateDecision {
    pub allowed: bool,
    pub remaining: u32,
    pub retry_after_secs: u64,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

#[derive(Clone, Debug)]
pub struct RateRule {
    pub capacity: f64,
    pub refill_per_sec: f64,
}

impl RateRule {
    pub const fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
        }
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
    rule: RateRule,
}

impl RateLimiter {
    pub fn new(rule: RateRule) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            rule,
        }
    }

    pub fn check(&self, key: &str) -> RateDecision {
        let now = Instant::now();
        let mut guard = match self.buckets.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if guard.len() > 20000 {
            guard.retain(|_, b| now.duration_since(b.last) < Duration::from_secs(3600));
        }
        let rule = &self.rule;
        let bucket = guard.entry(key.to_string()).or_insert(Bucket {
            tokens: rule.capacity,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.last = now;
        bucket.tokens = (bucket.tokens + elapsed * rule.refill_per_sec).min(rule.capacity);
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            RateDecision {
                allowed: true,
                remaining: bucket.tokens.floor() as u32,
                retry_after_secs: 0,
            }
        } else {
            let need = 1.0 - bucket.tokens;
            let wait = (need / rule.refill_per_sec).ceil() as u64;
            RateDecision {
                allowed: false,
                remaining: 0,
                retry_after_secs: wait.max(1),
            }
        }
    }

    pub fn reset(&self, key: &str) {
        if let Ok(mut g) = self.buckets.lock() {
            g.remove(key);
        }
    }
}

pub struct ConcurrencyGate {
    limit: usize,
    active: Mutex<usize>,
}

impl ConcurrencyGate {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            active: Mutex::new(0),
        }
    }

    pub fn try_acquire(&self) -> Option<ConcurrencyGuard<'_>> {
        let mut guard = match self.active.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if *guard >= self.limit {
            return None;
        }
        *guard += 1;
        drop(guard);
        Some(ConcurrencyGuard { gate: self })
    }

    pub fn active(&self) -> usize {
        self.active.lock().map(|g| *g).unwrap_or(0)
    }
}

pub struct ConcurrencyGuard<'a> {
    gate: &'a ConcurrencyGate,
}

impl Drop for ConcurrencyGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut g) = self.gate.active.lock() {
            *g = g.saturating_sub(1);
        }
    }
}

pub struct QuotaTracker {
    counters: Mutex<HashMap<String, (u64, Instant)>>,
}

impl QuotaTracker {
    pub fn new() -> Self {
        Self {
            counters: Mutex::new(HashMap::new()),
        }
    }

    pub fn add(&self, key: &str, amount: u64) -> u64 {
        let mut guard = match self.counters.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let entry = guard.entry(key.to_string()).or_insert((0, Instant::now()));
        entry.0 = entry.0.saturating_add(amount);
        entry.0
    }

    pub fn get(&self, key: &str) -> u64 {
        self.counters
            .lock()
            .map(|g| g.get(key).map(|v| v.0).unwrap_or(0))
            .unwrap_or(0)
    }

    pub fn snapshot(&self) -> Vec<(String, u64)> {
        let guard = match self.counters.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut out: Vec<(String, u64)> = guard.iter().map(|(k, v)| (k.clone(), v.0)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    pub fn prune_older_than(&self, secs: u64) {
        let now = Instant::now();
        if let Ok(mut g) = self.counters.lock() {
            g.retain(|_, (_, at)| now.duration_since(*at) < Duration::from_secs(secs));
        }
    }
}

impl Default for QuotaTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn security_headers() -> Vec<(&'static str, String)> {
    vec![
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self' data:; connect-src 'self' ws: wss:; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'".to_string(),
        ),
        ("x-content-type-options", "nosniff".to_string()),
        ("x-frame-options", "DENY".to_string()),
        ("referrer-policy", "no-referrer".to_string()),
        (
            "permissions-policy",
            "geolocation=(), microphone=(), camera=(), payment=()".to_string(),
        ),
        ("cross-origin-opener-policy", "same-origin".to_string()),
        ("cross-origin-resource-policy", "same-origin".to_string()),
        ("x-permitted-cross-domain-policies", "none".to_string()),
    ]
}

pub fn hsts_header(https: bool) -> Option<(&'static str, String)> {
    if https {
        Some((
            "strict-transport-security",
            "max-age=31536000; includeSubDomains".to_string(),
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_ipv4_matching() {
        let c = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(c.contains("10.1.2.3".parse().unwrap()));
        assert!(!c.contains("11.1.2.3".parse().unwrap()));
        let host = Cidr::parse("192.168.1.5").unwrap();
        assert!(host.contains("192.168.1.5".parse().unwrap()));
        assert!(!host.contains("192.168.1.6".parse().unwrap()));
        let zero = Cidr::parse("0.0.0.0/0").unwrap();
        assert!(zero.contains("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn cidr_ipv6_matching() {
        let c = Cidr::parse("2001:db8::/32").unwrap();
        assert!(c.contains("2001:db8:1::1".parse().unwrap()));
        assert!(!c.contains("2001:db9::1".parse().unwrap()));
        let mapped = Cidr::parse("::ffff:127.0.0.1/128").unwrap();
        assert!(mapped.contains("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn allowlist_semantics() {
        let allow = parse_allowlist("10.0.0.0/8, 192.168.0.0/16");
        assert!(ip_allowed(&allow, "10.5.5.5"));
        assert!(ip_allowed(&allow, "192.168.9.9"));
        assert!(!ip_allowed(&allow, "8.8.8.8"));
        assert!(ip_allowed(&allow, "local"));
        assert!(ip_allowed(&[], "8.8.8.8"));
        assert!(ip_allowed(&allow, "::ffff:10.1.1.1"));
        assert!(!ip_allowed(&allow, "not-an-ip"));
    }

    #[test]
    fn client_ip_precedence() {
        assert_eq!(
            client_ip(Some("1.1.1.1, 2.2.2.2"), None, Some("3.3.3.3")),
            "1.1.1.1"
        );
        assert_eq!(client_ip(Some("1.1.1.1"), Some("9.9.9.9"), None), "9.9.9.9");
        assert_eq!(client_ip(None, None, None), "local");
        assert_eq!(client_ip(None, None, Some("4.4.4.4")), "4.4.4.4");
    }

    #[test]
    fn rate_limiter_token_bucket() {
        let limiter = RateLimiter::new(RateRule::new(3.0, 1.0));
        assert!(limiter.check("k").allowed);
        assert!(limiter.check("k").allowed);
        assert!(limiter.check("k").allowed);
        let denied = limiter.check("k");
        assert!(!denied.allowed);
        assert!(denied.retry_after_secs >= 1);
        assert!(limiter.check("other").allowed);
    }

    #[test]
    fn concurrency_gate_limits() {
        let gate = ConcurrencyGate::new(2);
        let a = gate.try_acquire().unwrap();
        let b = gate.try_acquire().unwrap();
        assert!(gate.try_acquire().is_none());
        assert_eq!(gate.active(), 2);
        drop(a);
        assert_eq!(gate.active(), 1);
        drop(b);
        assert_eq!(gate.active(), 0);
        assert!(gate.try_acquire().is_some());
    }

    #[test]
    fn quota_accumulates() {
        let q = QuotaTracker::new();
        assert_eq!(q.add("u1", 5), 5);
        assert_eq!(q.add("u1", 7), 12);
        assert_eq!(q.get("u1"), 12);
        assert_eq!(q.get("u2"), 0);
        let snap = q.snapshot();
        assert_eq!(snap[0].0, "u1");
    }

    #[test]
    fn headers_present() {
        let h = security_headers();
        let names: Vec<&str> = h.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"content-security-policy"));
        assert!(names.contains(&"x-frame-options"));
        assert!(names.contains(&"x-content-type-options"));
        assert!(hsts_header(true).is_some());
        assert!(hsts_header(false).is_none());
    }
}
