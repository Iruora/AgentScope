//! Tray / menu bar icon. The badge shows how many agents are waiting on the
//! user, and the menu lists live sessions, waiting ones first.

use agentscope_core::{AgentState, Snapshot};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

const TRAY_ID: &str = "main";
const SESSION_PREFIX: &str = "session:";
const MAX_SESSIONS: usize = 10;

/// What the menu shows, to skip rebuilding it when nothing visible changed.
pub type MenuKey = Vec<(String, AgentState, String)>;

fn build_menu<R: Runtime>(app: &AppHandle<R>, sessions: &MenuKey) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for (id, state, label) in sessions {
        let marker = if state.needs_user() { "● " } else { "" };
        menu.append(&MenuItem::with_id(
            app,
            format!("{SESSION_PREFIX}{id}"),
            format!("{marker}{label}"),
            true,
            None::<&str>,
        )?)?;
    }
    if !sessions.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open AgentScope",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit AgentScope",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("AgentScope")
        .menu(&build_menu(app, &Vec::new())?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_main_window(app),
            "quit" => app.exit(0),
            id => {
                if let Some(session) = id.strip_prefix(SESSION_PREFIX) {
                    crate::open_session(app, session);
                }
            }
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

/// Live sessions for the menu: waiting on the user first, then most recent.
pub fn menu_key(snapshot: &Snapshot) -> MenuKey {
    let mut live: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|s| s.ended_at.is_none())
        .collect();
    // `sessions` is most recent first and the sort is stable.
    live.sort_by_key(|s| !s.state.needs_user());
    live.into_iter()
        .take(MAX_SESSIONS)
        .map(|s| {
            let name = s
                .title
                .clone()
                .unwrap_or_else(|| crate::focus::display_dir(&s.cwd));
            (
                s.id.clone(),
                s.state,
                agentscope_core::redact::truncate(&name, 48),
            )
        })
        .collect()
}

pub fn update<R: Runtime>(app: &AppHandle<R>, waiting: u32, sessions: Option<&MenuKey>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let tooltip = match waiting {
        0 => "AgentScope".to_owned(),
        1 => "AgentScope · 1 agent waiting".to_owned(),
        n => format!("AgentScope · {n} agents waiting"),
    };
    let _ = tray.set_tooltip(Some(tooltip));
    // Only macOS shows text next to the menu bar icon.
    #[cfg(target_os = "macos")]
    let _ = tray.set_title((waiting > 0).then(|| waiting.to_string()));
    if let Some(sessions) = sessions {
        match build_menu(app, sessions) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(err) => tracing::warn!(%err, "failed to rebuild tray menu"),
        }
    }
}
