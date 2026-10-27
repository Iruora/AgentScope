//! IPC commands. Types are exported to `src/bindings.ts` by tauri-specta.

use std::sync::Arc;

use agentscope_adapters::Detection;
use agentscope_core::Snapshot;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::engine::Engine;
use crate::Runtime;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    /// `None` until the ingest server is up, or if it failed to start.
    pub ingest_port: Option<u16>,
    pub harnesses: Vec<Detection>,
}

/// Emitted whenever the graph changes. The UI refetches the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct GraphChanged {
    pub waiting_count: u32,
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
