//! Manual sealed envelopes (dead drops) — encrypt to a peer's public key.
//! Not offline messaging. Not a delivery system. A file you hand off yourself.

use anyhow::{anyhow, bail, Context, Result};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use x25519_dalek::{PublicKey as XPublic, StaticSecret};

const MAGIC: &[u8; 8] = b"RATDROP1";
const VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropMeta {
    pub recipient: String,
    pub kind: String,
    pub name: String,
}

fn x25519_sk_from_ed_secret(secret: &[u8; 32]) -> StaticSecret {
    let sk = SigningKey::from_bytes(secret);
    StaticSecret::from(sk.to_scalar_bytes())
}

fn x25519_pk_from_endpoint(endpoint_id: &[u8; 32]) -> Result<XPublic> {
    let vk = VerifyingKey::from_bytes(endpoint_id).map_err(|e| anyhow!("bad peer key: {e}"))?;
    Ok(XPublic::from(vk.to_montgomery().to_bytes()))
}

fn shared_key(sk: &StaticSecret, pk: &XPublic) -> [u8; 32] {
    let shared = sk.diffie_hellman(pk);
    blake3::derive_key("ratline-dead-drop-v1", shared.as_bytes())
}

/// Seal plaintext bytes for `recipient_endpoint` (32-byte public id).
pub fn seal(
    sender_secret: &[u8; 32],
    recipient_endpoint: &[u8; 32],
    kind: &str,
    name: &str,
    body: &[u8],
) -> Result<Vec<u8>> {
    let recip_pk = x25519_pk_from_endpoint(recipient_endpoint)?;
    let eph = StaticSecret::random_from_rng(OsRng);
    let eph_pk = XPublic::from(&eph);
    let key = shared_key(&eph, &recip_pk);
    let cipher = ChaCha20Poly1305::new((&key).into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let payload = DropPayload {
        kind: kind.into(),
        name: name.into(),
        body: body.to_vec(),
        sender: hex::encode({
            let sk = SigningKey::from_bytes(sender_secret);
            sk.verifying_key().to_bytes()
        }),
    };
    let plain = serde_json::to_vec(&payload)?;
    let ct = cipher
        .encrypt(&nonce, plain.as_ref())
        .map_err(|e| anyhow!("seal: {e}"))?;

    let mut out = Vec::with_capacity(8 + 1 + 32 + 32 + 12 + ct.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(recipient_endpoint);
    out.extend_from_slice(eph_pk.as_bytes());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    let _ = sender_secret; // used above via SigningKey
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct DropPayload {
    kind: String,
    name: String,
    body: Vec<u8>,
    sender: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenedDrop {
    pub kind: String,
    pub name: String,
    pub body_text: Option<String>,
    pub body_bytes: Vec<u8>,
    pub sender: String,
    pub recipient: String,
}

/// Open a sealed file if it is addressed to our endpoint id.
pub fn open(our_secret: &[u8; 32], our_endpoint: &[u8; 32], data: &[u8]) -> Result<OpenedDrop> {
    if data.len() < 8 + 1 + 32 + 32 + 12 + 16 {
        bail!("not a ratline drop (too short)");
    }
    if &data[0..8] != MAGIC {
        bail!("not a ratline drop");
    }
    if data[8] != VERSION {
        bail!("unsupported drop version");
    }
    let recip: [u8; 32] = data[9..41].try_into().unwrap();
    if &recip != our_endpoint {
        bail!("sealed for someone else");
    }
    let eph_pk = XPublic::from(<[u8; 32]>::try_from(&data[41..73]).unwrap());
    let nonce = Nonce::from_slice(&data[73..85]);
    let ct = &data[85..];

    let sk = x25519_sk_from_ed_secret(our_secret);
    let key = shared_key(&sk, &eph_pk);
    let cipher = ChaCha20Poly1305::new((&key).into());
    let plain = cipher
        .decrypt(nonce, ct)
        .map_err(|_| anyhow!("could not open — wrong key or corrupt file"))?;
    let payload: DropPayload = serde_json::from_slice(&plain)?;
    let body_text = if payload.kind == "text" {
        Some(String::from_utf8_lossy(&payload.body).into_owned())
    } else {
        None
    };
    Ok(OpenedDrop {
        kind: payload.kind,
        name: payload.name,
        body_text,
        body_bytes: payload.body,
        sender: payload.sender,
        recipient: hex::encode(recip),
    })
}

pub fn write_drop_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).context("write drop file")?;
    Ok(())
}

pub fn read_drop_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).context("read drop file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    #[test]
    fn seal_open_roundtrip() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut a);
        rand::thread_rng().fill_bytes(&mut b);
        let a_sk = SigningKey::from_bytes(&a);
        let b_sk = SigningKey::from_bytes(&b);
        let b_id = b_sk.verifying_key().to_bytes();

        let sealed = seal(&a, &b_id, "text", "note", b"meet at dusk").unwrap();
        let opened = open(&b, &b_id, &sealed).unwrap();
        assert_eq!(opened.body_text.as_deref(), Some("meet at dusk"));
        assert_eq!(opened.sender, hex::encode(a_sk.verifying_key().to_bytes()));

        let a_id = a_sk.verifying_key().to_bytes();
        assert!(open(&a, &a_id, &sealed).is_err());
    }
}
