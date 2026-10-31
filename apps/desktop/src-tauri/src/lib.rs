mod commands;
mod decide;
mod diagnostics;
mod engine;
mod focus;
mod hotkey;
mod logbuf;
mod popup;
mod procs;
mod push;
mod risky;
mod settings;
mod shim;
mod tray;
mod updater;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use agentscope_core::ingest::{drain_spool, DecideRequest, IngestServer};
use agentscope_core::AgentState;
use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_specta::{collect_commands, collect_events, Event};
use tokio::sync::mpsc;

use crate::commands::{GraphChanged, OpenSession};
use crate::engine::{Alert, Engine, Outcome};
use crate::settings::Settings;

/// How often process liveness and scans run (spec section 3.1).
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);
/// Old history is also dropped while the app stays open for days.
const PRUNE_INTERVAL: Duration = Duration::from_secs(3600);

/// Transcripts are rescanned this often in case the file watcher missed a change.
const RESCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Coalesces bursts of file events into one read per file.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

/// A session is notified about the same state at most once in this window.
const NOTIFY_DEBOUNCE: Duration = Duration::from_secs(60);

/// Process-wide state that is not part of the agent graph.
pub struct Runtime {
    pub data_dir: PathBuf,
    ingest_port: Mutex<Option<u16>>,
    settings: Mutex<Settings>,
    notified: Mutex<HashMap<String, (AgentState, Instant)>>,
    tray_menu: Mutex<tray::MenuKey>,
}

impl Runtime {
    pub fn ingest_port(&self) -> Option<u16> {
        *self.ingest_port.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::get_snapshot,
            commands::get_app_info,
            commands::get_session_events,
            commands::get_integrations,
            commands::set_hooks,
            commands::go_to_agent,
            commands::reveal_transcript,
            commands::get_settings,
            commands::set_settings,
            commands::get_update,
            commands::check_for_update,
            commands::install_update,
            commands::open_release_page,
            commands::get_diagnostics,
            commands::copy_debug_report,
            commands::open_new_issue,
            commands::get_answerable,
            commands::answer_approval,
            commands::release_approval,
            commands::show_session,
            commands::close_popup,
            commands::test_push,
        ])
        .events(collect_events![
            GraphChanged,
            OpenSession,
            updater::UpdateAvailable
        ])
}

#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) {
    builder
        .export(
            specta_typescript::Typescript::default()
                .header("// @ts-nocheck\n// Regenerate with `cargo test -p agentscope-desktop`."),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts"),
        )
        .expect("failed to export TypeScript bindings");
}

pub fn run() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,agentscope=debug".into()),
            ),
        )
        // Warnings and errors are also kept for Settings → Diagnostics.
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(logbuf::RecentLogs)
                .with_filter(tracing_subscriber::filter::LevelFilter::WARN),
        )
        .init();

    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            setup(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps AgentScope running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building AgentScope");

    app.run(|app, event| match event {
        RunEvent::Exit => {
            // Shims spool from now on instead of posting to a dead port.
            let data_dir = &app.state::<Runtime>().data_dir;
            let _ = std::fs::remove_file(agentscope_protocol::ingest_info_path(data_dir));
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(app),
        _ => {}
    });
}

/// Shims spool from now on instead of posting to a dead port.
pub(crate) fn remove_ingest_info() {
    if let Some(data_dir) = agentscope_protocol::data_dir() {
        let _ = std::fs::remove_file(agentscope_protocol::ingest_info_path(&data_dir));
    }
}

fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = agentscope_protocol::data_dir().ok_or("could not find a data directory")?;
    std::fs::create_dir_all(&data_dir)?;
    let settings = settings::load(&data_dir);
    let shortcut = settings.global_shortcut.clone();
    let engine = Arc::new(Engine::open(
        &data_dir.join("agentscope.db"),
        settings.retention_days,
    )?);
    app.manage(engine.clone());
    app.manage(decide::Decisions::default());
    app.manage(updater::PendingUpdate::default());
    app.manage(Runtime {
        settings: Mutex::new(settings),
        data_dir: data_dir.clone(),
        ingest_port: Mutex::new(None),
        notified: Mutex::new(HashMap::new()),
        tray_menu: Mutex::new(tray::MenuKey::default()),
    });

    tray::create(app)?;
    if let Err(err) = hotkey::apply(app, &shortcut) {
        tracing::warn!(%err, "global shortcut not registered");
    }
    publish(app, &engine, Outcome::changed());

    spawn_ingest(app.clone(), engine.clone(), data_dir);
    spawn_transcript_watcher(app.clone(), engine.clone());
    spawn_sweeper(app.clone(), engine);
    updater::spawn_checker(app.clone());
    Ok(())
}

