//! Wire protocol for 1:1 chat streams and gossip payloads.

use serde::{Deserialize, Serialize};

pub const CHAT_ALPN: &[u8] = b"ratline/chat/1";
pub const PAIR_ALPN: &[u8] = b"ratline/pair/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ChatMsg {
    Text {
        id: String,
        body: String,
        ts: i64,
    },
    FileOffer {
        id: String,
        name: String,
        size: u64,
        hash: String,
        ts: i64,
    },
    Typing {
        active: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum GossipMsg {
    Chat {
        id: String,
        sender: String,
        body: String,
        ts: i64,
    },
    Typing {
        sender: String,
        active: bool,
    },
    FileOffer {
        id: String,
        sender: String,
        name: String,
        size: u64,
        hash: String,
        ts: i64,
    },
    Presence {
        sender: String,
        label: String,
    },
}

pub fn encode_chat(msg: &ChatMsg) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(msg)
}

pub fn decode_chat(bytes: &[u8]) -> Result<ChatMsg, serde_json::Error> {
    serde_json::from_slice(bytes)
}

pub fn encode_gossip(msg: &GossipMsg) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(msg)
}

pub fn decode_gossip(bytes: &[u8]) -> Result<GossipMsg, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Length-prefixed write for chat streams.
pub async fn write_lp<W: tokio::io::AsyncWriteExt + Unpin>(
    w: &mut W,
    payload: &[u8],
) -> std::io::Result<()> {
    w.write_all(&(payload.len() as u32).to_be_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await
}

pub async fn read_lp<R: tokio::io::AsyncReadExt + Unpin>(
    r: &mut R,
    max: usize,
) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "payload too large",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_json_roundtrip() {
        let m = ChatMsg::Text {
            id: "1".into(),
            body: "hi".into(),
            ts: 99,
        };
        let b = encode_chat(&m).unwrap();
        match decode_chat(&b).unwrap() {
            ChatMsg::Text { body, .. } => assert_eq!(body, "hi"),
            _ => panic!("wrong variant"),
        }
    }
}
