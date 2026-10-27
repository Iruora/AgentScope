//! IPC commands. Types are exported to `src/bindings.ts` by tauri-specta.

use std::sync::Arc;

use agentscope_adapters::{Detection, Diff, HookStatus};
use agentscope_core::{AgentEvent, HarnessId, Snapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::engine::Engine;
use crate::focus::GoToAgent;
use crate::settings::Settings;
use crate::{shim, Runtime};

/// How many events the session timeline shows.
const TIMELINE_LIMIT: u32 = 500;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    /// `None` until the ingest server is up, or if it failed to start.
    pub ingest_port: Option<u16>,
    pub harnesses: Vec<Detection>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    pub detection: Detection,
    pub hook_status: HookStatus,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    pub harnesses: Vec<Integration>,
    /// Where the bundled `agentscope-hook` is, if it is usable.
    pub shim_path: Option<String>,
}

/// Emitted whenever the graph changes. The UI refetches the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct GraphChanged {
    pub waiting_count: u32,
}

/// Emitted when the tray or a notification asks to show a session.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct OpenSession {
    pub session_id: String,
}

#[tauri::command]
#[specta::specta]
pub fn get_snapshot(engine: State<'_, Arc<Engine>>) -> Snapshot {
    engine.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn get_app_info(engine: State<'_, Arc<Engine>>, runtime: State<'_, Runtime>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        data_dir: runtime.data_dir.display().to_string(),
        ingest_port: runtime.ingest_port(),
        harnesses: engine.detections(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_session_events(
    engine: State<'_, Arc<Engine>>,
    session_id: String,
) -> Result<Vec<AgentEvent>, String> {
    engine
        .session_events(&session_id, TIMELINE_LIMIT)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_integrations(
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
) -> Integrations {
    let shim = shim::path(&runtime.data_dir);
    let harnesses = engine
        .detections()
        .into_iter()
        .map(|detection| Integration {
            hook_status: shim.as_deref().map_or(HookStatus::NotInstalled, |s| {
                engine.hooks_status(detection.harness, s)
            }),
            detection,
        })
        .collect();
    Integrations {
        harnesses,
        shim_path: shim.map(|p| p.display().to_string()),
    }
}

/// Connects (`connect: true`) or disconnects a harness. With `dry_run` the
/// config is left alone and only the diff is returned, for the preview.
#[tauri::command]
#[specta::specta]
pub fn set_hooks(
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
    harness: HarnessId,
    connect: bool,
    dry_run: bool,
) -> Result<Diff, String> {
    let shim = shim::path(&runtime.data_dir).ok_or_else(|| {
        "The agentscope-hook helper is missing from this build. Reinstall AgentScope.".to_owned()
    })?;
    let diff = engine
        .set_hooks(harness, &shim, connect, dry_run)
        .map_err(|e| e.to_string())?;
    if !dry_run {
        tracing::info!(%harness, connect, path = %diff.path, "updated hook config");
    }
    Ok(diff)
}

#[tauri::command]
#[specta::specta]
pub async fn go_to_agent(app: AppHandle, session_id: String) -> Result<GoToAgent, String> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let location = engine
            .locate(&session_id)
            .ok_or_else(|| "This session is no longer known.".to_owned())?;
        Ok(crate::focus::go_to(&location))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn reveal_transcript(engine: State<'_, Arc<Engine>>, session_id: String) -> Result<(), String> {
    let path = engine
        .session(&session_id)
        .and_then(|s| s.transcript_path)
        .ok_or_else(|| "This session has no transcript file.".to_owned())?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(runtime: State<'_, Runtime>) -> Settings {
    runtime.settings()
}

#[tauri::command]
#[specta::specta]
pub fn set_settings(
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
    settings: Settings,
) -> Result<(), String> {
    crate::settings::save(&runtime.data_dir, &settings).map_err(|e| e.to_string())?;
    if settings.retention_days != runtime.settings().retention_days {
        engine
            .set_retention(settings.retention_days)
            .map_err(|e| e.to_string())?;
    }
    *runtime.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
    Ok(())
}