fn spawn_ingest(app: AppHandle, engine: Arc<Engine>, data_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(1024);
        let (decide_tx, decide_rx) = mpsc::channel(64);
        spawn_decide_loop(app.clone(), engine.clone(), decide_rx);
        // Start listening before draining so nothing new lands in the spool meanwhile.
        let _server = match IngestServer::start(&data_dir, tx, decide_tx).await {
            Ok(server) => {
                *app.state::<Runtime>()
                    .ingest_port
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(server.port);
                Some(server)
            }
            Err(err) => {
                tracing::error!(%err, "ingest server failed to start; hooks will spool");
                None
            }
        };
        match drain_spool(&data_dir) {
            Ok(spooled) => {
                let mut outcome = Outcome::default();
                for raw in &spooled {
                    // Spooled events are history: apply them, but do not notify.
                    outcome.changed |= engine.ingest(raw).changed;
                }
                tracing::info!(count = spooled.len(), "drained spool");
                publish(&app, &engine, outcome);
            }
            Err(err) => tracing::error!(%err, "failed to drain spool"),
        }
        while let Some(raw) = rx.recv().await {
            let outcome = engine.ingest(&raw);
            publish(&app, &engine, outcome);
        }
    });
}

/// Applies permission prompts from `--decide` shims and holds them for the
/// user's answer until the decide window runs out.
fn spawn_decide_loop(app: AppHandle, engine: Arc<Engine>, mut rx: mpsc::Receiver<DecideRequest>) {
    tauri::async_runtime::spawn(async move {
        while let Some(DecideRequest { event, reply }) = rx.recv().await {
            let outcome = engine.ingest(&event);
            // Dropping `reply` (no approval came of it) lets the harness ask itself.
            if let [approval_id] = outcome.requested.as_slice() {
                let approval_id = approval_id.clone();
                app.state::<decide::Decisions>()
                    .register(approval_id.clone(), reply);
                let window = app.state::<Runtime>().settings().decide_window();
                let (app, engine) = (app.clone(), engine.clone());
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(window).await;
                    if app.state::<decide::Decisions>().release(&approval_id) {
                        tracing::info!(%approval_id, "no answer in AgentScope; the harness asks");
                        publish(&app, &engine, Outcome::changed());
                    }
                });
            }
            publish(
                &app,
                &engine,
                Outcome {
                    changed: true,
                    ..outcome
                },
            );
        }
    });
}

/// Tails transcripts as they change: a file watcher for speed, plus a
/// periodic rescan for platforms or file systems where events get lost.
fn spawn_transcript_watcher(app: AppHandle, engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("transcripts".into())
        .spawn(move || {
            let started = SystemTime::now();
            publish(&app, &engine, engine.backfill(None));
            tracing::info!(elapsed = ?started.elapsed().unwrap_or_default(), "transcript backfill done");

            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = match notify::recommended_watcher(tx) {
                Ok(w) => Some(w),
                Err(err) => {
                    tracing::warn!(%err, "file watcher unavailable; polling transcripts");
                    None
                }
            };
            let mut watched = HashSet::new();
            let mut last_rescan = SystemTime::now();
            loop {
                // Roots may appear later, e.g. the first time a harness runs.
                if let Some(w) = watcher.as_mut() {
                    for root in engine.transcript_roots() {
                        if !watched.contains(&root.path) && root.path.is_dir() {
                            let mode = if root.recursive {
                                RecursiveMode::Recursive
                            } else {
                                RecursiveMode::NonRecursive
                            };
                            match w.watch(&root.path, mode) {
                                Ok(()) => {
                                    watched.insert(root.path);
                                }
                                Err(err) => tracing::warn!(%err, path = %root.path.display(), "cannot watch"),
                            }
                        }
                    }
                }

                let mut paths = HashSet::new();
                match rx.recv_timeout(RESCAN_INTERVAL) {
                    Ok(first) => {
                        let deadline = Instant::now() + WATCH_DEBOUNCE;
                        let mut next = Some(first);
                        while let Some(event) = next {
                            if let Ok(event) = event {
                                paths.extend(event.paths);
                            }
                            next = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok();
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        watcher = None;
                        std::thread::sleep(RESCAN_INTERVAL);
                    }
                }
                let mut outcome = Outcome::default();
                for path in paths.iter().filter(|p| p.is_file()) {
                    outcome.merge(engine.tail(path));
                }
                if last_rescan.elapsed().unwrap_or_default() >= RESCAN_INTERVAL {
                    let since = last_rescan - RESCAN_INTERVAL;
                    last_rescan = SystemTime::now();
                    outcome.merge(engine.backfill(Some(since)));
                }
                publish(&app, &engine, outcome);
            }
        })
        .expect("failed to start the transcript watcher");
}

fn spawn_sweeper(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_INTERVAL);
        let mut last_prune = Instant::now();
        loop {
            interval.tick().await;
            let engine = engine.clone();
            let swept = tauri::async_runtime::spawn_blocking({
                let engine = engine.clone();
                let prune = last_prune.elapsed() >= PRUNE_INTERVAL;
                if prune {
                    last_prune = Instant::now();
                }
                move || {
                    if prune {
                        if let Err(err) = engine.prune() {
                            tracing::warn!(%err, "pruning history failed");
                        }
                    }
                    engine.sweep()
                }
            })
            .await;
            match swept {
                Ok(outcome) => publish(&app, &engine, outcome),
                Err(err) => tracing::error!(%err, "sweep failed"),
            }
        }
    });
}

