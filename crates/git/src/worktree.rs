//! Worktrees: several checkouts of one repository, side by side.
//!
//! The feature that makes reviewing a pull request cheap. Without it, looking
//! at someone else's branch means stashing, switching, unstashing, and hoping
//! nothing was lost — so in practice people either don't review, or they review
//! in the browser. A worktree is a second directory on a second branch sharing
//! one object store, so the current work is simply left alone.
//!
//! `git worktree list --porcelain` is the only sane way to read them. The
//! human-facing format aligns columns with spaces and embeds the branch in
//! brackets, which breaks on any path containing a space — and a checkout under
//! "My Documents" is not exotic.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{GitError, Repo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Short branch name, or `None` when the worktree is on a detached HEAD.
    pub branch: Option<String>,
    pub head: Option<String>,
    /// The main working tree, as opposed to a linked one. It cannot be removed.
    pub is_main: bool,
    /// The directory is gone but the administrative entry remains — usually
    /// someone deleted the folder by hand. Prunable, not usable.
    pub is_prunable: bool,
    /// Held by a `git worktree lock`, typically because it lives on removable
    /// media. Refuses removal until unlocked.
    pub is_locked: bool,
}

impl Worktree {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Whether removing this worktree is possible at all.
    pub fn removable(&self) -> bool {
        !self.is_main && !self.is_locked
    }
}

/// Every worktree of this repository, main first.
pub fn list(repo: &Repo) -> Result<Vec<Worktree>, GitError> {
    let out = git(repo, &["worktree", "list", "--porcelain"])?;
    Ok(parse_list(&out))
}

/// Test hook for the parser, which is otherwise private.
///
/// Exposed rather than making `parse_list` public: callers should go through
/// [`list`], and a `#[doc(hidden)]` function keeps that true while letting the
/// hostile-input tests reach the parser directly.
#[doc(hidden)]
pub fn parse_list_for_test(text: &str) -> Vec<Worktree> {
    parse_list(text)
}

/// Parse `git worktree list --porcelain`.
///
/// Records are separated by blank lines. `branch` carries a full ref name;
/// `detached` appears instead when there is no branch. `bare`, `locked` and
/// `prunable` are flag lines that may carry a reason after a space.
fn parse_list(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut current: Option<Worktree> = None;

    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            out.extend(current.take());
            continue;
        }

        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (line, ""),
        };

        match key {
            "worktree" => {
                out.extend(current.take());
                current = Some(Worktree {
                    path: PathBuf::from(value),
                    branch: None,
                    head: None,
                    // The first record `git worktree list` prints is always the
                    // main one; linked worktrees follow.
                    is_main: out.is_empty(),
                    is_prunable: false,
                    is_locked: false,
                });
            }
            "HEAD" => {
                if let Some(w) = current.as_mut() {
                    w.head = Some(value.to_string());
                }
            }
            "branch" => {
                if let Some(w) = current.as_mut() {
                    w.branch = Some(shorten_ref(value));
                }
            }
            "locked" => {
                if let Some(w) = current.as_mut() {
                    w.is_locked = true;
                }
            }
            "prunable" => {
                if let Some(w) = current.as_mut() {
                    w.is_prunable = true;
                }
            }
            _ => {}
        }
    }

    out.extend(current);
    out
}

fn shorten_ref(full: &str) -> String {
    full.strip_prefix("refs/heads/").unwrap_or(full).to_string()
}

/// Create a worktree at `path` on `branch`.
///
/// `create_branch` makes a new branch there; otherwise the branch must exist
/// and must not already be checked out elsewhere — git enforces that, and the
/// error it gives says which worktree holds it.
pub fn add(repo: &Repo, path: &Path, branch: &str, create_branch: bool) -> Result<(), GitError> {
    let path_str = path.to_string_lossy().into_owned();
    let mut args: Vec<&str> = vec!["worktree", "add"];
    if create_branch {
        args.push("-b");
        args.push(branch);
        args.push(&path_str);
    } else {
        args.push(&path_str);
        args.push(branch);
    }
    git(repo, &args).map(|_| ())
}

/// Remove a worktree and its directory.
///
/// `force` discards uncommitted changes inside it. Without it git refuses when
/// the worktree is dirty, which is the behaviour worth keeping by default —
/// the whole point of a worktree is that work lives in it.
pub fn remove(repo: &Repo, path: &Path, force: bool) -> Result<(), GitError> {
    let path_str = path.to_string_lossy().into_owned();
    let mut args: Vec<&str> = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path_str);
    git(repo, &args).map(|_| ())
}

/// Drop administrative entries whose directories are gone.
pub fn prune(repo: &Repo) -> Result<(), GitError> {
    git(repo, &["worktree", "prune"]).map(|_| ())
}

