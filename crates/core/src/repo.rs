//! Finds the git repository and branch a session's cwd belongs to, by reading
//! `.git` directly so no `git` binary is needed.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::Repo;

/// The repo containing `cwd`, with its checked-out branch if HEAD is on one.
pub fn repo_for(cwd: &Path) -> Option<Repo> {
    let root = cwd.ancestors().find(|dir| dir.join(".git").exists())?;
    Some(Repo {
        root: root.display().to_string(),
        branch: git_dir(root).and_then(|dir| branch(&dir)),
    })
}

/// `.git` is a directory, or a file pointing at one for worktrees and submodules.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    Some(root.join(target))
}

fn branch(git_dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_root_and_branch_from_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        let sub = dir.path().join("src/deep");
        fs::create_dir_all(&sub).unwrap();

        let repo = repo_for(&sub).unwrap();
        assert_eq!(repo.root, dir.path().display().to_string());
        assert_eq!(repo.branch.as_deref(), Some("feature/x"));
    }

    #[test]
    fn follows_worktree_gitdir_files_and_detached_heads() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("main/.git/worktrees/wt");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("HEAD"), "0123abcd\n").unwrap();
        let wt = dir.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", real.display())).unwrap();

        let repo = repo_for(&wt).unwrap();
        assert_eq!(repo.root, wt.display().to_string());
        assert_eq!(repo.branch, None);
    }
}
