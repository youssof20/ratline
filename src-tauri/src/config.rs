//! User prefs stored next to identity — commands only, no settings UI.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Valid global-hotkey name for the backtick key (not "grave").
const DEFAULT_HOTKEY: &str = "ctrl+backquote";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global summon hotkey (global-hotkey crate syntax).
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

/// Map legacy / friendly names to what global-hotkey accepts.
pub fn normalize_hotkey(raw: &str) -> String {
    let s = raw.trim().to_lowercase().replace(' ', "");
    if s.is_empty() {
        return DEFAULT_HOTKEY.into();
    }
    // old default that panics the plugin: "ctrl+grave"
    let s = s
        .replace("+grave", "+backquote")
        .replace("grave+", "backquote+");
    if s == "grave" {
        return "backquote".into();
    }
    // allow ctrl+` literally
    s.replace("+`", "+backquote").replace("`+", "backquote+")
}

impl Config {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.json")
    }

    pub fn load(data_dir: &Path) -> Self {
        let path = Self::path(data_dir);
        let mut cfg = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        let normalized = normalize_hotkey(&cfg.hotkey);
        if normalized != cfg.hotkey {
            cfg.hotkey = normalized;
            let _ = cfg.save(data_dir);
        }
        cfg
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_grave_to_backquote() {
        assert_eq!(normalize_hotkey("ctrl+grave"), "ctrl+backquote");
        assert_eq!(normalize_hotkey("Ctrl + Grave"), "ctrl+backquote");
        assert_eq!(normalize_hotkey("ctrl+`"), "ctrl+backquote");
    }
}
