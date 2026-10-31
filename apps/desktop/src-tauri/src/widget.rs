//! The mini widget: a small always-on-top window with the "Needs you" count
//! and one line per live agent, for keeping an eye on agents while working
//! in another app. It is toggled from the tray, Settings or its own close
//! button, and reopens on launch when it was left open.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "widget";
pub const WIDTH: f64 = 280.0;
/// The starting height; the page resizes the window to fit its rows.
const HEIGHT: f64 = 120.0;
const MARGIN: f64 = 16.0;

/// Opens or closes the widget.
pub fn set(app: &AppHandle, visible: bool) {
    if visible {
        if let Err(err) = open(app) {
            tracing::warn!(%err, "failed to open the mini widget");
        }
    } else if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.close();
    }
}

fn open(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return window.show();
    }
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("AgentScope widget")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .minimizable(false)
        .maximizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .shadow(true)
        .focused(false);
    // Bottom right of the main screen, above the taskbar or dock.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let origin = monitor.position().to_logical::<f64>(scale);
        builder = builder.position(
            origin.x + size.width - WIDTH - MARGIN,
            origin.y + size.height - HEIGHT - MARGIN * 6.0,
        );
    }
    builder.build()?;
    Ok(())
}
