use std::fmt::Write as _;

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = if key.len() > 64 {
        sha256(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(64, 0);
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = ipad.to_vec();
    inner.extend_from_slice(msg);
    let ih = sha256(&inner);
    let mut outer = opad.to_vec();
    outer.extend_from_slice(&ih);
    sha256(&outer)
}

pub fn hmac_sha256_multi(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut k = if key.len() > 64 {
        sha256(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(64, 0);
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = ipad.to_vec();
    for p in parts {
        inner.extend_from_slice(p);
    }
    let ih = sha256(&inner);
    let mut outer = opad.to_vec();
    outer.extend_from_slice(&ih);
    sha256(&outer)
}

pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let prk = hmac_sha256(salt, ikm);
    let mut out = Vec::with_capacity(len);
    let mut prev: Vec<u8> = Vec::new();
    let mut counter = 1u8;
    while out.len() < len {
        let mut parts: Vec<&[u8]> = Vec::with_capacity(3);
        parts.push(&prev);
        parts.push(info);
        let c = [counter];
        parts.push(&c);
        let block = hmac_sha256_multi(&prk, &parts);
        out.extend_from_slice(&block);
        prev = block.to_vec();
        counter = counter.wrapping_add(1);
    }
    out.truncate(len);
    out
}

pub fn hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    let bytes = clean.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(out)
}

pub fn random_bytes(len: usize) -> Vec<u8> {
    // 直接使用操作系统级 CSPRNG：rand_core::OsRng 在 Linux/macOS 走 getrandom，
    // 在 Windows 走 BCryptGenRandom/ProcessPrng，提供密码学安全熵源。
    // 失败时 fill_bytes 会 panic——这是正确的语义，避免静默降级到弱随机。
    use rand_core::{OsRng, RngCore};
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

pub fn random_token(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let raw = random_bytes(len);
    raw.iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;
    for i in 0..8 {
        state[4 + i] =
            u32::from_le_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4],
            nonce[i * 4 + 1],
            nonce[i * 4 + 2],
            nonce[i * 4 + 3],
        ]);
    }
    let init = state;
    macro_rules! qr {
        ($a:expr, $b:expr, $c:expr, $d:expr) => {
            state[$a] = state[$a].wrapping_add(state[$b]);
            state[$d] ^= state[$a];
            state[$d] = state[$d].rotate_left(16);
            state[$c] = state[$c].wrapping_add(state[$d]);
            state[$b] ^= state[$c];
            state[$b] = state[$b].rotate_left(12);
            state[$a] = state[$a].wrapping_add(state[$b]);
            state[$d] ^= state[$a];
            state[$d] = state[$d].rotate_left(8);
            state[$c] = state[$c].wrapping_add(state[$d]);
            state[$b] ^= state[$c];
            state[$b] = state[$b].rotate_left(7);
        };
    }
    for _ in 0..10 {
        qr!(0, 4, 8, 12);
        qr!(1, 5, 9, 13);
        qr!(2, 6, 10, 14);
        qr!(3, 7, 11, 15);
        qr!(0, 5, 10, 15);
        qr!(1, 6, 11, 12);
        qr!(2, 7, 8, 13);
        qr!(3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        let v = state[i].wrapping_add(init[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

fn chacha20_xor(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut ctr = counter;
    for chunk in data.chunks(64) {
        let ks = chacha20_block(key, ctr, nonce);
        for (i, b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
        ctr = ctr.wrapping_add(1);
    }
    out
}

fn poly1305(msg: &[u8], key: &[u8; 32]) -> [u8; 16] {
    let t0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
    let t1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]);
    let t2 = u32::from_le_bytes([key[8], key[9], key[10], key[11]]);
    let t3 = u32::from_le_bytes([key[12], key[13], key[14], key[15]]);
    let r0 = (t0 & 0x3ffffff) as u64;
    let r1 = (((t0 >> 26) | (t1 << 6)) & 0x3ffff03) as u64;
    let r2 = (((t1 >> 20) | (t2 << 12)) & 0x3ffc0ff) as u64;
    let r3 = (((t2 >> 14) | (t3 << 18)) & 0x3f03fff) as u64;
    let r4 = ((t3 >> 8) & 0x00fffff) as u64;
    let s1 = r1 * 5;
    let s2 = r2 * 5;
    let s3 = r3 * 5;
    let s4 = r4 * 5;
    let mut h0 = 0u64;
    let mut h1 = 0u64;
    let mut h2 = 0u64;
    let mut h3 = 0u64;
    let mut h4 = 0u64;
    let mut idx = 0;
    while idx < msg.len() {
        let remaining = msg.len() - idx;
        let take = remaining.min(16);
        let mut block = [0u8; 17];
        block[..take].copy_from_slice(&msg[idx..idx + take]);
        block[take] = 1;
        let b0 = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as u64;
        let b1 = u32::from_le_bytes([block[4], block[5], block[6], block[7]]) as u64;
        let b2 = u32::from_le_bytes([block[8], block[9], block[10], block[11]]) as u64;
        let b3 = u32::from_le_bytes([block[12], block[13], block[14], block[15]]) as u64;
        let b4 = u32::from_le_bytes([block[16], 0, 0, 0]) as u64;
        h0 += b0 & 0x3ffffff;
        h1 += ((b0 >> 26) | (b1 << 6)) & 0x3ffffff;
        h2 += ((b1 >> 20) | (b2 << 12)) & 0x3ffffff;
        h3 += ((b2 >> 14) | (b3 << 18)) & 0x3ffffff;
        h4 += (b3 >> 8) | (b4 << 24);
        let d0 = h0 * r0 + h1 * s4 + h2 * s3 + h3 * s2 + h4 * s1;
        let d1 = h0 * r1 + h1 * r0 + h2 * s4 + h3 * s3 + h4 * s2;
        let d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * s4 + h4 * s3;
        let d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * s4;
        let d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;
        let mut c = d0 >> 26;
        h0 = d0 & 0x3ffffff;
        let d1 = d1 + c;
        c = d1 >> 26;
        h1 = d1 & 0x3ffffff;
        let d2 = d2 + c;
        c = d2 >> 26;
        h2 = d2 & 0x3ffffff;
        let d3 = d3 + c;
        c = d3 >> 26;
        h3 = d3 & 0x3ffffff;
        let d4 = d4 + c;
        c = d4 >> 26;
        h4 = d4 & 0x3ffffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ffffff;
        h1 += c;
        idx += take;
    }
    let mut c = h1 >> 26;
    h1 &= 0x3ffffff;
    h2 += c;
    c = h2 >> 26;
    h2 &= 0x3ffffff;
    h3 += c;
    c = h3 >> 26;
    h3 &= 0x3ffffff;
    h4 += c;
    c = h4 >> 26;
    h4 &= 0x3ffffff;
    h0 += c * 5;
    c = h0 >> 26;
    h0 &= 0x3ffffff;
    h1 += c;
    let mut g0 = h0 + 5;
    c = g0 >> 26;
    g0 &= 0x3ffffff;
    let mut g1 = h1 + c;
    c = g1 >> 26;
    g1 &= 0x3ffffff;
    let mut g2 = h2 + c;
    c = g2 >> 26;
    g2 &= 0x3ffffff;
    let mut g3 = h3 + c;
    c = g3 >> 26;
    g3 &= 0x3ffffff;
    let g4 = (h4 + c).wrapping_sub(1 << 26);
    let mask = (g4 >> 63).wrapping_sub(1);
    let nmask = !mask;
    h0 = (h0 & nmask) | (g0 & mask);
    h1 = (h1 & nmask) | (g1 & mask);
    h2 = (h2 & nmask) | (g2 & mask);
    h3 = (h3 & nmask) | (g3 & mask);
    h4 = (h4 & nmask) | (g4 & mask);
    let mut f0 = ((h0) | (h1 << 26)) & 0xffffffff;
    let mut f1 = ((h1 >> 6) | (h2 << 20)) & 0xffffffff;
    let mut f2 = ((h2 >> 12) | (h3 << 14)) & 0xffffffff;
    let mut f3 = ((h3 >> 18) | (h4 << 8)) & 0xffffffff;
    let s0 = u32::from_le_bytes([key[16], key[17], key[18], key[19]]) as u64;
    let s1k = u32::from_le_bytes([key[20], key[21], key[22], key[23]]) as u64;
    let s2k = u32::from_le_bytes([key[24], key[25], key[26], key[27]]) as u64;
    let s3k = u32::from_le_bytes([key[28], key[29], key[30], key[31]]) as u64;
    f0 += s0;
    f1 += s1k + (f0 >> 32);
    f0 &= 0xffffffff;
    f2 += s2k + (f1 >> 32);
    f1 &= 0xffffffff;
    f3 += s3k + (f2 >> 32);
    f2 &= 0xffffffff;
    f3 &= 0xffffffff;
    let mut tag = [0u8; 16];
    tag[0..4].copy_from_slice(&(f0 as u32).to_le_bytes());
    tag[4..8].copy_from_slice(&(f1 as u32).to_le_bytes());
    tag[8..12].copy_from_slice(&(f2 as u32).to_le_bytes());
    tag[12..16].copy_from_slice(&(f3 as u32).to_le_bytes());
    tag
}

fn poly1305_key_gen(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
    let block = chacha20_block(key, 0, nonce);
    let mut out = [0u8; 32];
    out.copy_from_slice(&block[..32]);
    out
}

fn pad16(data: &[u8]) -> Vec<u8> {
    let rem = data.len() % 16;
    if rem == 0 {
        Vec::new()
    } else {
        vec![0u8; 16 - rem]
    }
}

pub fn aead_encrypt(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let otk = poly1305_key_gen(key, nonce);
    let cipher = chacha20_xor(key, nonce, 1, plain);
    let mut mac_in = Vec::new();
    mac_in.extend_from_slice(aad);
    mac_in.extend_from_slice(&pad16(aad));
    mac_in.extend_from_slice(&cipher);
    mac_in.extend_from_slice(&pad16(&cipher));
    mac_in.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_in.extend_from_slice(&(cipher.len() as u64).to_le_bytes());
    let tag = poly1305(&mac_in, &otk);
    let mut out = cipher;
    out.extend_from_slice(&tag);
    out
}

pub fn aead_decrypt(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 16 {
        return None;
    }
    let (cipher, tag) = data.split_at(data.len() - 16);
    let otk = poly1305_key_gen(key, nonce);
    let mut mac_in = Vec::new();
    mac_in.extend_from_slice(aad);
    mac_in.extend_from_slice(&pad16(aad));
    mac_in.extend_from_slice(cipher);
    mac_in.extend_from_slice(&pad16(cipher));
    mac_in.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_in.extend_from_slice(&(cipher.len() as u64).to_le_bytes());
    let expect = poly1305(&mac_in, &otk);
    if !ct_eq(&expect, tag) {
        return None;
    }
    Some(chacha20_xor(key, nonce, 1, cipher))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn hmac_sha256_vectors() {
        let key = [0x0bu8; 20];
        assert_eq!(
            hex(&hmac_sha256(&key, b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hkdf_rfc5869_case1() {
        let ikm = [0x0bu8; 22];
        let salt: Vec<u8> = (0..13).map(|i| i as u8).collect();
        let info: Vec<u8> = (0xf0u8..0xfa).collect();
        let okm = hkdf_sha256(&ikm, &salt, &info, 42);
        assert_eq!(
            hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn chacha20_poly1305_rfc8439() {
        let key: [u8; 32] = [
            0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d,
            0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b,
            0x9c, 0x9d, 0x9e, 0x9f,
        ];
        let nonce: [u8; 12] = [
            0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
        ];
        let aad: [u8; 12] = [
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
        ];
        let plain = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let out = aead_encrypt(&key, &nonce, &aad, plain);
        let expect = "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116";
        let expect_tag = "1ae10b594f09e26a7e902ecbd0600691";
        let mut full = expect.to_string();
        full.push_str(expect_tag);
        assert_eq!(hex(&out), full);
        let back = aead_decrypt(&key, &nonce, &aad, &out).unwrap();
        assert_eq!(back, plain);
        let mut tampered = out.clone();
        tampered[0] ^= 1;
        assert!(aead_decrypt(&key, &nonce, &aad, &tampered).is_none());
    }

    #[test]
    fn hex_roundtrip_and_ct_eq() {
        let data = random_bytes(32);
        assert_eq!(unhex(&hex(&data)).unwrap(), data);
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
