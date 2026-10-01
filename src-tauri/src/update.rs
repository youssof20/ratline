//! GitHub release check + download/apply for in-terminal `/update`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

const REPO: &str = "youssof20/ratline";
const USER_AGENT: &str = "ratline-updater";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub notes: String,
    pub asset_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn parse_ver(s: &str) -> (u64, u64, u64) {
    let t = s.trim().trim_start_matches('v');
    let mut parts = t.split('.');
    let major = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let patch = parts
        .next()
        .unwrap_or("0")
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (major, minor, patch)
}

fn cmp_ver(a: &str, b: &str) -> Ordering {
    parse_ver(a).cmp(&parse_ver(b))
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn pick_asset(assets: &[GhAsset]) -> Result<&GhAsset> {
    #[cfg(target_os = "windows")]
    {
        assets
            .iter()
            .find(|a| a.name.ends_with("-setup.exe"))
            .or_else(|| assets.iter().find(|a| a.name.ends_with(".msi")))
            .ok_or_else(|| anyhow!("no Windows installer in release"))
    }
    #[cfg(target_os = "macos")]
    {
        let prefer = if std::env::consts::ARCH == "aarch64" {
            "aarch64.dmg"
        } else {
            "x64.dmg"
        };
        assets
            .iter()
            .find(|a| a.name.ends_with(prefer))
            .or_else(|| assets.iter().find(|a| a.name.ends_with(".dmg")))
            .ok_or_else(|| anyhow!("no macOS dmg in release"))
    }
    #[cfg(target_os = "linux")]
    {
        assets
            .iter()
            .find(|a| a.name.ends_with(".AppImage"))
            .or_else(|| assets.iter().find(|a| a.name.ends_with("_amd64.deb")))
            .ok_or_else(|| anyhow!("no Linux package in release"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = assets;
        bail!("unsupported platform for updates")
    }
}

fn find_sums_asset(assets: &[GhAsset]) -> Result<&GhAsset> {
    assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS") || a.name.ends_with("SHA256SUMS.txt"))
        .ok_or_else(|| anyhow!("release has no SHA256SUMS — refusing update"))
}

async fn fetch_latest() -> Result<GhRelease> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()?;
    let res = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("contact github")?;
    if !res.status().is_success() {
        bail!("github returned {}", res.status());
    }
    res.json::<GhRelease>().await.context("parse release json")
}

fn parse_sha256sums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // "hash  filename" or "hash *filename"
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let name = name.trim_start_matches('*');
        map.insert(name.to_string(), hash.to_lowercase());
    }
    map
}

async fn download_text(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .build()?;
    let res = client.get(url).send().await.context("download sums")?;
    if !res.status().is_success() {
        bail!("checksum download failed: {}", res.status());
    }
    Ok(res.text().await?)
}

fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).context("open for hash")?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn emit_progress(
    app: &AppHandle,
    phase: &str,
    pct: u8,
    from: &str,
    to: &str,
    file: &str,
) {
    let width = 24usize;
    let filled = ((pct as usize) * width) / 100;
    let empty = width.saturating_sub(filled);
    let bar = format!("[{}{}] {pct}%", "█".repeat(filled), "░".repeat(empty));
    let _ = app.emit(
        "update_progress",
        serde_json::json!({
            "phase": phase,
            "pct": pct,
            "bar": bar,
            "from": from,
            "to": to,
            "file": file,
        }),
    );
}

pub async fn check_update() -> Result<UpdateInfo> {
    let current = current_version();
    let rel = fetch_latest().await?;
    let latest_raw = rel.tag_name.trim_start_matches('v').to_string();
    let available = cmp_ver(&latest_raw, &current) == Ordering::Greater;
    let asset = if available {
        pick_asset(&rel.assets).ok().map(|a| a.name.clone())
    } else {
        None
    };
    let notes = rel
        .body
        .unwrap_or_default()
        .lines()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(UpdateInfo {
        current: format!("v{current}"),
        latest: format!("v{latest_raw}"),
        available,
        notes,
        asset_name: asset,
    })
}

pub async fn run_update(app: AppHandle) -> Result<()> {
    let current = current_version();
    let rel = fetch_latest().await?;
    let latest = rel.tag_name.trim_start_matches('v').to_string();
    if cmp_ver(&latest, &current) != Ordering::Greater {
        bail!("already on latest (v{current})");
    }
    let asset = pick_asset(&rel.assets)?;
    let sums_asset = find_sums_asset(&rel.assets)?;
    let sums_text = download_text(&sums_asset.browser_download_url).await?;
    let sums = parse_sha256sums(&sums_text);
    let expected = sums
        .get(&asset.name)
        .cloned()
        .ok_or_else(|| anyhow!("SHA256SUMS has no entry for {}", asset.name))?;

    let url = asset.browser_download_url.clone();
    let name = asset.name.clone();
    let total = asset.size.max(1);
    let from = format!("v{current}");
    let to = format!("v{latest}");

    emit_progress(&app, "start", 0, &from, &to, &name);

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(600))
        .build()?;
    let mut res = client.get(&url).send().await.context("download")?;
    if !res.status().is_success() {
        bail!("download failed: {}", res.status());
    }

    let dir = std::env::temp_dir().join("ratline-update");
    let _ = std::fs::create_dir_all(&dir);
    let dest = dir.join(&name);
    let mut file = File::create(&dest).context("create temp file")?;

    let mut downloaded: u64 = 0;
    let mut last_emit = 255u8;

    while let Some(chunk) = res.chunk().await.context("read chunk")? {
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        let pct = ((downloaded.saturating_mul(100)) / total).min(100) as u8;
        if pct != last_emit {
            last_emit = pct;
            emit_progress(&app, "download", pct, &from, &to, &name);
        }
    }
    file.flush()?;
    drop(file);

    emit_progress(&app, "verify", 100, &from, &to, &name);
    let got = sha256_file(&dest)?;
    if got != expected {
        let _ = std::fs::remove_file(&dest);
        bail!("checksum mismatch — update aborted");
    }

    emit_progress(&app, "install", 100, &from, &to, &name);
    apply_and_restart(&app, &dest)?;
    Ok(())
}

fn apply_and_restart(app: &AppHandle, path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let path_str = path.to_string_lossy().to_string();
        if path_str.ends_with(".msi") {
            std::process::Command::new("msiexec")
                .args(["/i", &path_str])
                .spawn()
                .context("launch msi")?;
        } else {
            std::process::Command::new(&path_str)
                .spawn()
                .context("launch installer")?;
        }
        app.exit(0);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .context("open dmg")?;
        app.exit(0);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.extension().and_then(|e| e.to_str()) == Some("AppImage") {
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms)?;
            if let Ok(exe) = std::env::current_exe() {
                let exe_s = exe.to_string_lossy();
                if exe_s.contains("AppImage") || exe_s.ends_with(".AppImage") {
                    let bak = exe.with_extension("AppImage.bak");
                    let _ = std::fs::rename(&exe, &bak);
                    std::fs::copy(path, &exe)?;
                    let mut p = std::fs::metadata(&exe)?.permissions();
                    p.set_mode(0o755);
                    std::fs::set_permissions(&exe, p)?;
                    std::process::Command::new(&exe).spawn()?;
                    app.exit(0);
                    return Ok(());
                }
            }
            std::process::Command::new(path).spawn()?;
            app.exit(0);
            Ok(())
        } else {
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
            app.exit(0);
            Ok(())
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, path);
        bail!("unsupported platform")
    }
}
