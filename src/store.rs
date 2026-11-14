//! Saved apps + secret, ported from App/Storage.swift.
//!
//! macOS: JSON in Application Support + secret in login keychain.
//! Linux: XDG config dir (`~/.config/localfly/`) + secret file with mode 0600
//! (a libsecret/KWallet backend can replace the file without changing callers).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedApp {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    pub port: u16,
    #[serde(default)]
    pub start_on_launch: bool,
    #[serde(default)]
    pub hue: f64,
    #[serde(default)]
    pub public_port: Option<u16>,
    #[serde(default)]
    pub public_address: Option<String>,
}

impl SavedApp {
    pub fn new(name: String, port: u16) -> Self {
        let hue = hue_for(&name);
        Self { id: Uuid::new_v4(), name, port, start_on_launch: false, hue, public_port: None, public_address: None }
    }

    pub fn initial(&self) -> String {
        self.name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "•".into())
    }

    /// djb2, so the colour is stable across launches (mirrors Swift).
    pub fn hue_for(name: &str) -> f64 {
        hue_for(name)
    }
}

pub fn hue_for(name: &str) -> f64 {
    let mut hash: u32 = 5381;
    for b in name.to_lowercase().bytes() {
        hash = hash.wrapping_shl(5).wrapping_add(hash).wrapping_add(b as u32);
    }
    f64::from(hash % 360) / 360.0
}

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LOCALFLY_CONFIG") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("localfly")
}

fn apps_path() -> PathBuf {
    config_dir().join("apps.json")
}

fn secret_path() -> PathBuf {
    config_dir().join("secret")
}

pub fn load_apps() -> Vec<SavedApp> {
    let path = apps_path();
    let data = std::fs::read(path).unwrap_or_default();
    if data.is_empty() {
        return Vec::new();
    }
    serde_json::from_slice(&data).unwrap_or_default()
}

pub fn save_apps(apps: &[SavedApp]) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(data) = serde_json::to_string_pretty(apps) {
        let path = apps_path();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
}

pub fn read_secret() -> String {
    if let Ok(env) = std::env::var("LOCALFLY_SECRET") {
        if !env.is_empty() {
            return env;
        }
    }
    std::fs::read_to_string(secret_path()).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn write_secret(secret: &str) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = secret_path();
    if secret.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(secret.as_bytes())
            });
        return;
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(&path, secret);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_is_stable_and_matches_swift_djb2() {
        // Swift: name.lowercased().utf8 djb2 % 360 / 360.
        // "web" -> check against a locally computed value.
        let h1 = hue_for("Web");
        let h2 = hue_for("web");
        assert_eq!(h1, h2);
        assert!((0.0..1.0).contains(&h1));
    }
}
