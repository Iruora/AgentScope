//! Read-only `git diff` for a file a session wrote, for the Files tab.

use std::path::Path;
use std::process::Command;

use serde::Serialize;
use specta::Type;

/// Diffs longer than this are cut.
const MAX_DIFF_BYTES: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    /// Unified diff against the index and HEAD, empty when the file is unchanged.
    pub diff: String,
    /// Git does not track the file yet.
    pub untracked: bool,
    /// The file is gone from disk.
    pub deleted: bool,
    pub truncated: bool,
}

fn git(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args(["--no-pager", "-c", "core.quotepath=off"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// `git diff HEAD -- <path>` in `repo`, without changing anything.
pub fn diff(repo: &Path, path: &Path) -> Result<FileDiff, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = git(repo)
            .args(args)
            .arg("--")
            .arg(path)
            .output()
            .map_err(|e| format!("Could not run git: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(err.lines().next().unwrap_or("git failed").to_owned());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let untracked = !run(&["ls-files", "--others", "--exclude-standard"])?
        .trim()
        .is_empty();
    // `HEAD` is missing in a repository without commits; fall back to the index.
    let mut text = run(&["diff", "--no-color", "--no-ext-diff", "HEAD"])
        .or_else(|_| run(&["diff", "--no-color", "--no-ext-diff"]))?;
    let truncated = text.len() > MAX_DIFF_BYTES;
    if truncated {
        let mut cut = MAX_DIFF_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    Ok(FileDiff {
        diff: text,
        untracked,
        deleted: !path.exists(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn diffs_tracked_and_flags_untracked_files() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        sh(repo, &["init", "-q"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        sh(repo, &["add", "a.txt"]);
        sh(repo, &["commit", "-q", "-m", "init"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join("new.txt"), "hi\n").unwrap();

        let d = diff(repo, &repo.join("a.txt")).unwrap();
        assert!(d.diff.contains("+two"), "{}", d.diff);
        assert!(!d.untracked && !d.deleted && !d.truncated);

        let d = diff(repo, &repo.join("new.txt")).unwrap();
        assert!(d.untracked);
        assert!(d.diff.is_empty());

        std::fs::remove_file(repo.join("a.txt")).unwrap();
        let d = diff(repo, &repo.join("a.txt")).unwrap();
        assert!(d.deleted);
        assert!(d.diff.contains("-one"));
    }
}
