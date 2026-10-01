//! User prefs stored next to identity — commands only, no settings UI.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_HOTKEY: &str = "ctrl+grave";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global summon hotkey (Tauri / keyboard crate syntax).
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Soft connect / message tones. Off by default.
    #[serde(default)]
    pub sound: bool,
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            sound: false,
        }
    }
}

impl Config {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.json")
    }

    pub fn load(data_dir: &Path) -> Self {
        let path = Self::path(data_dir);
        match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        fs::create_dir_all(data_dir)?;
        let path = Self::path(data_dir);
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json).context("write config")?;
        Ok(())
    }
}

pub fn default_hotkey_str() -> &'static str {
    DEFAULT_HOTKEY
}
