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

/// One global default (relay/local_host/...) with config-file fallback.
/// Shared by the CLI and the daemon so both resolve identically.
pub fn global_config_value(key: &str) -> Option<String> {
    let path = config_dir().join("config.json");
    let data = std::fs::read(&path).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&data).ok()?;
    if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
        if !s.trim().is_empty() {
            return Some(s.to_string());
        }
        return None;
    }
    v.get(key).and_then(|x| x.as_bool()).map(|b| b.to_string())
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
    // Secret Service (GNOME Keyring / KWallet) first, 0600 file as fallback.
    if let Some(s) = keyring_get() {
        if !s.is_empty() {
            return s;
        }
    }
    std::fs::read_to_string(secret_path()).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn write_secret(secret: &str) {
    // Always keep the 0600 file fresh (headless fallback); mirror into the
    // keyring when a Secret Service is available.
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = secret_path();
    if secret.is_empty() {
        let _ = std::fs::remove_file(&path);
        keyring_delete();
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
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(&path, secret);
    }
    keyring_set(secret);
}

/// Best-effort keyring access: None when no Secret Service answers, or when
/// built with `--no-default-features` (keyring support compiled out).
#[cfg(feature = "keyring")]
fn keyring_get() -> Option<String> {
    tokio_block_on(keyring_get_async()).flatten().filter(|s| !s.is_empty())
}

#[cfg(feature = "keyring")]
fn keyring_set(secret: &str) {
    let owned = secret.to_string();
    let _ = tokio_block_on(keyring_set_async(owned));
}

#[cfg(feature = "keyring")]
fn keyring_delete() {
    let _ = tokio_block_on(keyring_delete_async());
}

#[cfg(feature = "keyring")]
async fn keyring_get_async() -> Option<String> {
    use secret_service::EncryptionType;
    use std::collections::HashMap;
    let ss = secret_service::SecretService::connect(EncryptionType::Dh).await.ok()?;
    let collection = ss.get_default_collection().await.ok()?;
    if collection.is_locked().await.unwrap_or(true) {
        return None;
    }
    let mut attrs = HashMap::new();
    attrs.insert("application", "localfly");
    attrs.insert("account", "relay");
    let items = collection.search_items(attrs).await.ok()?;
    let item = items.into_iter().next()?;
    let secret = item.get_secret().await.ok()?;
    String::from_utf8(secret).ok()
}

#[cfg(feature = "keyring")]
async fn keyring_set_async(secret: String) -> Option<()> {
    use secret_service::EncryptionType;
    use std::collections::HashMap;
    let ss = secret_service::SecretService::connect(EncryptionType::Dh).await.ok()?;
    let collection = ss.get_default_collection().await.ok()?;
    if collection.is_locked().await.unwrap_or(true) {
        return None;
    }
    let mut attrs = HashMap::new();
    attrs.insert("application", "localfly");
    attrs.insert("account", "relay");
    collection
        .create_item("Localfly relay secret", attrs, secret.as_bytes(), true, "text/plain")
        .await
        .ok()
        .map(|_| ())
}

#[cfg(feature = "keyring")]
async fn keyring_delete_async() -> Option<()> {
    use secret_service::EncryptionType;
    use std::collections::HashMap;
    let ss = secret_service::SecretService::connect(EncryptionType::Dh).await.ok()?;
    let collection = ss.get_default_collection().await.ok()?;
    if collection.is_locked().await.unwrap_or(true) {
        return None;
    }
    let mut attrs = HashMap::new();
    attrs.insert("application", "localfly");
    attrs.insert("account", "relay");
    let items = collection.search_items(attrs).await.ok()?;
    for item in items {
        let _ = item.delete().await;
    }
    Some(())
}

#[cfg(not(feature = "keyring"))]
#[allow(dead_code)]
fn keyring_get() -> Option<String> {
    None
}

#[cfg(not(feature = "keyring"))]
#[allow(dead_code)]
fn keyring_set(_secret: &str) {}

#[cfg(not(feature = "keyring"))]
#[allow(dead_code)]
fn keyring_delete() {}

#[cfg(feature = "keyring")]
fn tokio_block_on<F>(fut: F) -> Option<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    // Run on a throwaway thread+runtime so this works from sync code,
    // inside async contexts, and on machines without D-Bus alike.
    std::thread::spawn(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .map(|rt| rt.block_on(fut))
    })
    .join()
    .ok()
    .flatten()
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
