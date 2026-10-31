//! Tray / menu bar icon. The badge shows how many agents are waiting on the
//! user. The menu lists the permission prompts that can be answered from
//! AgentScope, each with Approve / Deny / Ask in terminal, then live sessions,
//! waiting ones first.

use agentscope_core::{AgentState, Snapshot};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

const TRAY_ID: &str = "main";
const SESSION_PREFIX: &str = "session:";
const MAX_SESSIONS: usize = 10;
const MAX_APPROVALS: usize = 5;

/// What the menu shows, to skip rebuilding it when nothing visible changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuKey {
    approvals: Vec<MenuApproval>,
    sessions: Vec<(String, AgentState, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuApproval {
    id: String,
    session_id: String,
    label: String,
    /// Risky enough that approving needs a second look in the popup.
    review: bool,
}

/// A tray action on one prompt, encoded in the menu item id.
#[derive(Debug, PartialEq, Eq)]
enum Action<'a> {
    Approve(&'a str),
    Deny(&'a str),
    Release(&'a str),
    Review(&'a str),
}

impl<'a> Action<'a> {
    fn id(verb: &str, approval_id: &str) -> String {
        format!("{verb}:{approval_id}")
    }

    fn parse(id: &'a str) -> Option<Self> {
        let (verb, approval_id) = id.split_once(':')?;
        Some(match verb {
            "approve" => Self::Approve(approval_id),
            "deny" => Self::Deny(approval_id),
            "release" => Self::Release(approval_id),
            "review" => Self::Review(approval_id),
            _ => return None,
        })
    }
}

fn item(app: &AppHandle, id: impl AsRef<str>, text: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, id.as_ref(), text, true, None::<&str>)
}

fn build_menu(app: &AppHandle, key: &MenuKey) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    if !key.approvals.is_empty() {
        menu.append(&MenuItem::new(
            app,
            "Answer from here",
            false,
            None::<&str>,
        )?)?;
        for a in &key.approvals {
            let approve = if a.review {
                item(app, Action::id("review", &a.id), "Review and approve…")?
            } else {
                item(app, Action::id("approve", &a.id), "Approve")?
            };
            let submenu = Submenu::with_items(
                app,
                format!("⏵ {}", a.label),
                true,
                &[
                    &approve,
                    &item(app, Action::id("deny", &a.id), "Deny")?,
                    &item(app, Action::id("release", &a.id), "Ask in terminal")?,
                    &PredefinedMenuItem::separator(app)?,
                    &item(
                        app,
                        format!("{SESSION_PREFIX}{}", a.session_id),
                        "Show session",
                    )?,
                ],
            )?;
            menu.append(&submenu)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    for (id, state, label) in &key.sessions {
        let marker = if state.needs_user() { "● " } else { "" };
        menu.append(&item(
            app,
            format!("{SESSION_PREFIX}{id}"),
            &format!("{marker}{label}"),
        )?)?;
    }
    if !key.sessions.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&item(app, "open", "Open AgentScope")?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item(app, "quit", "Quit AgentScope")?)?;
    Ok(menu)
}

pub fn create(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("AgentScope")
        .menu(&build_menu(app, &MenuKey::default())?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_main_window(app),
            "quit" => app.exit(0),
            id => {
                if let Some(session) = id.strip_prefix(SESSION_PREFIX) {
                    crate::open_session(app, session);
                    return;
                }
                let result = match Action::parse(id) {
                    Some(Action::Approve(a)) => crate::commands::answer(app, a, true),
                    Some(Action::Deny(a)) => crate::commands::answer(app, a, false),
                    Some(Action::Release(a)) => {
                        crate::commands::release_approval(app.clone(), a.to_owned());
                        Ok(())
                    }
                    Some(Action::Review(_)) => {
                        crate::popup::open(app, true).map_err(|e| e.to_string())
                    }
                    None => Ok(()),
                };
                if let Err(err) = result {
                    tracing::warn!(%err, "tray action failed");
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

/// Answerable prompts, oldest first, then live sessions: waiting on the
/// user first, then most recent.
pub fn menu_key(snapshot: &Snapshot, answerable: &[String], confirm_risky: bool) -> MenuKey {
    let place = |session_id: &str| {
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| crate::focus::display_dir(&s.cwd))
    };
    let approvals = snapshot
        .approvals
        .iter()
        .filter(|a| answerable.contains(&a.id))
        .take(MAX_APPROVALS)
        .map(|a| {
            let summary = agentscope_core::redact::truncate(&a.summary, 40);
            MenuApproval {
                id: a.id.clone(),
                session_id: a.session_id.clone(),
                label: match place(&a.session_id) {
                    Some(dir) => format!(
                        "{summary} · {}",
                        agentscope_core::redact::truncate(&dir, 24)
                    ),
                    None => summary,
                },
                review: confirm_risky && crate::risky::is_risky(&a.summary),
            }
        })
        .collect();

    let mut live: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|s| s.ended_at.is_none())
        .collect();
    // `sessions` is most recent first and the sort is stable.
    live.sort_by_key(|s| !s.state.needs_user());
    let sessions = live
        .into_iter()
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
        .collect();
    MenuKey {
        approvals,
        sessions,
    }
}

pub fn update(app: &AppHandle, waiting: u32, menu: Option<&MenuKey>) {
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
    if let Some(key) = menu {
        match build_menu(app, key) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(err) => tracing::warn!(%err, "failed to rebuild tray menu"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_ids_round_trip() {
        // Approval ids contain `#` and may contain `:` from session keys.
        let id = Action::id("deny", "claude_code:abc#toolu_1");
        assert_eq!(
            Action::parse(&id),
            Some(Action::Deny("claude_code:abc#toolu_1"))
        );
        assert_eq!(Action::parse("open"), None);
        assert_eq!(Action::parse("session:x"), None);
    }
}