pub(crate) fn publish_outcome(app: &AppHandle, engine: &Engine, outcome: Outcome) {
    publish(app, engine, outcome);
}

/// Refreshes the UI after something outside the graph changed, e.g. which
/// prompts can be answered here.
pub(crate) fn publish_changed(app: &AppHandle) {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    publish(app, &engine, Outcome::changed());
}

/// Pushes a change to the UI and tray, and notifies about new alerts.
fn publish(app: &AppHandle, engine: &Engine, outcome: Outcome) {
    if !outcome.alerts.is_empty() {
        notify_user(app, outcome.alerts);
    }
    if !outcome.changed {
        return;
    }
    let snapshot = engine.snapshot();
    let open = snapshot.approvals.iter().map(|a| a.id.clone()).collect();
    let decisions = app.state::<decide::Decisions>();
    decisions.retain_open(&open);
    let answerable = decisions.ids();
    let waiting_count = snapshot.waiting_count;
    let runtime = app.state::<Runtime>();
    let settings = runtime.settings();
    popup::sync(app, &answerable, settings.popup_approvals);
    let key = tray::menu_key(&snapshot, &answerable, settings.confirm_risky);
    let menu_changed = {
        let mut last = runtime.tray_menu.lock().unwrap_or_else(|e| e.into_inner());
        let changed = *last != key;
        *last = key.clone();
        changed
    };
    tray::update(app, waiting_count, menu_changed.then_some(&key));
    if let Err(err) = (GraphChanged { waiting_count }).emit(app) {
        tracing::warn!(%err, "failed to emit graph change");
    }
}

fn notify_user(app: &AppHandle, alerts: Vec<Alert>) {
    let runtime = app.state::<Runtime>();
    let settings = runtime.settings();
    let mut notified = runtime.notified.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    notified.retain(|_, (_, at)| now.duration_since(*at) < NOTIFY_DEBOUNCE);
    for alert in alerts {
        let approval = matches!(
            alert.state,
            AgentState::WaitingApproval | AgentState::MaybeWaiting
        );
        let desktop = if approval {
            settings.notify_approvals
        } else {
            settings.notify_input
        };
        let phone = settings.push.wants(approval);
        if !(desktop || phone)
            || notified
                .get(&alert.session_id)
                .is_some_and(|(state, _)| *state == alert.state)
        {
            continue;
        }
        notified.insert(alert.session_id.clone(), (alert.state, now));
        if phone {
            push::spawn_send(
                settings.push.clone(),
                push::PushMessage::new(&settings.push, &alert.title, &alert.body, approval),
            );
        }
        if !desktop {
            continue;
        }
        if let Err(err) = app
            .notification()
            .builder()
            .title(&alert.title)
            .body(&alert.body)
            .show()
        {
            tracing::warn!(%err, "failed to show notification");
        }
    }
}

pub(crate) fn show_main_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Shows the main window on a session's detail page.
pub(crate) fn open_session<R: tauri::Runtime>(app: &AppHandle<R>, session_id: &str) {
    show_main_window(app);
    let event = OpenSession {
        session_id: session_id.to_owned(),
    };
    // The name tauri-specta gives `OpenSession`; `Event::emit` needs a concrete runtime.
    if let Err(err) = app.emit("open-session", event) {
        tracing::warn!(%err, "failed to open session");
    }
}

#[cfg(test)]
mod tests {
    /// Regenerates `src/bindings.ts`. CI fails if the committed file is stale.
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
