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
use crate::updater::UpdateInfo;
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
    /// The harness takes answers to permission prompts from a hook.
    pub can_answer: bool,
    /// The installed hooks let AgentScope answer its permission prompts.
    pub answers: bool,
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
        .map(|detection| {
            let adapter = engine.adapter(detection.harness);
            Integration {
                hook_status: shim.as_deref().map_or(HookStatus::NotInstalled, |s| {
                    engine.hooks_status(detection.harness, s)
                }),
                can_answer: adapter.is_some_and(|a| a.can_answer_approvals()),
                answers: adapter.is_some_and(|a| a.answers_approvals()),
                detection,
            }
        })
        .collect();
    Integrations {
        harnesses,
        shim_path: shim.map(|p| p.display().to_string()),
    }
}

/// Connects (`connect: true`) or disconnects a harness. With `dry_run` the
/// config is left alone and only the diff is returned, for the preview.
/// `answer_approvals` installs the hook that lets AgentScope answer
/// permission prompts, where the harness supports it.
#[tauri::command]
#[specta::specta]
pub fn set_hooks(
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
    harness: HarnessId,
    connect: bool,
    dry_run: bool,
    answer_approvals: bool,
) -> Result<Diff, String> {
    let shim = shim::path(&runtime.data_dir).ok_or_else(|| {
        "The agentscope-hook helper is missing from this build. Reinstall AgentScope.".to_owned()
    })?;
    let diff = engine
        .set_hooks(harness, &shim, connect, dry_run, answer_approvals)
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
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
    settings: Settings,
) -> Result<(), String> {
    if !crate::settings::DECIDE_WINDOWS.contains(&settings.decide_window_secs) {
        return Err(format!(
            "{} seconds is not one of the offered waits",
            settings.decide_window_secs
        ));
    }
    if settings.global_shortcut != runtime.settings().global_shortcut {
        crate::hotkey::apply(&app, &settings.global_shortcut)?;
    }
    crate::settings::save(&runtime.data_dir, &settings).map_err(|e| e.to_string())?;
    if settings.retention_days != runtime.settings().retention_days {
        engine
            .set_retention(settings.retention_days)
            .map_err(|e| e.to_string())?;
    }
    *runtime.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
    Ok(())
}

/// The update found by the last check, without asking GitHub again.
#[tauri::command]
#[specta::specta]
pub fn get_update(app: AppHandle) -> Option<UpdateInfo> {
    crate::updater::pending(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    crate::updater::check(&app).await
}

/// Installs the pending update and restarts. Only returns on failure.
#[tauri::command]
#[specta::specta]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    crate::updater::install(&app).await
}

#[tauri::command]
#[specta::specta]
pub fn open_release_page() -> Result<(), String> {
    tauri_plugin_opener::open_url(crate::updater::RELEASES_URL, None::<&str>)
        .map_err(|e| e.to_string())
}

fn diagnostics(engine: &Engine, runtime: &Runtime) -> crate::diagnostics::Diagnostics {
    let shim = shim::path(&runtime.data_dir);
    let snapshot = engine.snapshot();
    let hook_status = |harness: HarnessId| {
        shim.as_deref().map_or(HookStatus::NotInstalled, |s| {
            engine.hooks_status(harness, s)
        })
    };
    crate::diagnostics::collect(crate::diagnostics::Inputs {
        detections: engine.detections(),
        hook_status: &hook_status,
        signals: engine.signals(),
        snapshot: &snapshot,
        data_dir: &runtime.data_dir,
        ingest_port: runtime.ingest_port(),
        shim_path: shim.as_ref().map(|p| p.display().to_string()),
        recent_logs: crate::logbuf::recent(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn get_diagnostics(
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
) -> crate::diagnostics::Diagnostics {
    diagnostics(&engine, &runtime)
}

/// Puts the redacted plain-text debug report on the clipboard and returns it.
#[tauri::command]
#[specta::specta]
pub fn copy_debug_report(
    app: AppHandle,
    engine: State<'_, Arc<Engine>>,
    runtime: State<'_, Runtime>,
) -> Result<String, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let report = crate::diagnostics::report(&diagnostics(&engine, &runtime));
    app.clipboard()
        .write_text(report.clone())
        .map_err(|e| e.to_string())?;
    Ok(report)
}

/// Where "Report a problem" sends people, with the debug report on the clipboard.
const NEW_ISSUE_URL: &str = "https://github.com/Iruora/AgentScope/issues/new";

#[tauri::command]
#[specta::specta]
pub fn open_new_issue() -> Result<(), String> {
    tauri_plugin_opener::open_url(NEW_ISSUE_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Approvals waiting for an answer in AgentScope right now.
#[tauri::command]
#[specta::specta]
pub fn get_answerable(decisions: State<'_, crate::decide::Decisions>) -> Vec<String> {
    decisions.ids()
}

/// Answers a waiting permission prompt. The agent goes on (or stops) at once.
#[tauri::command]
#[specta::specta]
pub fn answer_approval(app: AppHandle, approval_id: String, allow: bool) -> Result<(), String> {
    answer(&app, &approval_id, allow)
}

/// Sends the user's answer to a waiting prompt and marks it resolved.
pub(crate) fn answer(app: &AppHandle, approval_id: &str, allow: bool) -> Result<(), String> {
    use agentscope_core::{Decision, Resolution};
    let decision = if allow {
        Decision::Allow
    } else {
        Decision::Deny {
            message: crate::decide::DENY_MESSAGE.to_owned(),
        }
    };
    if !app
        .state::<crate::decide::Decisions>()
        .answer(approval_id, decision)
    {
        crate::publish_changed(app);
        return Err("This prompt is no longer waiting here. Answer it in the terminal.".to_owned());
    }
    tracing::info!(%approval_id, allow, "answered a permission prompt");
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let resolution = if allow {
        Resolution::Approved
    } else {
        Resolution::Denied
    };
    let mut outcome = engine.resolve_approval(approval_id, resolution);
    outcome.changed = true;
    crate::publish_outcome(app, &engine, outcome);
    Ok(())
}

/// Hands a waiting prompt back to the harness, which then asks in the terminal.
#[tauri::command]
#[specta::specta]
pub fn release_approval(app: AppHandle, approval_id: String) {
    if app
        .state::<crate::decide::Decisions>()
        .release(&approval_id)
    {
        crate::publish_changed(&app);
    }
}

/// Shows the main window on a session, e.g. from the approval popup.
#[tauri::command]
#[specta::specta]
pub fn show_session(app: AppHandle, session_id: String) {
    crate::open_session(&app, &session_id);
}

/// Closes the approval popup until a new prompt arrives.
#[tauri::command]
#[specta::specta]
pub fn close_popup(app: AppHandle) {
    crate::popup::close(&app);
}
