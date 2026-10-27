//! "Go to agent": bring the terminal or IDE window an agent runs in to the
//! front. Best effort per OS (spec section 5.2): walk up from the harness
//! process to the ancestor that owns a window.

use std::path::Path;

use serde::Serialize;
use specta::Type;

use crate::engine::AgentLocation;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GoToAgent {
    pub focused: bool,
    /// The app that was brought forward, e.g. "iTerm2".
    pub app: Option<String>,
    /// When focusing failed: a command for the user to run instead, which the
    /// UI copies to the clipboard.
    pub fallback_command: Option<String>,
    pub message: Option<String>,
}

/// Last path segment, for notification titles.
pub fn display_dir(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(cwd)
        .to_owned()
}

fn shell_quote(path: &str) -> String {
    if path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-~:\\".contains(c))
    {
        path.to_owned()
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

pub fn go_to(location: &AgentLocation) -> GoToAgent {
    let fallback =
        || (!location.cwd.is_empty()).then(|| format!("cd {}", shell_quote(&location.cwd)));
    if location.chain.is_empty() {
        return GoToAgent {
            focused: false,
            app: None,
            fallback_command: fallback(),
            message: Some("The agent's process is not running any more.".into()),
        };
    }
    match platform::focus(location) {
        Some(app) => GoToAgent {
            focused: true,
            app: Some(app),
            fallback_command: None,
            message: None,
        },
        None => GoToAgent {
            focused: false,
            app: None,
            fallback_command: fallback(),
            message: Some(platform::UNSUPPORTED.into()),
        },
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use crate::engine::AgentLocation;

    pub const UNSUPPORTED: &str = "Could not find the app this agent runs in.";

    /// The outermost `.app` bundle in an executable path, so a helper inside
    /// `Visual Studio Code.app/…/Code Helper.app` resolves to VS Code.
    fn app_bundle(exe: &Path) -> Option<PathBuf> {
        let mut bundle = PathBuf::new();
        for part in exe.components() {
            bundle.push(part);
            if part.as_os_str().to_string_lossy().ends_with(".app") {
                return Some(bundle);
            }
        }
        None
    }

    fn tty_of(pid: u32) -> Option<String> {
        let out = Command::new("ps")
            .args(["-o", "tty=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let tty = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let valid =
            !tty.is_empty() && tty != "??" && tty.chars().all(|c| c.is_ascii_alphanumeric());
        valid.then(|| format!("/dev/{tty}"))
    }

    /// Selects the tab whose tty is `tty` in Terminal or iTerm2. Needs the
    /// user's one-time Automation consent, so failures are ignored.
    fn select_tab(app: &str, tty: &str) {
        let script = match app {
            "Terminal" => format!(
                r#"tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "{tty}" then
        set selected of t to true
        set index of w to 1
        return
      end if
    end repeat
  end repeat
end tell"#
            ),
            "iTerm" | "iTerm2" => format!(
                r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{tty}" then
          select w
          select t
          select s
          return
        end if
      end repeat
    end repeat
  end repeat
end tell"#
            ),
            _ => return,
        };
        let _ = Command::new("osascript")
            .args(["-e", &script])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    pub fn focus(location: &AgentLocation) -> Option<String> {
        let bundle = location
            .exes
            .iter()
            .flatten()
            .find_map(|exe| app_bundle(exe))?;
        let app = bundle.file_stem()?.to_string_lossy().into_owned();
        if let Some(tty) = tty_of(location.chain[0].pid) {
            select_tab(&app, &tty);
        }
        let opened = Command::new("open")
            .arg("-a")
            .arg(&bundle)
            .status()
            .is_ok_and(|s| s.success());
        opened.then_some(app)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn finds_the_outermost_bundle() {
            assert_eq!(
                app_bundle(Path::new(
                    "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper"
                )),
                Some(PathBuf::from("/Applications/Visual Studio Code.app"))
            );
            assert_eq!(app_bundle(Path::new("/bin/zsh")), None);
        }
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
    };

    use crate::engine::AgentLocation;

    pub const UNSUPPORTED: &str = "Could not find the window this agent runs in.";

    struct Search {
        pids: Vec<u32>,
        /// Rank in `pids` (nearest ancestor first) and the window.
        best: Option<(usize, HWND)>,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `Search` passed to `EnumWindows` below,
        // which outlives the enumeration.
        let search = unsafe { &mut *(lparam as *mut Search) };
        // SAFETY: `hwnd` comes from the enumeration and is valid during it.
        unsafe {
            if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
                return 1;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if let Some(rank) = search.pids.iter().position(|p| *p == pid) {
                if search.best.is_none_or(|(best, _)| rank < best) {
                    search.best = Some((rank, hwnd));
                }
            }
        }
        1
    }

    pub fn focus(location: &AgentLocation) -> Option<String> {
        let mut search = Search {
            pids: location.chain.iter().map(|p| p.pid).collect(),
            best: None,
        };
        // SAFETY: the callback only touches `search`, which lives across the call.
        unsafe {
            EnumWindows(Some(visit), &mut search as *mut Search as LPARAM);
        }
        let (rank, hwnd) = search.best?;
        // SAFETY: `hwnd` was a valid top-level window a moment ago; these
        // calls fail harmlessly if it closed since.
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            if SetForegroundWindow(hwnd) == 0 {
                return None;
            }
        }
        let name = &location.chain[rank].name;
        Some(name.strip_suffix(".exe").unwrap_or(name).to_owned())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::process::Command;

    use crate::engine::AgentLocation;

    pub const UNSUPPORTED: &str =
        "Focusing windows needs X11 and wmctrl. Run the copied command in a terminal instead.";

    /// Uses `wmctrl` (EWMH), which works on X11 and for X apps under XWayland.
    pub fn focus(location: &AgentLocation) -> Option<String> {
        let out = Command::new("wmctrl").arg("-lp").output().ok()?;
        let listing = String::from_utf8_lossy(&out.stdout);
        let (rank, window) = listing
            .lines()
            .filter_map(|line| {
                let mut cols = line.split_whitespace();
                let id = cols.next()?;
                let pid: u32 = cols.nth(1)?.parse().ok()?;
                let rank = location.chain.iter().position(|p| p.pid == pid)?;
                Some((rank, id.to_owned()))
            })
            .min_by_key(|(rank, _)| *rank)?;
        let ok = Command::new("wmctrl")
            .args(["-ia", &window])
            .status()
            .is_ok_and(|s| s.success());
        ok.then(|| location.chain[rank].name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths_for_the_fallback_command() {
        assert_eq!(shell_quote("/work/shop"), "/work/shop");
        assert_eq!(shell_quote("/my repo/it's"), r"'/my repo/it'\''s'");
        assert_eq!(display_dir("/work/shop"), "shop");
    }

    #[test]
    fn a_gone_process_gets_a_fallback() {
        let result = go_to(&AgentLocation {
            cwd: "/work/shop".into(),
            chain: Vec::new(),
            exes: Vec::new(),
        });
        assert!(!result.focused);
        assert_eq!(result.fallback_command.as_deref(), Some("cd /work/shop"));
    }
}
