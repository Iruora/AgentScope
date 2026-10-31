//! A small always-on-top window for prompts that can be answered from
//! AgentScope, so the user does not have to find the main window. It opens
//! without taking focus, so typing in a terminal never lands on its buttons,
//! and closes itself once nothing is left to answer.

use std::collections::BTreeSet;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "popup";
const WIDTH: f64 = 420.0;
const HEIGHT: f64 = 300.0;
const MARGIN: f64 = 16.0;

/// Prompts the popup already appeared for, so closing it sticks until a new one arrives.
static SHOWN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Opens or closes the popup to match the prompts waiting here.
pub fn sync(app: &AppHandle, answerable: &[String], enabled: bool) {
    let new = {
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        shown.retain(|id| answerable.contains(id));
        let new = answerable.iter().any(|id| !shown.contains(id));
        shown.extend(answerable.iter().cloned());
        new
    };
    if answerable.is_empty() {
        close(app);
    } else if new && enabled && !main_window_focused(app) {
        if let Err(err) = open(app, false) {
            tracing::warn!(%err, "failed to open the approval popup");
        }
    }
}

fn main_window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

/// Shows the popup, focused only when the user asked for it.
pub fn open(app: &AppHandle, focus: bool) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        if focus {
            window.show()?;
            window.set_focus()?;
        }
        return Ok(());
    }
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("AgentScope · Approval needed")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(focus);
    // Top right of the main screen, out of the way of most editors.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let origin = monitor.position().to_logical::<f64>(scale);
        builder = builder.position(
            origin.x + size.width - WIDTH - MARGIN,
            origin.y + MARGIN * 3.0,
        );
    }
    builder.build()?;
    Ok(())
}

pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.close();
    }
}
