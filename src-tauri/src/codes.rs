//! Short human-typeable pairing codes and key derivation.

use anyhow::{bail, Result};
use data_encoding::BASE32_NOPAD;
use iroh::SecretKey;
use rand::RngCore;
use serde::Serialize;

/// Crockford alphabet (no I, L, O, U) - easier to read aloud.
const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

const CODE_BYTES: usize = 5; // ~8 crockford chars
pub const CODE_TTL_SECS: u64 = 600; // 10 minutes (peer codes only)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeKind {
    Peer,
    Room,
}

impl CodeKind {
    pub fn prefix(self) -> char {
        match self {
            CodeKind::Peer => 'P',
            CodeKind::Room => 'R',
        }
    }

    pub fn from_prefix(c: char) -> Option<Self> {
        match c.to_ascii_uppercase() {
            'P' => Some(CodeKind::Peer),
            'R' => Some(CodeKind::Room),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CodeKind::Peer => "peer",
            CodeKind::Room => "room",
        }
    }
}

/// Generate a peer code like `P-H3K9-M2PQ`.
pub fn generate_peer_code() -> String {
    format_typed(CodeKind::Peer, &raw_body())
}

/// Generate a room code like `R-H3K9-M2PQ`.
pub fn generate_room_code() -> String {
    format_typed(CodeKind::Room, &raw_body())
}

fn raw_body() -> String {
    let mut raw = [0u8; CODE_BYTES];
    rand::thread_rng().fill_bytes(&mut raw);
    let encoded = encode_crockford(&raw);
    format!("{}-{}", &encoded[..4], &encoded[4..])
}

fn format_typed(kind: CodeKind, body: &str) -> String {
    format!("{}-{}", kind.prefix(), body)
}

/// Normalize: strip separators, uppercase, map ambiguous chars. Keeps leading P/R.
pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            'U' => 'V',
            other => other,
        })
        .collect()
}

/// Parse typed code → (kind, normalized body without prefix).
pub fn parse_code(input: &str) -> Result<(CodeKind, String)> {
    let n = normalize_code(input);
    if n.len() < 7 {
        bail!("invalid code");
    }
    let kind = CodeKind::from_prefix(n.chars().next().unwrap())
        .ok_or_else(|| anyhow::anyhow!("code must start with P (peer) or R (room)"))?;
    let body: String = n.chars().skip(1).collect();
    validate_body(&body)?;
    Ok((kind, body))
}

pub fn validate_body(body: &str) -> Result<()> {
    if body.len() < 6 || body.len() > 12 {
        bail!("invalid code length");
    }
    if !body.bytes().all(|b| CROCKFORD.contains(&b)) {
        bail!("invalid code characters");
    }
    Ok(())
}

/// Material for KDF / SPAKE password: includes kind so P/R never collide.
pub fn password_material(kind: CodeKind, body: &str) -> String {
    format!("{}:{}", kind.as_str(), body)
}

/// Derive an ephemeral iroh SecretKey from the pairing password.
pub fn ephemeral_secret(kind: CodeKind, body: &str) -> SecretKey {
    let mat = password_material(kind, body);
    let key = blake3::derive_key("ratline-pair-eph-v1", mat.as_bytes());
    SecretKey::from_bytes(&key)
}

pub fn dm_topic_id(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut hasher = blake3::Hasher::new_derive_key("ratline-dm-topic-v1");
    hasher.update(lo);
    hasher.update(hi);
    *hasher.finalize().as_bytes()
}

pub fn room_topic_from_code(body: &str) -> [u8; 32] {
    blake3::derive_key(
        "ratline-room-topic-v1",
        password_material(CodeKind::Room, body).as_bytes(),
    )
}

pub fn session_key_from_spake(shared: &[u8]) -> [u8; 32] {
    blake3::derive_key("ratline-session-v1", shared)
}

fn encode_crockford(bytes: &[u8]) -> String {
    let mut bits: u64 = 0;
    let mut nbits: u32 = 0;
    let mut out = String::new();
    for &b in bytes {
        bits = (bits << 8) | u64::from(b);
        nbits += 8;
        while nbits >= 5 {
            nbits -= 5;
            let idx = ((bits >> nbits) & 0x1f) as usize;
            out.push(CROCKFORD[idx] as char);
        }
    }
    if nbits > 0 {
        let idx = ((bits << (5 - nbits)) & 0x1f) as usize;
        out.push(CROCKFORD[idx] as char);
    }
    out
}

pub fn short_id(bytes: &[u8]) -> String {
    let enc = BASE32_NOPAD.encode(bytes);
    enc.chars().take(8).collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_and_room_distinct() {
        let p = generate_peer_code();
        let r = generate_room_code();
        assert!(p.starts_with("P-"));
        assert!(r.starts_with("R-"));
        let (kp, bp) = parse_code(&p).unwrap();
        let (kr, br) = parse_code(&r).unwrap();
        assert_eq!(kp, CodeKind::Peer);
        assert_eq!(kr, CodeKind::Room);
        assert_ne!(
            ephemeral_secret(kp, &bp).public(),
            ephemeral_secret(kr, &br).public()
        );
    }

    #[test]
    fn normalize_ambiguous() {
        assert_eq!(normalize_code("p-oi-lu"), "P011V");
    }

    #[test]
    fn dm_topic_symmetric() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_eq!(dm_topic_id(&a, &b), dm_topic_id(&b, &a));
    }

    #[test]
    fn rejects_untyped() {
        assert!(parse_code("H3K9-M2PQ").is_err());
    }
}
