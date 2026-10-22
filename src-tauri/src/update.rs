//! Updates (M3). A release build carries the updater's endpoint and public
//! key (`release.conf.json`); a build without them never checks. The page
//! decides when to check. Every update is verified against the public key
//! before anything is run. An AppImage replaces itself and restarts; a
//! `.deb` is only told where the new package is, so maido never asks for a
//! root password.

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
    /// maido can install it itself (an AppImage); otherwise `url` is the
    /// package to download.
    install: bool,
    url: String,
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
        install: bundle_type() == Some(BundleType::AppImage),
        url: u.download_url.to_string(),
    });
    *updates.pending.lock().unwrap() = found;
    Ok(available)
}

/// Download the update found by `check_update`, verify it, replace the
/// AppImage and restart.
#[tauri::command]
pub async fn install_update(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    if bundle_type() != Some(BundleType::AppImage) {
        return Err("only the AppImage updates itself".into());
    }
    let update = updates.pending.lock().unwrap().clone().ok_or("no update to install")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}
