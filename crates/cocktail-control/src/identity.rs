use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::crypto;

pub fn b64url_decode(raw: &str) -> Option<Vec<u8>> {
    let mut s = raw.trim().replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = table.iter().position(|&t| t == c)? as u32;
        buffer = (buffer << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

pub fn b64url_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for b in data {
        buffer = (buffer << 8) | *b as u32;
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(TABLE[((buffer >> bits) & 0x3f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(TABLE[((buffer << (6 - bits)) & 0x3f) as usize] as char);
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct BigUint {
    limbs: Vec<u32>,
}

impl BigUint {
    pub fn from_be_bytes(bytes: &[u8]) -> Self {
        let mut limbs = Vec::with_capacity(bytes.len() / 4 + 1);
        let mut idx = bytes.len();
        while idx > 0 {
            let start = idx.saturating_sub(4);
            let mut v = 0u32;
            for &b in &bytes[start..idx] {
                v = (v << 8) | b as u32;
            }
            limbs.push(v);
            idx = start;
        }
        Self::normalize(limbs)
    }

    pub fn from_u64(v: u64) -> Self {
        Self::normalize(vec![v as u32, (v >> 32) as u32])
    }

    fn normalize(mut limbs: Vec<u32>) -> Self {
        while limbs.len() > 1 && limbs.last() == Some(&0) {
            limbs.pop();
        }
        if limbs.is_empty() {
            limbs.push(0);
        }
        Self { limbs }
    }

    pub fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&l| l == 0)
    }

    pub fn bit_len(&self) -> usize {
        if self.is_zero() {
            return 0;
        }
        let top = *self.limbs.last().unwrap();
        (self.limbs.len() - 1) * 32 + (32 - top.leading_zeros() as usize)
    }

    pub fn bit(&self, idx: usize) -> bool {
        let limb = idx / 32;
        if limb >= self.limbs.len() {
            return false;
        }
        (self.limbs[limb] >> (idx % 32)) & 1 == 1
    }

    pub fn mul(&self, other: &Self) -> Self {
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            if a == 0 {
                continue;
            }
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let cur = out[i + j] as u64 + a as u64 * b as u64 + carry;
                out[i + j] = cur as u32;
                carry = cur >> 32;
            }
            let mut k = i + other.limbs.len();
            while carry > 0 {
                if k >= out.len() {
                    out.push(0);
                }
                let cur = out[k] as u64 + carry;
                out[k] = cur as u32;
                carry = cur >> 32;
                k += 1;
            }
        }
        Self::normalize(out)
    }

    fn rem(&self, m: &Self) -> Self {
        let mut r = Self::normalize(vec![0]);
        let bits = self.bit_len();
        for i in (0..bits).rev() {
            r = r.shl1();
            if self.bit(i) {
                r = r.add_small(1);
            }
            while r.cmp(m) != std::cmp::Ordering::Less {
                r = r.sub(m);
            }
        }
        r
    }

    fn shl1(&self) -> Self {
        let mut out = vec![0u32; self.limbs.len() + 1];
        let mut carry = 0u32;
        for (i, &l) in self.limbs.iter().enumerate() {
            out[i] = (l << 1) | carry;
            carry = l >> 31;
        }
        out[self.limbs.len()] = carry;
        Self::normalize(out)
    }

    fn add_small(&self, v: u32) -> Self {
        let mut out = self.limbs.clone();
        let mut carry = v as u64;
        let mut i = 0;
        while carry > 0 && i < out.len() {
            let cur = out[i] as u64 + carry;
            out[i] = cur as u32;
            carry = cur >> 32;
            i += 1;
        }
        if carry > 0 {
            out.push(carry as u32);
        }
        Self::normalize(out)
    }

    fn sub(&self, other: &Self) -> Self {
        let mut out = vec![0u32; self.limbs.len()];
        let mut borrow = 0i64;
        for i in 0..self.limbs.len() {
            let b = other.limbs.get(i).copied().unwrap_or(0) as i64;
            let cur = self.limbs[i] as i64 - b - borrow;
            if cur < 0 {
                out[i] = (cur + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                out[i] = cur as u32;
                borrow = 0;
            }
        }
        Self::normalize(out)
    }

    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for i in (0..self.limbs.len()).rev() {
            match self.limbs[i].cmp(&other.limbs[i]) {
                std::cmp::Ordering::Equal => continue,
                ord => return ord,
            }
        }
        std::cmp::Ordering::Equal
    }

    pub fn modexp(base: &Self, exp: &Self, modulus: &Self) -> Self {
        if modulus.is_zero() {
            return Self::normalize(vec![0]);
        }
        let mut result = Self::from_u64(1);
        let b = base.rem(modulus);
        let bits = exp.bit_len();
        for i in (0..bits).rev() {
            result = result.mul(&result).rem(modulus);
            if exp.bit(i) {
                result = result.mul(&b).rem(modulus);
            }
        }
        result
    }

    pub fn to_be_bytes(&self, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        for i in (0..self.limbs.len()).rev() {
            out.extend_from_slice(&self.limbs[i].to_be_bytes());
        }
        while out.len() > 1 && out[0] == 0 {
            out.remove(0);
        }
        if out.len() < len {
            let mut padded = vec![0u8; len - out.len()];
            padded.extend_from_slice(&out);
            return padded;
        }
        out
    }
}

pub fn rs256_verify(n_hex: &str, e_hex: &str, msg: &[u8], sig: &[u8]) -> bool {
    let n_bytes = match crypto::unhex(n_hex) {
        Some(b) if !b.is_empty() => b,
        _ => return false,
    };
    let e_bytes = crypto::unhex(e_hex).unwrap_or_else(|| vec![1, 0, 1]);
    let n = BigUint::from_be_bytes(&n_bytes);
    let e = BigUint::from_be_bytes(&e_bytes);
    let s = BigUint::from_be_bytes(sig);
    if s.cmp(&n) != std::cmp::Ordering::Less {
        return false;
    }
    let m = BigUint::modexp(&s, &e, &n);
    let k = n_bytes.len();
    let em = m.to_be_bytes(k);
    let digest = crypto::sha256(msg);
    let mut expected = vec![0x00, 0x01];
    let pad_len = k.saturating_sub(3 + digest.len() + 8);
    expected.extend(std::iter::repeat_n(0xffu8, pad_len));
    expected.push(0x00);
    expected.extend_from_slice(&[
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ]);
    expected.extend_from_slice(&digest);
    if expected.len() != em.len() {
        return false;
    }
    crypto::ct_eq(&expected, &em)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwtHeader {
    pub alg: String,
    pub kid: Option<String>,
    pub typ: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: Option<String>,
    pub aud: Option<serde_json::Value>,
    pub exp: Option<i64>,
    pub iat: Option<i64>,
    pub nbf: Option<i64>,
    pub nonce: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub groups: Option<serde_json::Value>,
}

impl Claims {
    pub fn group_list(&self) -> Vec<String> {
        match &self.groups {
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            Some(serde_json::Value::String(s)) => s
                .split([',', ' '])
                .filter(|t| !t.is_empty())
                .map(|t| t.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn audience_contains(&self, expect: &str) -> bool {
        match &self.aud {
            Some(serde_json::Value::String(s)) => s == expect,
            Some(serde_json::Value::Array(items)) => {
                items.iter().any(|v| v.as_str() == Some(expect))
            }
            _ => false,
        }
    }

    pub fn is_expired(&self, now: i64, leeway: i64) -> bool {
        self.exp.map(|e| now > e + leeway).unwrap_or(false)
    }

    pub fn not_yet_valid(&self, now: i64, leeway: i64) -> bool {
        self.nbf.map(|n| now + leeway < n).unwrap_or(false)
    }

    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.email.clone())
            .unwrap_or_else(|| self.sub.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwksKey {
    pub kid: String,
    pub kty: String,
    pub alg: String,
    pub n: Option<String>,
    pub e: Option<String>,
    pub x5c: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Jwks {
    pub keys: Vec<JwksKey>,
}

impl Jwks {
    pub fn find(&self, kid: Option<&str>) -> Option<&JwksKey> {
        match kid {
            Some(k) => self.keys.iter().find(|key| key.kid == k),
            None => self.keys.first(),
        }
    }

    pub fn parse(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JwtError {
    Malformed,
    UnsupportedAlg,
    BadSignature,
    Expired,
    NotYetValid,
    WrongIssuer,
    WrongAudience,
    UnknownKey,
    NonceMismatch,
}

impl JwtError {
    pub fn message(&self) -> &'static str {
        match self {
            JwtError::Malformed => "令牌格式错误",
            JwtError::UnsupportedAlg => "不支持的签名算法",
            JwtError::BadSignature => "签名校验失败",
            JwtError::Expired => "令牌已过期",
            JwtError::NotYetValid => "令牌尚未生效",
            JwtError::WrongIssuer => "签发者不匹配",
            JwtError::WrongAudience => "受众不匹配",
            JwtError::UnknownKey => "找不到匹配的公钥",
            JwtError::NonceMismatch => "nonce 不匹配",
        }
    }
}

pub struct JwtVerifier<'a> {
    jwks: Option<&'a Jwks>,
    hmac_secret: Option<&'a [u8]>,
    issuer: Option<String>,
    audience: Option<String>,
    leeway: i64,
}

impl<'a> JwtVerifier<'a> {
    pub fn new() -> Self {
        Self {
            jwks: None,
            hmac_secret: None,
            issuer: None,
            audience: None,
            leeway: 60,
        }
    }

    pub fn with_jwks(mut self, jwks: &'a Jwks) -> Self {
        self.jwks = Some(jwks);
        self
    }

    pub fn with_hmac(mut self, secret: &'a [u8]) -> Self {
        self.hmac_secret = Some(secret);
        self
    }

    pub fn expect_issuer(mut self, iss: &str) -> Self {
        self.issuer = Some(iss.to_string());
        self
    }

    pub fn expect_audience(mut self, aud: &str) -> Self {
        self.audience = Some(aud.to_string());
        self
    }

    pub fn with_leeway(mut self, secs: i64) -> Self {
        self.leeway = secs;
        self
    }

    pub fn verify(&self, token: &str, now: i64, nonce: Option<&str>) -> Result<Claims, JwtError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(JwtError::Malformed);
        }
        let header_bytes = b64url_decode(parts[0]).ok_or(JwtError::Malformed)?;
        let payload_bytes = b64url_decode(parts[1]).ok_or(JwtError::Malformed)?;
        let sig = b64url_decode(parts[2]).ok_or(JwtError::Malformed)?;
        let header: JwtHeader =
            serde_json::from_slice(&header_bytes).map_err(|_| JwtError::Malformed)?;
        let claims: Claims =
            serde_json::from_slice(&payload_bytes).map_err(|_| JwtError::Malformed)?;
        let signed = format!("{}.{}", parts[0], parts[1]);
        match header.alg.as_str() {
            "HS256" => {
                let Some(secret) = self.hmac_secret else {
                    return Err(JwtError::UnsupportedAlg);
                };
                let mac = crypto::hmac_sha256(secret, signed.as_bytes());
                if !crypto::ct_eq(&mac, &sig) {
                    return Err(JwtError::BadSignature);
                }
            }
            "RS256" => {
                let Some(jwks) = self.jwks else {
                    return Err(JwtError::UnsupportedAlg);
                };
                let key = jwks
                    .find(header.kid.as_deref())
                    .ok_or(JwtError::UnknownKey)?;
                let (Some(n), Some(e)) = (key.n.as_deref(), key.e.as_deref()) else {
                    return Err(JwtError::UnknownKey);
                };
                let n_hex = crypto::hex(&b64url_decode(n).ok_or(JwtError::UnknownKey)?);
                let e_hex = crypto::hex(&b64url_decode(e).ok_or(JwtError::UnknownKey)?);
                if !rs256_verify(&n_hex, &e_hex, signed.as_bytes(), &sig) {
                    return Err(JwtError::BadSignature);
                }
            }
            "none" => return Err(JwtError::UnsupportedAlg),
            _ => return Err(JwtError::UnsupportedAlg),
        }
        if claims.is_expired(now, self.leeway) {
            return Err(JwtError::Expired);
        }
        if claims.not_yet_valid(now, self.leeway) {
            return Err(JwtError::NotYetValid);
        }
        if let Some(expect) = &self.issuer {
            if claims.iss.as_deref() != Some(expect.as_str()) {
                return Err(JwtError::WrongIssuer);
            }
        }
        if let Some(expect) = &self.audience {
            if !claims.audience_contains(expect) {
                return Err(JwtError::WrongAudience);
            }
        }
        if let Some(expect) = nonce {
            if claims.nonce.as_deref() != Some(expect) {
                return Err(JwtError::NonceMismatch);
            }
        }
        Ok(claims)
    }
}

impl Default for JwtVerifier<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OidcProvider {
    pub id: String,
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub auto_create_users: bool,
    pub default_role: String,
    pub group_role_map: BTreeMap<String, String>,
    pub require_verified_email: bool,
}

impl OidcProvider {
    pub fn new(id: &str, issuer: &str, client_id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            issuer: issuer.trim_end_matches('/').to_string(),
            client_id: client_id.to_string(),
            client_secret: String::new(),
            authorization_endpoint: format!("{}/authorize", issuer.trim_end_matches('/')),
            token_endpoint: format!("{}/token", issuer.trim_end_matches('/')),
            jwks_uri: format!("{}/jwks", issuer.trim_end_matches('/')),
            scopes: vec!["openid".into(), "profile".into(), "email".into()],
            enabled: true,
            auto_create_users: true,
            default_role: "observer".into(),
            group_role_map: BTreeMap::new(),
            require_verified_email: true,
        }
    }

    pub fn map_role(&self, groups: &[String]) -> String {
        for g in groups {
            if let Some(role) = self.group_role_map.get(g) {
                return role.clone();
            }
        }
        self.default_role.clone()
    }

    pub fn authorization_url(&self, redirect_uri: &str, state: &str, nonce: &str) -> String {
        let scopes = self.scopes.join(" ");
        format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}",
            self.authorization_endpoint,
            urlencode(&self.client_id),
            urlencode(redirect_uri),
            urlencode(&scopes),
            urlencode(state),
            urlencode(nonce)
        )
    }

    pub fn validate_issuer(&self, claims: &Claims) -> bool {
        claims.iss.as_deref() == Some(self.issuer.as_str())
    }

    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.issuer.is_empty() {
            issues.push("issuer 不能为空".into());
        }
        if self.client_id.is_empty() {
            issues.push("client_id 不能为空".into());
        }
        if self.client_secret.is_empty() {
            issues.push("client_secret 不能为空".into());
        }
        if !self.issuer.starts_with("https://") {
            issues.push("issuer 必须使用 https".into());
        }
        issues
    }
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn random_state() -> String {
    crypto::random_token(32)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LdapConfig {
    pub url: String,
    pub bind_dn: String,
    pub bind_password: String,
    pub user_base: String,
    pub user_filter: String,
    pub group_base: String,
    pub group_filter: String,
    pub user_dn_template: String,
    pub group_role_map: BTreeMap<String, String>,
    pub default_role: String,
    pub enabled: bool,
    pub tls: bool,
}

impl LdapConfig {
    pub fn new(url: &str, user_base: &str) -> Self {
        Self {
            url: url.to_string(),
            bind_dn: String::new(),
            bind_password: String::new(),
            user_base: user_base.to_string(),
            user_filter: "(uid={user})".into(),
            group_base: String::new(),
            group_filter: "(member={dn})".into(),
            user_dn_template: format!("uid={{user}},{user_base}"),
            group_role_map: BTreeMap::new(),
            default_role: "observer".into(),
            enabled: false,
            tls: true,
        }
    }

    pub fn user_dn(&self, username: &str) -> String {
        self.user_dn_template
            .replace("{user}", &escape_filter(username))
    }

    pub fn search_filter(&self, username: &str) -> String {
        self.user_filter.replace("{user}", &escape_filter(username))
    }

    pub fn map_role(&self, groups: &[String]) -> String {
        for g in groups {
            if let Some(role) = self.group_role_map.get(g) {
                return role.clone();
            }
        }
        self.default_role.clone()
    }

    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.url.is_empty() {
            issues.push("LDAP 地址不能为空".into());
        }
        if !self.url.starts_with("ldap://") && !self.url.starts_with("ldaps://") {
            issues.push("LDAP 地址需以 ldap:// 或 ldaps:// 开头".into());
        }
        if self.user_base.is_empty() {
            issues.push("user_base 不能为空".into());
        }
        if self.user_dn_template.is_empty() {
            issues.push("user_dn_template 不能为空".into());
        }
        if self.tls && self.url.starts_with("ldap://") {
            issues.push("启用 TLS 时应使用 ldaps://".into());
        }
        issues
    }
}

pub fn escape_filter(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        match c {
            '*' => out.push_str("\\2a"),
            '(' => out.push_str("\\28"),
            ')' => out.push_str("\\29"),
            '\\' => out.push_str("\\5c"),
            '\0' => out.push_str("\\00"),
            '/' => out.push_str("\\2f"),
            _ => out.push(c),
        }
    }
    out
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SsoUser {
    pub external_id: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub groups: Vec<String>,
    pub role: String,
    pub provider: String,
}

impl SsoUser {
    pub fn from_oidc(claims: &Claims, provider: &OidcProvider) -> Self {
        let groups = claims.group_list();
        let role = provider.map_role(&groups);
        let username = sanitize_username(
            claims
                .email
                .as_deref()
                .and_then(|e| e.split('@').next())
                .unwrap_or(&claims.sub),
        );
        Self {
            external_id: claims.sub.clone(),
            username,
            display_name: claims.display_name(),
            email: claims.email.clone(),
            groups,
            role,
            provider: provider.id.clone(),
        }
    }

    pub fn from_ldap(username: &str, groups: Vec<String>, config: &LdapConfig) -> Self {
        let role = config.map_role(&groups);
        Self {
            external_id: config.user_dn(username),
            username: sanitize_username(username),
            display_name: username.to_string(),
            email: None,
            groups,
            role,
            provider: "ldap".into(),
        }
    }

    pub fn unique_username(&self, taken: &BTreeSet<String>) -> String {
        if !taken.contains(&self.username) {
            return self.username.clone();
        }
        for i in 1..1000 {
            let candidate = format!("{}-{}", self.username, i);
            if !taken.contains(&candidate) {
                return candidate;
            }
        }
        format!("{}-{}", self.username, crypto::random_token(4))
    }
}

pub fn sanitize_username(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .take(32)
        .collect();
    if cleaned.is_empty() {
        "sso-user".into()
    } else {
        cleaned.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_roundtrip() {
        let data = b"hello world!\x00\xff";
        let enc = b64url_encode(data);
        assert!(!enc.contains('='));
        assert!(!enc.contains('+'));
        assert_eq!(b64url_decode(&enc).unwrap(), data);
        assert_eq!(b64url_decode("aGVsbG8").unwrap(), b"hello");
    }

    #[test]
    fn biguint_modexp_small() {
        let base = BigUint::from_u64(4);
        let exp = BigUint::from_u64(13);
        let m = BigUint::from_u64(497);
        let r = BigUint::modexp(&base, &exp, &m);
        assert_eq!(r.to_be_bytes(8).last().copied(), Some(445u32 as u8));
        let bytes = r.to_be_bytes(8);
        let value = bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64);
        assert_eq!(value, 445);
    }

    #[test]
    fn biguint_multiplication_and_bits() {
        let a = BigUint::from_u64(0xffff_ffff);
        let b = BigUint::from_u64(0xffff_ffff);
        let p = a.mul(&b);
        let bytes = p.to_be_bytes(16);
        let value = bytes.iter().fold(0u128, |acc, &x| (acc << 8) | x as u128);
        assert_eq!(value, 0xffff_fffe_0000_0001u128);
        assert!(BigUint::from_u64(1).bit(0));
        assert!(!BigUint::from_u64(1).bit(1));
        assert_eq!(BigUint::from_u64(255).bit_len(), 8);
    }

    #[test]
    fn jwt_hs256_roundtrip() {
        let secret = b"topsecretkey";
        let header = b64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let now = 1_700_000_000i64;
        let payload_json = format!(
            "{{\"sub\":\"user1\",\"iss\":\"https://idp.example\",\"aud\":\"cocktail\",\"exp\":{},\"nonce\":\"n1\",\"email\":\"a@b.c\"}}",
            now + 600
        );
        let payload = b64url_encode(payload_json.as_bytes());
        let signed = format!("{header}.{payload}");
        let mac = crypto::hmac_sha256(secret, signed.as_bytes());
        let token = format!("{signed}.{}", b64url_encode(&mac));
        let verifier = JwtVerifier::new()
            .with_hmac(secret)
            .expect_issuer("https://idp.example")
            .expect_audience("cocktail");
        let claims = verifier.verify(&token, now, Some("n1")).unwrap();
        assert_eq!(claims.sub, "user1");
        assert_eq!(claims.email.as_deref(), Some("a@b.c"));
        assert_eq!(claims.display_name(), "a@b.c");
    }

    #[test]
    fn jwt_rejects_bad_signature_and_alg() {
        let secret = b"k";
        let header = b64url_encode(br#"{"alg":"HS256"}"#);
        let payload = b64url_encode(br#"{"sub":"u","exp":9999999999}"#);
        let token = format!("{header}.{payload}.AAAA");
        let v = JwtVerifier::new().with_hmac(secret);
        assert!(matches!(
            v.verify(&token, 0, None),
            Err(JwtError::BadSignature)
        ));
        let none_header = b64url_encode(br#"{"alg":"none"}"#);
        let forged = format!("{none_header}.{payload}.");
        assert!(v.verify(&forged, 0, None).is_err());
    }

    #[test]
    fn jwt_rejects_expired_and_wrong_audience() {
        let secret = b"s";
        let header = b64url_encode(br#"{"alg":"HS256"}"#);
        let payload = b64url_encode(br#"{"sub":"u","exp":1000,"aud":"other"}"#);
        let signed = format!("{header}.{payload}");
        let mac = crypto::hmac_sha256(secret, signed.as_bytes());
        let token = format!("{signed}.{}", b64url_encode(&mac));
        let v = JwtVerifier::new().with_hmac(secret).with_leeway(0);
        assert!(matches!(
            v.verify(&token, 5000, None),
            Err(JwtError::Expired)
        ));
        let v2 = JwtVerifier::new()
            .with_hmac(secret)
            .expect_audience("cocktail");
        assert!(matches!(
            v2.verify(&token, 500, None),
            Err(JwtError::WrongAudience)
        ));
    }

    #[test]
    fn jwt_nonce_mismatch() {
        let secret = b"s";
        let header = b64url_encode(br#"{"alg":"HS256"}"#);
        let payload = b64url_encode(br#"{"sub":"u","nonce":"abc"}"#);
        let signed = format!("{header}.{payload}");
        let mac = crypto::hmac_sha256(secret, signed.as_bytes());
        let token = format!("{signed}.{}", b64url_encode(&mac));
        let v = JwtVerifier::new().with_hmac(secret);
        assert!(matches!(
            v.verify(&token, 0, Some("xyz")),
            Err(JwtError::NonceMismatch)
        ));
        assert!(v.verify(&token, 0, Some("abc")).is_ok());
    }

    #[test]
    fn claims_group_and_audience_helpers() {
        let claims: Claims =
            serde_json::from_str(r#"{"sub":"u","aud":["a","b"],"groups":["admins","devs"]}"#)
                .unwrap();
        assert_eq!(claims.group_list(), vec!["admins", "devs"]);
        assert!(claims.audience_contains("b"));
        assert!(!claims.audience_contains("c"));
        let single: Claims = serde_json::from_str(r#"{"sub":"u","groups":"admins devs"}"#).unwrap();
        assert_eq!(single.group_list().len(), 2);
    }

    #[test]
    fn jwks_parsing_and_lookup() {
        let json = r#"{"keys":[{"kid":"k1","kty":"RSA","alg":"RS256","n":"AQAB","e":"AQAB"}]}"#;
        let jwks = Jwks::parse(json).unwrap();
        assert_eq!(jwks.keys.len(), 1);
        assert!(jwks.find(Some("k1")).is_some());
        assert!(jwks.find(Some("nope")).is_none());
        assert!(jwks.find(None).is_some());
    }

    #[test]
    fn oidc_provider_config() {
        let p = OidcProvider::new("keycloak", "https://idp.example", "cocktail");
        assert!(p.authorization_endpoint.ends_with("/authorize"));
        assert_eq!(p.jwks_uri, "https://idp.example/jwks");
        let url = p.authorization_url("https://app/cb", "st", "nc");
        assert!(url.contains("client_id=cocktail"));
        assert!(url.contains("state=st"));
        assert!(url.contains("nonce=nc"));
        assert!(url.contains("response_type=code"));
    }

    #[test]
    fn oidc_validation_flags_insecure() {
        let mut p = OidcProvider::new("x", "http://insecure", "c");
        p.client_secret = "s".into();
        let issues = p.validate();
        assert!(issues.iter().any(|i| i.contains("https")));
        let mut good = OidcProvider::new("x", "https://idp", "c");
        good.client_secret = "s".into();
        assert!(good.validate().is_empty());
    }

    #[test]
    fn group_role_mapping() {
        let mut p = OidcProvider::new("p", "https://i", "c");
        p.group_role_map.insert("mc-admins".into(), "admin".into());
        p.group_role_map.insert("mc-ops".into(), "support".into());
        assert_eq!(p.map_role(&["other".into(), "mc-ops".into()]), "support");
        assert_eq!(p.map_role(&["unknown".into()]), "observer");
    }

    #[test]
    fn sso_user_derivation() {
        let mut p = OidcProvider::new("keycloak", "https://i", "c");
        p.group_role_map.insert("admins".into(), "admin".into());
        let claims: Claims = serde_json::from_str(
            r#"{"sub":"abc-123","email":"Alice.Smith@corp.com","name":"Alice","groups":["admins"]}"#,
        )
        .unwrap();
        let u = SsoUser::from_oidc(&claims, &p);
        assert_eq!(u.username, "alice.smith");
        assert_eq!(u.role, "admin");
        assert_eq!(u.display_name, "Alice");
        assert_eq!(u.provider, "keycloak");
    }

    #[test]
    fn sso_username_collision_resolution() {
        let u = SsoUser {
            external_id: "1".into(),
            username: "alice".into(),
            display_name: "A".into(),
            email: None,
            groups: vec![],
            role: "observer".into(),
            provider: "p".into(),
        };
        let mut taken = BTreeSet::new();
        assert_eq!(u.unique_username(&taken), "alice");
        taken.insert("alice".to_string());
        assert_eq!(u.unique_username(&taken), "alice-1");
        taken.insert("alice-1".to_string());
        assert_eq!(u.unique_username(&taken), "alice-2");
    }

    #[test]
    fn username_sanitization() {
        assert_eq!(sanitize_username("Alice Smith!"), "alicesmith");
        assert_eq!(sanitize_username("bob.admin"), "bob.admin");
        assert_eq!(sanitize_username(""), "sso-user");
        assert_eq!(sanitize_username("!!!"), "sso-user");
    }

    #[test]
    fn ldap_filter_escaping() {
        assert_eq!(escape_filter("a*b"), "a\\2ab");
        assert_eq!(escape_filter("(x)"), "\\28x\\29");
        assert_eq!(escape_filter("a\\b"), "a\\5cb");
        assert_eq!(escape_filter("normal"), "normal");
    }

    #[test]
    fn ldap_dn_and_filter_building() {
        let c = LdapConfig::new("ldaps://ldap.corp:636", "ou=people,dc=corp,dc=com");
        assert_eq!(c.user_dn("alice"), "uid=alice,ou=people,dc=corp,dc=com");
        assert_eq!(c.search_filter("alice"), "(uid=alice)");
        let mut c2 = c.clone();
        c2.tls = true;
        c2.url = "ldap://plain:389".into();
        assert!(c2.validate().iter().any(|i| i.contains("ldaps")));
    }

    #[test]
    fn ldap_role_mapping_and_validation() {
        let mut c = LdapConfig::new("ldaps://ldap.corp", "ou=p");
        c.group_role_map.insert("cn=ops".into(), "admin".into());
        assert_eq!(c.map_role(&["cn=ops".into()]), "admin");
        assert_eq!(c.map_role(&[]), "observer");
        let bad = LdapConfig {
            url: "http://wrong".into(),
            user_base: String::new(),
            user_dn_template: String::new(),
            ..c.clone()
        };
        let issues = bad.validate();
        assert!(issues.iter().any(|i| i.contains("ldap://")));
        assert!(issues.iter().any(|i| i.contains("user_base")));
    }

    #[test]
    fn urlencoding_escapes_specials() {
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("a/b?c=d&e"), "a%2Fb%3Fc%3Dd%26e");
        assert_eq!(urlencode("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn random_state_is_unique() {
        let a = random_state();
        let b = random_state();
        assert_ne!(a, b);
        assert!(a.len() >= 32);
    }
}
