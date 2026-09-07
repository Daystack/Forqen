//! The reflog: every position HEAD has held, and the way back from a mistake.
//!
//! git already records this. What is missing everywhere is a way to *read* it
//! without knowing it exists — so a bad reset, a rebase that ate a commit, or
//! a branch deleted one line too early becomes a search-engine problem instead
//! of a button.
//!
//! Entries are read with `%gd%x00%gs%x00%H%x00%gD`, null-separated, because a
//! reflog message is free text: "commit: fix the thing | with a pipe" is a
//! legal message and any printable separator eventually appears inside one.

use std::process::Command;

use crate::{GitError, ObjectId, Repo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Selector as git names it: `HEAD@{3}`.
    pub selector: String,
    /// What moved HEAD: `commit`, `rebase (finish)`, `reset`, `checkout`, …
    pub action: String,
    /// The rest of the message after the action.
    pub detail: String,
    pub id: ObjectId,
    /// Relative time git prints, e.g. `2 hours ago`.
    pub when: String,
}

impl Entry {
    /// Whether this entry is a plausible thing to recover *to*.
    ///
    /// A reflog is mostly noise — every checkout and every commit appears. The
    /// entries worth surfacing are the ones immediately before something
    /// destructive, and the destructive actions are what mark them.
    pub fn is_recovery_point(&self) -> bool {
        matches!(
            self.action.as_str(),
            "reset" | "rebase" | "merge" | "revert" | "am" | "cherry-pick"
        ) || self.action.starts_with("rebase")
    }
}

/// Read the reflog for `ref_name`, newest first.
pub fn read(repo: &Repo, ref_name: &str, limit: usize) -> Result<Vec<Entry>, GitError> {
    let workdir = repo.workdir().unwrap_or_else(|| repo.git_dir());
    let out = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args([
            "reflog",
            "show",
            // `%gD` is the indexed selector (HEAD@{0}); `%gd` under
            // --date=relative is the *same* selector with the time in place of
            // the index (HEAD@{2 hours ago}), which is how git exposes when an
            // entry was written. There is no %gr — an earlier version used one
            // and git printed it back literally, into the UI.
            "--date=relative",
            "--format=%gD%x00%gs%x00%H%x00%gd",
            &format!("-{limit}"),
            ref_name,
        ])
        .output()?;

    // A ref with no reflog is not an error — a freshly cloned repository has
    // none for most branches.
    if !out.status.success() {
        return Ok(Vec::new());
    }

    let mut entries = parse(&String::from_utf8_lossy(&out.stdout));

    // Renumber the selectors.
    //
    // `--date=relative` applies to `%gD` as well as `%gd`, so git returns
    // `HEAD@{2 minutes ago}` for both — the index form is unavailable in the
    // same call. Entries come back newest first, which *is* git's indexing, so
    // position gives back `HEAD@{0}`, `HEAD@{1}` … exactly as `git reflog`
    // prints them and as they can be typed into a command.
    for (i, entry) in entries.iter_mut().enumerate() {
        entry.selector = format!("{ref_name}@{{{i}}}");
    }

    Ok(entries)
}

fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let selector = parts.next()?.to_string();
            let subject = parts.next()?;
            let hex = parts.next()?;
            // The relative selector reads `HEAD@{2 hours ago}`; the time is
            // what sits inside the braces.
            let when = parts.next().map(relative_time_of).unwrap_or_default();

            // `%gs` reads "action: detail"; the action is everything before the
            // first colon. Messages without one — git writes a few — keep the
            // whole string as the action and an empty detail.
            let (action, detail) = match subject.split_once(':') {
                Some((a, d)) => (a.trim().to_string(), d.trim().to_string()),
                None => (subject.trim().to_string(), String::new()),
            };

            Some(Entry {
                selector,
                action,
                detail,
                id: parse_hex(hex)?,
                when,
            })
        })
        .collect()
}

