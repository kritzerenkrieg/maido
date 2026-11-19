//! Updates (M3). A release build carries the updater's endpoint and public
//! key (`release.conf.json`, written by the release workflow); a build
//! without them never checks. The page decides when to check. Every update
//! is verified against the public key before anything is run. An AppImage,
//! the Windows installer's install and the macOS app replace themselves and
//! restart; a `.deb` (or `.rpm`) is only told where the new package is, so
//! maido never asks for a root password.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;
use tauri::utils::config::BundleType;
use tauri::utils::platform::bundle_type;
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

pub struct Updates {
    /// The build has an updater configuration.
    enabled: bool,
    /// The update the page was last told about.
    pending: Mutex<Option<Update>>,
    /// That update's package, once downloaded (its version, its bytes).
    downloaded: Mutex<Option<(String, Vec<u8>)>>,
    /// Windows with unsaved edits: installing restarts every window.
    unsaved: Mutex<HashSet<String>>,
}

impl Updates {
    pub fn new(enabled: bool) -> Updates {
        Updates { enabled, pending: Mutex::new(None), downloaded: Mutex::new(None), unsaved: Mutex::new(HashSet::new()) }
    }

    /// A window closed: its edits are gone with it.
    pub fn closed(&self, label: &str) {
        self.unsaved.lock().unwrap().remove(label);
    }

    fn check_saved(&self) -> Result<(), String> {
        if self.unsaved.lock().unwrap().is_empty() {
            Ok(())
        } else {
            Err("save or discard the edits in every maido window first: maido restarts to update".into())
        }
    }
}

/// The page tells Rust whether its window has unsaved edits.
#[tauri::command]
pub fn set_unsaved(window: WebviewWindow, updates: State<'_, Updates>, unsaved: bool) {
    let mut set = updates.unsaved.lock().unwrap();
    if unsaved {
        set.insert(window.label().to_string());
    } else {
        set.remove(window.label());
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
/// Every window restarts, so no window may have unsaved edits: checked
/// before the download and again before installing, since editing goes on
/// while it downloads. A download refused that way is kept for the next try.
#[tauri::command]
pub async fn install_update(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    if !installs_itself() {
        return Err("this package is updated by downloading the new one".into());
    }
    updates.check_saved()?;
    let update = updates.pending.lock().unwrap().clone().ok_or("no update to install")?;
    let kept = updates.downloaded.lock().unwrap().take().filter(|(v, _)| *v == update.version);
    let bytes = match kept {
        Some((_, bytes)) => bytes,
        None => update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?,
    };
    if let Err(e) = updates.check_saved() {
        *updates.downloaded.lock().unwrap() = Some((update.version.clone(), bytes));
        return Err(e);
    }
    update.install(&bytes).map_err(|e| e.to_string())?;
    app.restart()
}
