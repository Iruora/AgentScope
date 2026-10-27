//! Tray / menu bar icon. The badge shows how many agents are waiting on the user.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

const TRAY_ID: &str = "main";

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let open = MenuItem::with_id(app, "open", "Open AgentScope", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit AgentScope", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("AgentScope")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)
}

pub fn set_waiting_count<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let tooltip = match count {
        0 => "AgentScope".to_owned(),
        1 => "AgentScope · 1 agent waiting".to_owned(),
        n => format!("AgentScope · {n} agents waiting"),
    };
    let _ = tray.set_tooltip(Some(tooltip));
    // Only macOS shows text next to the menu bar icon.
    #[cfg(target_os = "macos")]
    let _ = tray.set_title((count > 0).then(|| count.to_string()));
}