/// Move `ref_name` back to the commit an entry points at.
///
/// A hard reset, because a soft one would leave the working tree describing a
/// state that no longer matches HEAD — and someone reaching for the reflog is
/// trying to undo, not to stage a diff.
///
/// This is itself recorded in the reflog, so an undo can be undone.
pub fn restore(repo: &Repo, id: ObjectId) -> Result<(), GitError> {
    let workdir = repo.workdir().unwrap_or_else(|| repo.git_dir());
    let out = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["reset", "--hard", &id.to_hex()])
        .output()?;

    if !out.status.success() {
        return Err(GitError::Walk(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(())
}

/// Pull `2 hours ago` out of `HEAD@{2 hours ago}`.
///
/// Falls back to the whole string rather than an empty label: an unfamiliar
/// shape should still show something the user can read.
fn relative_time_of(selector: &str) -> String {
    match (selector.find('{'), selector.rfind('}')) {
        (Some(open), Some(close)) if close > open + 1 => selector[open + 1..close].to_string(),
        _ => selector.trim().to_string(),
    }
}

fn parse_hex(hex: &str) -> Option<ObjectId> {
    let hex = hex.trim();
    if hex.len() < 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(ObjectId(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::fixture;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn splits_the_action_from_its_detail() {
        let line =
            format!("HEAD@{{0}}\u{0}commit: add the thing\u{0}{SHA}\u{0}HEAD@{{2 hours ago}}");
        let e = &parse(&line)[0];
        assert_eq!(e.selector, "HEAD@{0}");
        assert_eq!(e.action, "commit");
        assert_eq!(e.detail, "add the thing");
        assert_eq!(e.when, "2 hours ago");
    }

    #[test]
    fn a_message_containing_punctuation_survives() {
        // Null separation is the point: a reflog message is free text, and any
        // printable delimiter eventually appears inside one.
        let line =
            format!("HEAD@{{1}}\u{0}commit: fix a|b, c:d — and more\u{0}{SHA}\u{0}1 day ago");
        let e = &parse(&line)[0];
        assert_eq!(e.action, "commit");
        assert_eq!(
            e.detail, "fix a|b, c:d — and more",
            "only the first colon separates; the rest belongs to the message"
        );
    }

    #[test]
    fn a_message_with_no_colon_keeps_everything_as_the_action() {
        let line = format!("HEAD@{{2}}\u{0}rebase (finish)\u{0}{SHA}\u{0}HEAD@{{3 days ago}}");
        let e = &parse(&line)[0];
        assert_eq!(e.action, "rebase (finish)");
        assert!(e.detail.is_empty());
        assert!(e.is_recovery_point(), "a finished rebase is worth offering");
    }

    #[test]
    fn destructive_actions_are_marked_as_recovery_points() {
        let mk = |action: &str| Entry {
            selector: "HEAD@{0}".into(),
            action: action.into(),
            detail: String::new(),
            id: ObjectId([0; 20]),
            when: String::new(),
        };
        for a in ["reset", "rebase", "merge", "revert", "am", "cherry-pick"] {
            assert!(
                mk(a).is_recovery_point(),
                "{a} is worth offering as an undo"
            );
        }
        for a in ["commit", "checkout", "clone", "pull"] {
            assert!(!mk(a).is_recovery_point(), "{a} is ordinary movement");
        }
    }

    #[test]
    fn a_malformed_line_is_skipped_rather_than_panicking() {
        assert!(parse("nonsense with no nulls").is_empty());
        assert!(parse("HEAD@{0}\u{0}commit: x\u{0}tooshort\u{0}HEAD@{now}").is_empty());
        assert!(parse("").is_empty());
    }

    // --- against a real repository -------------------------------------------

    #[test]
    fn reads_the_reflog_of_a_real_repository() {
        let dir = fixture(3);
        let repo = crate::Repo::open(dir.path()).unwrap();

        // fast-import plus the reset the fixture does leaves at least one entry.
        let entries = read(&repo, "HEAD", 20).unwrap();
        assert!(
            !entries.is_empty(),
            "a repository with commits has a reflog"
        );
        assert!(entries[0].selector.starts_with("HEAD@{"));
    }

    #[test]
    fn a_hard_reset_can_be_undone_from_the_reflog() {
        let dir = fixture(3);
        let repo = crate::Repo::open(dir.path()).unwrap();

        let before = crate::history::Walker::from_head(&repo)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();

        // Throw away the top two commits, the classic mistake.
        let out = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["reset", "--hard", "HEAD~2"])
            .output()
            .unwrap();
        assert!(out.status.success());

        let repo = crate::Repo::open(dir.path()).unwrap();
        let after_reset = crate::history::Walker::from_head(&repo)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_ne!(before, after_reset, "the reset really moved HEAD");

        // The reflog still knows where it was.
        let entries = read(&repo, "HEAD", 20).unwrap();
        let target = entries
            .iter()
            .find(|e| e.id == before)
            .expect("the pre-reset position must still be recorded");

        restore(&repo, target.id).unwrap();

        let repo = crate::Repo::open(dir.path()).unwrap();
        let recovered = crate::history::Walker::from_head(&repo)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(recovered, before, "the commits are back");
    }

    #[test]
    fn an_undo_is_itself_recorded_so_it_can_be_undone() {
        let dir = fixture(3);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let before = crate::history::Walker::from_head(&repo)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();

        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["reset", "--hard", "HEAD~1"])
            .output()
            .unwrap();

        let repo = crate::Repo::open(dir.path()).unwrap();
        restore(&repo, before).unwrap();

        let repo = crate::Repo::open(dir.path()).unwrap();
        let entries = read(&repo, "HEAD", 20).unwrap();
        assert!(
            entries.len() >= 2,
            "the restore must appear in the reflog too, or an undo cannot be undone"
        );
    }

    #[test]
    fn a_ref_with_no_reflog_yields_nothing_rather_than_an_error() {
        let dir = fixture(1);
        let repo = crate::Repo::open(dir.path()).unwrap();
        assert!(read(&repo, "refs/heads/does-not-exist", 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_time_is_taken_from_inside_the_relative_selector() {
        // git has no %gr. An earlier version used one and git printed the
        // literal "%gr" straight into the UI; the time comes from %gd under
        // --date=relative instead.
        assert_eq!(relative_time_of("HEAD@{2 hours ago}"), "2 hours ago");
        assert_eq!(
            relative_time_of("refs/heads/main@{3 days ago}"),
            "3 days ago"
        );
    }

    #[test]
    fn an_unexpected_selector_shape_still_shows_something() {
        assert_eq!(relative_time_of("HEAD@{}"), "HEAD@{}");
        assert_eq!(relative_time_of("nonsense"), "nonsense");
        assert_eq!(relative_time_of(""), "");
    }

    #[test]
    fn a_real_reflog_entry_has_a_readable_time() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let entries = read(&repo, "HEAD", 5).unwrap();
        let when = &entries[0].when;
        assert!(!when.is_empty());
        assert!(
            !when.contains('%'),
            "a literal format placeholder reached the UI: {when}"
        );
        assert!(
            !when.contains('{'),
            "the selector braces should have been stripped: {when}"
        );
    }

    #[test]
    fn selectors_are_the_index_form_that_can_be_typed() {
        // --date=relative applies to %gD too, so git returns the time form for
        // both fields. The index is recovered from position, which is what
        // git's own numbering is.
        let dir = fixture(3);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let entries = read(&repo, "HEAD", 10).unwrap();

        assert_eq!(entries[0].selector, "HEAD@{0}");
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(e.selector, format!("HEAD@{{{i}}}"));
            assert!(
                !e.selector.contains("ago"),
                "the selector must be typeable, not a date: {}",
                e.selector
            );
        }
    }
}
