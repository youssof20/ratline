//! SPAKE2 pairing handshake helpers (unit-tested without network).

use anyhow::{anyhow, bail, Context, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::codes;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityPayload {
    /// Long-term iroh EndpointId bytes.
    pub endpoint_id: [u8; 32],
    /// Optional display hint (local only; not verified).
    pub label: String,
    /// For room invites: gossip topic id.
    pub topic_id: Option<[u8; 32]>,
    /// Live room member count when known (host-reported).
    #[serde(default)]
    pub member_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurePayload {
    pub identity: IdentityPayload,
    pub signature: Vec<u8>,
}

pub struct SpakeRoleA {
    state: Spake2<Ed25519Group>,
    pub outbound: Vec<u8>,
}

pub struct SpakeRoleB {
    state: Spake2<Ed25519Group>,
    pub outbound: Vec<u8>,
}

const ID_A: &[u8] = b"ratline-a";
const ID_B: &[u8] = b"ratline-b";

pub fn start_a(password: &str) -> SpakeRoleA {
    let (state, outbound) = Spake2::<Ed25519Group>::start_a(
        &Password::new(password.as_bytes()),
        &Identity::new(ID_A),
        &Identity::new(ID_B),
    );
    SpakeRoleA { state, outbound }
}

pub fn start_b(password: &str) -> SpakeRoleB {
    let (state, outbound) = Spake2::<Ed25519Group>::start_b(
        &Password::new(password.as_bytes()),
        &Identity::new(ID_A),
        &Identity::new(ID_B),
    );
    SpakeRoleB { state, outbound }
}

pub fn finish_a(role: SpakeRoleA, msg_b: &[u8]) -> Result<[u8; 32]> {
    let key = role
        .state
        .finish(msg_b)
        .map_err(|e| anyhow!("spake2 finish a: {e:?}"))?;
    Ok(codes::session_key_from_spake(&key))
}

pub fn finish_b(role: SpakeRoleB, msg_a: &[u8]) -> Result<[u8; 32]> {
    let key = role
        .state
        .finish(msg_a)
        .map_err(|e| anyhow!("spake2 finish b: {e:?}"))?;
    Ok(codes::session_key_from_spake(&key))
}

/// Seal an identity payload with the SPAKE2-derived session key.
pub fn seal_payload(session_key: &[u8; 32], payload: &IdentityPayload) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(session_key.into());
    let nonce = Nonce::from_slice(&[0u8; 12]); // single-use key per pairing
    let plaintext = serde_json::to_vec(payload)?;
    cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| anyhow!("encrypt: {e}"))
}

pub fn open_payload(session_key: &[u8; 32], ciphertext: &[u8]) -> Result<IdentityPayload> {
    let cipher = ChaCha20Poly1305::new(session_key.into());
    let nonce = Nonce::from_slice(&[0u8; 12]);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow!("decrypt failed - wrong code or tampered payload"))?;
    serde_json::from_slice(&plaintext).context("payload json")
}

/// Length-prefixed frame helpers for the pairing stream.
pub fn encode_frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(kind);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

pub async fn read_frame<R: tokio::io::AsyncReadExt + Unpin>(
    r: &mut R,
) -> Result<(u8, Vec<u8>)> {
    let mut hdr = [0u8; 5];
    r.read_exact(&mut hdr).await?;
    let kind = hdr[0];
    let len = u32::from_be_bytes(hdr[1..5].try_into().unwrap()) as usize;
    if len > 1_000_000 {
        bail!("frame too large");
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok((kind, body))
}

pub const FRAME_SPAKE: u8 = 1;
pub const FRAME_IDENTITY: u8 = 2;
pub const FRAME_ROOM: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spake2_and_seal_roundtrip() {
        let pw = "H3K9M2PQ";
        let a = start_a(pw);
        let b = start_b(pw);
        let msg_a = a.outbound.clone();
        let msg_b = b.outbound.clone();
        let key_a = finish_a(a, &msg_b).unwrap();
        let key_b = finish_b(b, &msg_a).unwrap();
        assert_eq!(key_a, key_b);

        let payload = IdentityPayload {
            endpoint_id: [7u8; 32],
            label: "alice".into(),
            topic_id: Some([9u8; 32]),
            member_count: Some(2),
        };
        let ct = seal_payload(&key_a, &payload).unwrap();
        let opened = open_payload(&key_b, &ct).unwrap();
        assert_eq!(opened, payload);
    }

    #[test]
    fn wrong_password_fails() {
        let a = start_a("AAAA-AAAA");
        let b = start_b("BBBB-BBBB");
        let msg_a = a.outbound.clone();
        let msg_b = b.outbound.clone();
        let key_a = finish_a(a, &msg_b).unwrap();
        let key_b = finish_b(b, &msg_a).unwrap();
        // SPAKE2 still "finishes" but keys differ
        assert_ne!(key_a, key_b);
        let payload = IdentityPayload {
            endpoint_id: [1u8; 32],
            label: String::new(),
            topic_id: None,
            member_count: None,
        };
        let ct = seal_payload(&key_a, &payload).unwrap();
        assert!(open_payload(&key_b, &ct).is_err());
    }
}