fn git(repo: &Repo, args: &[&str]) -> Result<String, GitError> {
    let workdir = repo.workdir().unwrap_or_else(|| repo.git_dir());
    let out = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(args)
        .output()?;

    if !out.status.success() {
        return Err(GitError::Walk(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::fixture;

    #[test]
    fn parses_a_main_worktree_and_a_linked_one() {
        let text = "\
worktree /home/me/project
HEAD abc123
branch refs/heads/main

worktree /home/me/project-pr-42
HEAD def456
branch refs/heads/pr/42
";
        let list = parse_list(text);
        assert_eq!(list.len(), 2);

        assert_eq!(list[0].path, PathBuf::from("/home/me/project"));
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert!(list[0].is_main);
        assert!(!list[0].removable(), "the main worktree cannot be removed");

        assert_eq!(list[1].branch.as_deref(), Some("pr/42"));
        assert!(!list[1].is_main);
        assert!(list[1].removable());
        assert_eq!(list[1].name(), "project-pr-42");
    }

    #[test]
    fn a_path_containing_a_space_survives() {
        // The human-readable format aligns columns with spaces, so this is
        // exactly the case that forces --porcelain.
        let text = "worktree /home/me/My Documents/project\nHEAD abc\nbranch refs/heads/main\n";
        let list = parse_list(text);
        assert_eq!(list[0].path, PathBuf::from("/home/me/My Documents/project"));
    }

    #[test]
    fn a_detached_worktree_has_no_branch() {
        let text = "worktree /tmp/wt\nHEAD abc123\ndetached\n";
        let list = parse_list(text);
        assert_eq!(list[0].branch, None);
        assert_eq!(list[0].head.as_deref(), Some("abc123"));
    }

    #[test]
    fn locked_and_prunable_flags_are_read_with_or_without_a_reason() {
        let text = "\
worktree /a
HEAD 1
branch refs/heads/main

worktree /b
HEAD 2
locked on a removable drive

worktree /c
HEAD 3
prunable
";
        let list = parse_list(text);
        assert!(!list[0].is_locked && !list[0].is_prunable);

        assert!(
            list[1].is_locked,
            "a reason after the flag must still parse"
        );
        assert!(
            !list[1].removable(),
            "a locked worktree refuses removal until unlocked"
        );

        assert!(list[2].is_prunable, "a bare flag with no reason must parse");
    }

    #[test]
    fn a_trailing_record_without_a_blank_line_is_not_lost() {
        // git does not always end its output with a blank line.
        let text = "worktree /a\nHEAD 1\nbranch refs/heads/main";
        assert_eq!(parse_list(text).len(), 1);
    }

    #[test]
    fn empty_output_yields_no_worktrees() {
        assert!(parse_list("").is_empty());
    }

    // --- against a real repository -------------------------------------------

    #[test]
    fn adding_a_worktree_creates_a_second_checkout() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();

        let holder = tempfile::tempdir().unwrap();
        let wt = holder.path().join("review");
        add(&repo, &wt, "pr/1", true).unwrap();

        assert!(wt.join("f.txt").exists(), "the new worktree is checked out");

        let list = list(&repo).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].is_main);
        assert_eq!(list[1].branch.as_deref(), Some("pr/1"));

        // And the original checkout is untouched — the entire point.
        assert!(dir.path().join("f.txt").exists());
    }

    #[test]
    fn removing_a_worktree_leaves_the_main_one_alone() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let holder = tempfile::tempdir().unwrap();
        let wt = holder.path().join("review");

        add(&repo, &wt, "pr/2", true).unwrap();
        remove(&repo, &wt, false).unwrap();

        assert!(!wt.exists());
        let list = list(&repo).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_main);
        assert!(dir.path().join("f.txt").exists());
    }

    #[test]
    fn a_dirty_worktree_refuses_removal_without_force() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let holder = tempfile::tempdir().unwrap();
        let wt = holder.path().join("review");
        add(&repo, &wt, "pr/3", true).unwrap();

        std::fs::write(wt.join("f.txt"), "uncommitted work\n").unwrap();

        assert!(
            remove(&repo, &wt, false).is_err(),
            "the point of a worktree is that work lives in it"
        );
        assert!(wt.exists(), "the refusal must not have deleted anything");

        remove(&repo, &wt, true).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn a_branch_already_checked_out_cannot_be_taken_twice() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let holder = tempfile::tempdir().unwrap();

        // `main` is checked out in the main worktree; git must refuse.
        let err = add(&repo, &holder.path().join("dup"), "main", false).unwrap_err();
        assert!(
            err.to_string().contains("already"),
            "git's own message says which worktree holds it: {err}"
        );
    }

    #[test]
    fn pruning_clears_an_entry_whose_directory_was_deleted() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let holder = tempfile::tempdir().unwrap();
        let wt = holder.path().join("review");
        add(&repo, &wt, "pr/4", true).unwrap();

        // Delete it the way a user would, outside git's knowledge.
        std::fs::remove_dir_all(&wt).unwrap();
        assert_eq!(
            list(&repo).unwrap().len(),
            2,
            "the entry outlives the folder"
        );

        prune(&repo).unwrap();
        assert_eq!(list(&repo).unwrap().len(), 1);
    }
}
