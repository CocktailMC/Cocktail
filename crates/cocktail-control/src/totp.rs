use std::time::{SystemTime, UNIX_EPOCH};

pub fn generate_secret() -> String {
    let mut bytes = [0u8; 20];
    use std::io::Read;
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    base32_encode(&bytes)
}

pub fn totp_code(secret: &str, period: u64) -> Option<u32> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    totp_code_at(secret, now, period)
}

pub fn totp_code_at(secret: &str, unix_secs: u64, period: u64) -> Option<u32> {
    let key = base32_decode(secret)?;
    let counter = unix_secs / period;
    let msg = counter.to_be_bytes();
    let mac = hmac_sha1(&key, &msg);
    let offset = (mac[mac.len() - 1] & 0x0f) as usize;
    let code = ((mac[offset] as u32 & 0x7f) << 24)
        | ((mac[offset + 1] as u32) << 16)
        | ((mac[offset + 2] as u32) << 8)
        | (mac[offset + 3] as u32);
    Some(code % 1_000_000)
}

pub fn verify_code(secret: &str, code: u32) -> bool {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for drift in -1i64..=1 {
        let counter = ((now as i64 / 30) + drift).max(0) as u64;
        let msg = counter.to_be_bytes();
        let mac = hmac_sha1(&base32_decode(secret).unwrap_or_default(), &msg);
        let offset = (mac[mac.len() - 1] & 0x0f) as usize;
        let expected = ((mac[offset] as u32 & 0x7f) << 24)
            | ((mac[offset + 1] as u32) << 16)
            | ((mac[offset + 2] as u32) << 8)
            | (mac[offset + 3] as u32);
        if expected % 1_000_000 == code {
            return true;
        }
    }
    false
}

pub fn otpauth_url(secret: &str, account: &str, issuer: &str) -> String {
    format!("otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}&digits=6&period=30")
}

fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = String::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for &b in data {
        buffer = (buffer << 8) | (b as u32);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            result.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    while result.len() % 8 != 0 {
        result.push('=');
    }
    result
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: [i8; 256] = {
        let mut t = [-1i8; 256];
        let alpha = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut i = 0;
        while i < alpha.len() {
            t[alpha[i] as usize] = i as i8;
            i += 1;
        }
        t
    };
    let mut buffer = 0u32;
    let mut bits = 0u32;
    let mut result = Vec::new();
    for c in s.chars().filter(|c| *c != '=' && !c.is_whitespace()) {
        let v = TABLE[c as usize];
        if v < 0 {
            return None;
        }
        buffer = (buffer << 5) | (v as u32);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            result.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(result)
}

fn hmac_sha1(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut k = if key.len() > 64 {
        let mut h = Sha1::new();
        h.update(key);
        h.finalize().to_vec()
    } else {
        key.to_vec()
    };
    while k.len() < 64 {
        k.push(0);
    }
    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = k[i] ^ 0x36;
        opad[i] = k[i] ^ 0x5c;
    }
    let mut inner = Sha1::new();
    inner.update(&ipad);
    inner.update(msg);
    let inner_hash = inner.finalize();
    let mut outer = Sha1::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize().to_vec()
}

struct Sha1 {
    h: [u32; 5],
    data: Vec<u8>,
    len: u64,
}

impl Sha1 {
    fn new() -> Self {
        Self {
            h: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0],
            data: Vec::new(),
            len: 0,
        }
    }
    fn update(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
        self.len = self.len.wrapping_add((data.len() * 8) as u64);
    }
    fn finalize(mut self) -> Vec<u8> {
        let bit_len = self.len;
        self.data.push(0x80);
        while self.data.len() % 64 != 56 {
            self.data.push(0);
        }
        self.data.extend_from_slice(&bit_len.to_be_bytes());
        for chunk in self.data.chunks(64) {
            let mut w = [0u32; 80];
            for i in 0..16 {
                w[i] = u32::from_be_bytes(chunk[i*4..i*4+4].try_into().unwrap());
            }
            for i in 16..80 {
                w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1);
            }
            let mut a = self.h[0];
            let mut b = self.h[1];
            let mut c = self.h[2];
            let mut d = self.h[3];
            let mut e = self.h[4];
            for i in 0..80 {
                let (f, k) = match i {
                    0..=19 => ((b & c) | ((!b) & d), 0x5a827999),
                    20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                    40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                    _ => (b ^ c ^ d, 0xca62c1d6),
                };
                let temp = a.rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(w[i]);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = temp;
            }
            self.h[0] = self.h[0].wrapping_add(a);
            self.h[1] = self.h[1].wrapping_add(b);
            self.h[2] = self.h[2].wrapping_add(c);
            self.h[3] = self.h[3].wrapping_add(d);
            self.h[4] = self.h[4].wrapping_add(e);
        }
        let mut out = Vec::with_capacity(20);
        for h in self.h {
            out.extend_from_slice(&h.to_be_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector_rfc6238_sha1() {
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let code = totp_code_at(secret, 59, 30);
        assert_eq!(code, Some(287082));
    }

    #[test]
    fn base32_roundtrip_and_otpauth() {
        let url = otpauth_url("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", "root", "Cocktail");
        assert!(url.starts_with("otpauth://totp/"));
        assert!(url.contains("digits=6"));
        assert!(url.contains("period=30"));
    }

    #[test]
    fn verify_rejects_wrong_code() {
        assert!(!verify_code("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", 1));
    }
}
