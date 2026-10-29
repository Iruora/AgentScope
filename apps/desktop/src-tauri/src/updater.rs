//! In-app updates from GitHub Releases. The release workflow publishes a
//! signed `latest.json`; the public key in `tauri.conf.json` verifies it.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;

use crate::Runtime;

/// The first check waits a little so it does not compete with the backfill.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
/// The app usually stays open in the tray for days.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub const RELEASES_URL: &str = "https://github.com/Iruora/AgentScope/releases/latest";

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    /// `false` for copies the updater cannot replace, such as the portable
    /// Windows zip; those link to the release page instead.
    pub can_install: bool,
}

/// Emitted when a background check finds a new version.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAvailable {
    pub version: String,
}

/// The update found by the last check, kept so installing does not check again.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

impl PendingUpdate {
    fn get(&self) -> Option<Update> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, update: Option<Update>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = update;
    }
}

/// Builds from the release zips and plain `cargo` builds have no installer to
/// update; the portable Windows zip marks itself with `portable.txt`.
fn can_install() -> bool {
    if tauri::utils::platform::bundle_type().is_none() {
        return false;
    }
    let portable = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("portable.txt").exists()));
    !portable.unwrap_or(false)
}

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone(),
        can_install: can_install(),
    }
}

/// Asks GitHub for the latest release and remembers it if it is newer.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let handle = app.clone();
    let update = app
        .updater_builder()
        // On Windows the installer takes over without a normal exit.
        .on_before_exit(move || {
            handle.cleanup_before_exit();
            crate::remove_ingest_info();
        })
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("Could not check GitHub for updates: {e}"))?;
    let found = update.as_ref().map(info);
    app.state::<PendingUpdate>().set(update);
    Ok(found)
}

pub fn pending(app: &AppHandle) -> Option<UpdateInfo> {
    app.state::<PendingUpdate>().get().as_ref().map(info)
}

/// Downloads and installs the pending update, then restarts into it.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = app
        .state::<PendingUpdate>()
        .get()
        .ok_or("There is no update to install.")?;
    if !can_install() {
        return Err(
            "This copy of AgentScope cannot update itself. Download the new version instead."
                .into(),
        );
    }
    tracing::info!(version = %update.version, "installing update");
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

/// Checks on launch and then every few hours, while the setting is on, and
/// tells the user once per version.
pub fn spawn_checker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        let mut announced: Option<String> = None;
        loop {
            if app.state::<Runtime>().settings().check_updates {
                match check(&app).await {
                    Ok(Some(found)) if announced.as_deref() != Some(&found.version) => {
                        tracing::info!(version = %found.version, "update available");
                        announce(&app, &found);
                        announced = Some(found.version);
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, "update check failed"),
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn announce(app: &AppHandle, found: &UpdateInfo) {
    let event = UpdateAvailable {
        version: found.version.clone(),
    };
    if let Err(err) = event.emit(app) {
        tracing::warn!(%err, "failed to emit update");
    }
    let body = if found.can_install {
        "Open AgentScope to install it."
    } else {
        "Open AgentScope to download it."
    };
    if let Err(err) = app
        .notification()
        .builder()
        .title(format!("AgentScope {} is available", found.version))
        .body(body)
        .show()
    {
        tracing::warn!(%err, "failed to show notification");
    }
}
