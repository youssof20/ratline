//! Local history. Message bodies encrypted at rest with ChaCha20-Poly1305.

use anyhow::{anyhow, Context, Result};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPeer {
    pub endpoint_id: String,
    pub label: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRoom {
    pub topic_id: String,
    pub label: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub body: String,
    pub kind: String,
    pub created_at: i64,
}

pub struct Storage {
    conn: Mutex<Connection>,
    cipher: ChaCha20Poly1305,
    path: PathBuf,
}

impl Storage {
    pub fn open(data_dir: &Path, db_key: &[u8; 32]) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("history.db");
        let conn = Connection::open(&path).context("open sqlite")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS peers (
                endpoint_id TEXT PRIMARY KEY,
                label TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rooms (
                topic_id TEXT PRIMARY KEY,
                label TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                body_ct BLOB NOT NULL,
                kind TEXT NOT NULL DEFAULT 'text',
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_conv
                ON messages(conversation_id, created_at);
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        {
            let mut stmt = conn.prepare(
                "INSERT OR IGNORE INTO settings(key, value) VALUES('history_enabled', '0')",
            )?;
            stmt.execute([])?;
        }
        let cipher = ChaCha20Poly1305::new(db_key.into());
        Ok(Self {
            conn: Mutex::new(conn),
            cipher,
            path,
        })
    }

    fn seal(&self, plaintext: &str) -> Result<Vec<u8>> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        let ct = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("encrypt: {e}"))?;
        out.extend_from_slice(&ct);
        Ok(out)
    }

    fn open_body(&self, blob: &[u8]) -> Result<String> {
        if blob.len() < 12 {
            bail_short()?;
        }
        let (n, ct) = blob.split_at(12);
        let nonce = Nonce::from_slice(n);
        let pt = self
            .cipher
            .decrypt(nonce, ct)
            .map_err(|_| anyhow!("decrypt failed"))?;
        Ok(String::from_utf8(pt)?)
    }

    pub fn history_enabled(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key='history_enabled'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "1".into());
        Ok(v == "1")
    }

    pub fn set_history_enabled(&self, on: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES('history_enabled', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![if on { "1" } else { "0" }],
        )?;
        Ok(())
    }

    pub fn upsert_peer(&self, peer: &StoredPeer) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO peers(endpoint_id, label, created_at) VALUES(?1,?2,?3)
             ON CONFLICT(endpoint_id) DO UPDATE SET label=excluded.label",
            params![peer.endpoint_id, peer.label, peer.created_at],
        )?;
        Ok(())
    }

    pub fn list_peers(&self) -> Result<Vec<StoredPeer>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT endpoint_id, label, created_at FROM peers ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(StoredPeer {
                endpoint_id: r.get(0)?,
                label: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_peer_label(&self, endpoint_id: &str, label: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE peers SET label=?1 WHERE endpoint_id=?2",
            params![label, endpoint_id],
        )?;
        Ok(n > 0)
    }

    pub fn set_room_label(&self, topic_id: &str, label: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE rooms SET label=?1 WHERE topic_id=?2",
            params![label, topic_id],
        )?;
        Ok(n > 0)
    }

    pub fn upsert_room(&self, room: &StoredRoom) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rooms(topic_id, label, created_at) VALUES(?1,?2,?3)
             ON CONFLICT(topic_id) DO UPDATE SET label=excluded.label",
            params![room.topic_id, room.label, room.created_at],
        )?;
        Ok(())
    }

    pub fn list_rooms(&self) -> Result<Vec<StoredRoom>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT topic_id, label, created_at FROM rooms ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(StoredRoom {
                topic_id: r.get(0)?,
                label: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn insert_message(&self, msg: &StoredMessage) -> Result<()> {
        if !self.history_enabled()? {
            return Ok(());
        }
        let ct = self.seal(&msg.body)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO messages(id, conversation_id, sender_id, body_ct, kind, created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                msg.id,
                msg.conversation_id,
                msg.sender_id,
                ct,
                msg.kind,
                msg.created_at
            ],
        )?;
        Ok(())
    }

    pub fn messages_for(&self, conversation_id: &str, limit: i64) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, sender_id, body_ct, kind, created_at FROM messages
             WHERE conversation_id=?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![conversation_id, limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (id, conversation_id, sender_id, body_ct, kind, created_at) = row;
            let body = self.open_body(&body_ct).unwrap_or_else(|_| "<decrypt error>".into());
            out.push(StoredMessage {
                id,
                conversation_id,
                sender_id,
                body,
                kind,
                created_at,
            });
        }
        out.reverse();
        Ok(out)
    }

    pub fn wipe_conversation(&self, conversation_id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM messages WHERE conversation_id=?1",
            params![conversation_id],
        )?;
        Ok(n)
    }

    pub fn remove_peer(&self, endpoint_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM messages WHERE conversation_id=?1",
            params![endpoint_id],
        )?;
        conn.execute("DELETE FROM peers WHERE endpoint_id=?1", params![endpoint_id])?;
        Ok(())
    }

    pub fn remove_room(&self, topic_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM messages WHERE conversation_id=?1",
            params![topic_id],
        )?;
        conn.execute("DELETE FROM rooms WHERE topic_id=?1", params![topic_id])?;
        Ok(())
    }

    pub fn purge_all(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DELETE FROM messages; DELETE FROM peers; DELETE FROM rooms;",
        )?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn bail_short() -> Result<()> {
    Err(anyhow!("ciphertext too short"))
}

/// Derive encryption key from the long-term identity secret.
pub fn db_key_from_identity(secret_bytes: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("ratline-history-v1", secret_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn history_roundtrip_and_wipe() {
        let dir = tempdir().unwrap();
        let key = [42u8; 32];
        let store = Storage::open(dir.path(), &key).unwrap();
        store
            .upsert_peer(&StoredPeer {
                endpoint_id: "abc".into(),
                label: "bob".into(),
                created_at: 1,
            })
            .unwrap();
        store
            .insert_message(&StoredMessage {
                id: "m1".into(),
                conversation_id: "abc".into(),
                sender_id: "me".into(),
                body: "hello".into(),
                kind: "text".into(),
                created_at: 2,
            })
            .unwrap();
        let msgs = store.messages_for("abc", 10).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].body, "hello");
        store.wipe_conversation("abc").unwrap();
        assert!(store.messages_for("abc", 10).unwrap().is_empty());
    }
}
