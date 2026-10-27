mod commands;
mod engine;
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agentscope_core::ingest::{drain_spool, IngestServer};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_specta::{collect_commands, collect_events, Event};
use tokio::sync::mpsc;

use crate::commands::GraphChanged;
use crate::engine::Engine;

/// Process-wide state that is not part of the agent graph.
pub struct Runtime {
    pub data_dir: PathBuf,
    ingest_port: Mutex<Option<u16>>,
}

impl Runtime {
    pub fn ingest_port(&self) -> Option<u16> {
        *self.ingest_port.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::get_snapshot,
            commands::get_app_info
        ])
        .events(collect_events![GraphChanged])
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,agentscope=debug".into()),
        )
        .init();

    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
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

fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = agentscope_protocol::data_dir().ok_or("could not find a data directory")?;
    std::fs::create_dir_all(&data_dir)?;
    let engine = Arc::new(Engine::open(&data_dir.join("agentscope.db"))?);
    app.manage(engine.clone());
    app.manage(Runtime {
        data_dir: data_dir.clone(),
        ingest_port: Mutex::new(None),
    });

    tray::create(app)?;
    tray::set_waiting_count(app, engine.snapshot().waiting_count);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(1024);
        // Start listening before draining so nothing new lands in the spool meanwhile.
        let _server = match IngestServer::start(&data_dir, tx).await {
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
                let changed = spooled
                    .iter()
                    .fold(false, |acc, raw| engine.ingest(raw) | acc);
                tracing::info!(count = spooled.len(), "drained spool");
                if changed {
                    notify(&app, &engine);
                }
            }
            Err(err) => tracing::error!(%err, "failed to drain spool"),
        }
        while let Some(raw) = rx.recv().await {
            if engine.ingest(&raw) {
                notify(&app, &engine);
            }
        }
    });
    Ok(())
}

fn notify(app: &AppHandle, engine: &Engine) {
    let waiting_count = engine.snapshot().waiting_count;
    tray::set_waiting_count(app, waiting_count);
    if let Err(err) = (GraphChanged { waiting_count }).emit(app) {
        tracing::warn!(%err, "failed to emit graph change");
    }
}

pub(crate) fn show_main_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
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
