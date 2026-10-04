use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::crypto;

const KEY_FILE: &str = "data/master.key";
const PREFIX: &str = "enc:v1:";

static MASTER: OnceLock<Vec<u8>> = OnceLock::new();

fn key_path() -> PathBuf {
    PathBuf::from(KEY_FILE)
}

fn derive_from_passphrase(pass: &str) -> Vec<u8> {
    let salt = b"cocktail-master-key-v1";
    crypto::hkdf_sha256(pass.as_bytes(), salt, b"field-encryption", 32)
}

fn load_or_create() -> Vec<u8> {
    if let Ok(raw) = std::env::var("COCKTAIL_MASTER_KEY") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            if let Some(bytes) = crypto::unhex(trimmed) {
                if bytes.len() == 32 {
                    return bytes;
                }
            }
            return derive_from_passphrase(trimmed);
        }
    }
    let path = key_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Some(bytes) = crypto::unhex(text.trim()) {
            if bytes.len() == 32 {
                return bytes;
            }
        }
    }
    let key = crypto::random_bytes(32);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, crypto::hex(&key));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
    key
}

pub fn master_key() -> &'static [u8] {
    MASTER.get_or_init(load_or_create)
}

pub fn key_source() -> &'static str {
    if std::env::var("COCKTAIL_MASTER_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        "env"
    } else if Path::new(KEY_FILE).exists() {
        "file"
    } else {
        "generated"
    }
}

pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(PREFIX)
}

pub fn encrypt(aad: &str, plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    if is_encrypted(plain) {
        return plain.to_string();
    }
    let key = master_key();
    let mut k = [0u8; 32];
    k.copy_from_slice(&key[..32]);
    let nonce_bytes = crypto::random_bytes(12);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes);
    let cipher = crypto::aead_encrypt(&k, &nonce, aad.as_bytes(), plain.as_bytes());
    let mut payload = nonce_bytes;
    payload.extend_from_slice(&cipher);
    format!("{PREFIX}{}", crypto::hex(&payload))
}

pub fn decrypt(aad: &str, value: &str) -> String {
    if value.is_empty() || !is_encrypted(value) {
        return value.to_string();
    }
    let Some(raw) = crypto::unhex(&value[PREFIX.len()..]) else {
        return String::new();
    };
    if raw.len() < 13 {
        return String::new();
    }
    let key = master_key();
    let mut k = [0u8; 32];
    k.copy_from_slice(&key[..32]);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&raw[..12]);
    match crypto::aead_decrypt(&k, &nonce, aad.as_bytes(), &raw[12..]) {
        Some(plain) => String::from_utf8_lossy(&plain).to_string(),
        None => String::new(),
    }
}

pub fn rotate_aad(aad: &str, value: &str) -> String {
    let plain = decrypt(aad, value);
    encrypt(aad, &plain)
}

pub fn encrypt_bytes(aad: &str, data: &[u8]) -> Vec<u8> {
    let key = master_key();
    let mut k = [0u8; 32];
    k.copy_from_slice(&key[..32]);
    let nonce_bytes = crypto::random_bytes(12);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes);
    let cipher = crypto::aead_encrypt(&k, &nonce, aad.as_bytes(), data);
    let mut out = b"CKTB1".to_vec();
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&cipher);
    out
}

pub fn decrypt_bytes(aad: &str, data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 5 + 12 + 16 || &data[..5] != b"CKTB1" {
        return None;
    }
    let key = master_key();
    let mut k = [0u8; 32];
    k.copy_from_slice(&key[..32]);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&data[5..17]);
    crypto::aead_decrypt(&k, &nonce, aad.as_bytes(), &data[17..])
}

pub fn is_encrypted_bytes(data: &[u8]) -> bool {
    data.len() >= 5 && &data[..5] == b"CKTB1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_string() {
        let secret = "Sup3rSecret!rcon";
        let enc = encrypt("instances.rcon_password", secret);
        assert!(is_encrypted(&enc));
        assert_ne!(enc, secret);
        assert_eq!(decrypt("instances.rcon_password", &enc), secret);
    }

    #[test]
    fn wrong_aad_yields_empty() {
        let enc = encrypt("field.a", "value");
        assert_eq!(decrypt("field.b", &enc), "");
    }

    #[test]
    fn tamper_detected() {
        let enc = encrypt("field.a", "value");
        let mut chars: Vec<char> = enc.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'a' { 'b' } else { 'a' };
        let bad: String = chars.into_iter().collect();
        assert_eq!(decrypt("field.a", &bad), "");
    }

    #[test]
    fn idempotent_and_empty() {
        assert_eq!(encrypt("a", ""), "");
        let once = encrypt("a", "x");
        assert_eq!(encrypt("a", &once), once);
        assert_eq!(decrypt("a", ""), "");
        assert_eq!(decrypt("a", "plain-not-encrypted"), "plain-not-encrypted");
    }

    #[test]
    fn bytes_roundtrip() {
        let data = b"binary payload \x00\x01\x02".to_vec();
        let enc = encrypt_bytes("backup.index", &data);
        assert!(is_encrypted_bytes(&enc));
        assert_eq!(decrypt_bytes("backup.index", &enc).unwrap(), data);
        assert!(decrypt_bytes("other", &enc).is_none());
    }
}