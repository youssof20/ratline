//! Local long-term identity (iroh SecretKey / EndpointId).

use anyhow::{Context, Result};
use iroh::SecretKey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Identity {
    pub secret: SecretKey,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    /// Hex-encoded 32-byte secret.
    secret_hex: String,
}

impl Identity {
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)?;
        let path = data_dir.join("identity.json");
        if path.exists() {
            let raw = fs::read_to_string(&path).context("read identity")?;
            let stored: StoredIdentity = serde_json::from_str(&raw)?;
            let bytes = hex::decode(stored.secret_hex.trim()).context("decode identity")?;
            if bytes.len() != 32 {
                anyhow::bail!("identity key wrong length");
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let secret = SecretKey::from_bytes(&arr);
            Ok(Self { secret, path })
        } else {
            let secret = SecretKey::generate();
            let stored = StoredIdentity {
                secret_hex: hex::encode(secret.to_bytes()),
            };
            let json = serde_json::to_string_pretty(&stored)?;
            // Restrictive perms best-effort on Unix; Windows ACLs left to user profile.
            fs::write(&path, json).context("write identity")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path)?.permissions();
                perms.set_mode(0o600);
                fs::set_permissions(&path, perms)?;
            }
            Ok(Self { secret, path })
        }
    }

    pub fn endpoint_id_bytes(&self) -> [u8; 32] {
        let pk = self.secret.public();
        let mut out = [0u8; 32];
        out.copy_from_slice(pk.as_bytes());
        out
    }

    pub fn endpoint_id_hex(&self) -> String {
        hex::encode(self.endpoint_id_bytes())
    }

    /// Export secret bytes for endpoint builder (caller should zeroize when done).
    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.secret.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persists_across_load() {
        let dir = tempdir().unwrap();
        let a = Identity::load_or_create(dir.path()).unwrap();
        let id = a.endpoint_id_hex();
        let b = Identity::load_or_create(dir.path()).unwrap();
        assert_eq!(id, b.endpoint_id_hex());
    }
}
