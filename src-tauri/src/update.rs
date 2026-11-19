//! Updates (M3). A release build carries the updater's endpoint and public
//! key (`release.conf.json`, written by the release workflow); a build
//! without them never checks. The page decides when to check. Every update
//! is verified against the public key before anything is run. An AppImage,
//! the Windows installer's install and the macOS app replace themselves and
//! restart; a `.deb` (or `.rpm`) is only told where the new package is, so
//! maido never asks for a root password.

use std::sync::Mutex;

use serde::Serialize;
use tauri::utils::config::BundleType;
use tauri::utils::platform::bundle_type;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

pub struct Updates {
    /// The build has an updater configuration.
    enabled: bool,
    /// The update the page was last told about.
    pending: Mutex<Option<Update>>,
}

impl Updates {
    pub fn new(enabled: bool) -> Updates {
        Updates { enabled, pending: Mutex::new(None) }
    }
}

#[derive(Serialize)]
pub struct Available {
    version: String,
    notes: String,
    /// maido can install it itself (see `installs_itself`); otherwise `url`
    /// is the package to download.
    install: bool,
    url: String,
}

/// Packages maido can replace as the user: not a system package, which needs root.
fn installs_itself() -> bool {
    matches!(bundle_type(), Some(BundleType::AppImage | BundleType::Nsis | BundleType::Msi | BundleType::App))
}

/// Ask the release server for a newer version; `None` when up to date.
#[tauri::command]
pub async fn check_update(app: AppHandle, updates: State<'_, Updates>) -> Result<Option<Available>, String> {
    if !updates.enabled {
        return Err("this build of maido does not check for updates".into());
    }
    let found = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let available = found.as_ref().map(|u| Available {
        version: u.version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        install: installs_itself(),
        url: u.download_url.to_string(),
    });
    *updates.pending.lock().unwrap() = found;
    Ok(available)
}

/// Download the update found by `check_update`, verify it, install it and
/// restart (on Windows the installer closes maido and starts it again).
#[tauri::command]
pub async fn install_update(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    if !installs_itself() {
        return Err("this package is updated by downloading the new one".into());
    }
    let update = updates.pending.lock().unwrap().clone().ok_or("no update to install")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}
