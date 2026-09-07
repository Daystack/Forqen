//! Repository search: file contents, file names, and commit messages.
//!
//! Backed by `git grep` and `git log` rather than a walk of the working tree.
//! git already has an index of tracked paths, respects `.gitignore` for free,
//! and never descends into `target/` or `node_modules/` — the three things a
//! naive recursive search gets wrong, in the order it gets them wrong.
//!
//! `-z` throughout. A path may contain a colon or a newline, and both appear
//! in the default output as field separators.

use std::process::Command;

use crate::{GitError, ObjectId, Repo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A line inside a tracked file.
    Content,
    /// A tracked path whose name matches.
    Path,
    /// A commit whose message matches.
    Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub kind: Kind,
    pub path: String,
    /// Line number for a content hit; `None` for a path or commit.
    pub line: Option<u32>,
    /// The matching line, the path, or the commit summary.
    pub text: String,
    /// Set for commit hits.
    pub commit: Option<ObjectId>,
}

/// Search everything, capped at `limit` hits per category.
///
/// Three categories rather than one ranked list: they answer different
/// questions — "where is this string", "where is this file", "when did this
/// change" — and blending them buries the one the user meant.
pub fn search(repo: &Repo, query: &str, limit: usize) -> Result<Vec<Hit>, GitError> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut hits = Vec::new();
    hits.extend(content(repo, query, limit)?);
    hits.extend(paths(repo, query, limit)?);
    hits.extend(messages(repo, query, limit)?);
    Ok(hits)
}

fn content(repo: &Repo, query: &str, limit: usize) -> Result<Vec<Hit>, GitError> {
    // --fixed-strings: a search box is not a regex prompt, and a stray `(`
    // would otherwise be an error rather than a search for a bracket.
    let out = run(
        repo,
        &[
            "grep",
            "--line-number",
            "--fixed-strings",
            "--ignore-case",
            "--null",
            "-e",
            query,
        ],
    )?;

    Ok(out
        .lines()
        .take(limit)
        .filter_map(|line| {
            // `path\0line\0text`. Verified against git's own bytes rather than
            // assumed: --null separates *every* field, including the line
            // number, so there is no colon to disambiguate and a path
            // containing one parses correctly for free.
            let (path, rest) = line.split_once('\0')?;
            let (no, text) = rest.split_once('\0')?;
            Some(Hit {
                kind: Kind::Content,
                path: path.to_string(),
                line: no.parse().ok(),
                text: text.trim_end().to_string(),
                commit: None,
            })
        })
        .collect())
}

fn paths(repo: &Repo, query: &str, limit: usize) -> Result<Vec<Hit>, GitError> {
    let out = run(repo, &["ls-files", "-z"])?;
    let needle = query.to_lowercase();

    Ok(out
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| p.to_lowercase().contains(&needle))
        .take(limit)
        .map(|p| Hit {
            kind: Kind::Path,
            path: p.to_string(),
            line: None,
            text: p.to_string(),
            commit: None,
        })
        .collect())
}

fn messages(repo: &Repo, query: &str, limit: usize) -> Result<Vec<Hit>, GitError> {
    let out = run(
        repo,
        &[
            "log",
            "--fixed-strings",
            "--regexp-ignore-case",
            &format!("--grep={query}"),
            &format!("-{limit}"),
            "--format=%H%x00%s",
        ],
    )?;

    Ok(out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let (hex, summary) = line.split_once('\0')?;
            Some(Hit {
                kind: Kind::Message,
                path: String::new(),
                line: None,
                text: summary.to_string(),
                commit: parse_hex(hex),
            })
        })
        .collect())
}

fn run(repo: &Repo, args: &[&str]) -> Result<String, GitError> {
    let workdir = repo.workdir().unwrap_or_else(|| repo.git_dir());
    let out = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(args)
        .output()?;

    // `git grep` exits 1 when nothing matched, which is not a failure.
    if !out.status.success() && out.status.code() != Some(1) {
        return Err(GitError::Walk(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
    use std::process::Command;

    /// A repo with searchable content, ignored files, and varied messages.
    fn searchable() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .args(args)
                .current_dir(p)
                .output()
                .unwrap();
            assert!(ok.status.success(), "git {args:?} failed");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.name", "Fixture"]);
        run(&["config", "user.email", "fixture@example.invalid"]);

        std::fs::write(p.join(".gitignore"), "ignored/\n").unwrap();
        std::fs::write(p.join("alpha.rs"), "fn needle() {}\nfn other() {}\n").unwrap();
        std::fs::write(p.join("beta.txt"), "nothing here\n").unwrap();
        std::fs::create_dir_all(p.join("ignored")).unwrap();
        std::fs::write(p.join("ignored/secret.rs"), "fn needle() {}\n").unwrap();

        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "add the needle function"]);

        std::fs::write(p.join("beta.txt"), "still nothing\n").unwrap();
        run(&["commit", "-qam", "unrelated change"]);
        dir
    }

    #[test]
    fn finds_a_string_inside_a_tracked_file() {
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let hits = search(&repo, "needle", 50).unwrap();

        let content: Vec<&Hit> = hits.iter().filter(|h| h.kind == Kind::Content).collect();
        assert_eq!(content.len(), 1, "one tracked file contains it");
        assert_eq!(content[0].path, "alpha.rs");
        assert_eq!(content[0].line, Some(1));
        assert!(content[0].text.contains("needle"));
    }

    #[test]
    fn ignored_files_are_not_searched() {
        // The reason this uses git grep rather than a directory walk: the same
        // string sits in ignored/secret.rs and must not appear.
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let hits = search(&repo, "needle", 50).unwrap();
        assert!(
            !hits.iter().any(|h| h.path.contains("ignored")),
            "gitignored paths must stay out of results"
        );
    }

    #[test]
    fn finds_a_file_by_name() {
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let hits = search(&repo, "beta", 50).unwrap();
        assert!(hits
            .iter()
            .any(|h| h.kind == Kind::Path && h.path == "beta.txt"));
    }

    #[test]
    fn finds_a_commit_by_message() {
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let hits = search(&repo, "unrelated", 50).unwrap();

        let msgs: Vec<&Hit> = hits.iter().filter(|h| h.kind == Kind::Message).collect();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "unrelated change");
        assert!(msgs[0].commit.is_some());
    }

    #[test]
    fn search_is_case_insensitive_in_every_category() {
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let hits = search(&repo, "NEEDLE", 50).unwrap();
        assert!(hits.iter().any(|h| h.kind == Kind::Content));

        let by_name = search(&repo, "ALPHA", 50).unwrap();
        assert!(by_name.iter().any(|h| h.kind == Kind::Path));
    }

    #[test]
    fn a_query_with_regex_characters_is_taken_literally() {
        // A search box is not a regex prompt. Without --fixed-strings this
        // would be an "unmatched (" error rather than a search.
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        assert!(search(&repo, "needle(", 50).is_ok());
        assert!(search(&repo, "[unclosed", 50).is_ok());
    }

    #[test]
    fn no_matches_is_an_empty_list_not_an_error() {
        // `git grep` exits 1 when nothing matched.
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        assert!(search(&repo, "zzzznotpresent", 50).unwrap().is_empty());
    }

    #[test]
    fn an_empty_query_searches_nothing() {
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        assert!(search(&repo, "", 50).unwrap().is_empty());
        assert!(search(&repo, "   ", 50).unwrap().is_empty());
    }

    #[test]
    fn results_are_capped_per_category() {
        let dir = searchable();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let hits = search(&repo, "fn", 1).unwrap();
        assert!(hits.iter().filter(|h| h.kind == Kind::Content).count() <= 1);
    }

    #[test]
    fn a_path_containing_a_colon_still_parses() {
        // --null separates every field, so a colon in a path is just a
        // character. An earlier version split the line number off with a colon
        // and found nothing at all.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(p)
                .output()
                .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.name", "Fixture"]);
        run(&["config", "user.email", "fixture@example.invalid"]);

        std::fs::write(p.join("od:d.txt"), "the needle\n").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "odd name"]);

        let repo = crate::Repo::open(p).unwrap();
        let hits = search(&repo, "needle", 10).unwrap();
        let content: Vec<&Hit> = hits.iter().filter(|h| h.kind == Kind::Content).collect();

        assert_eq!(content.len(), 1);
        assert_eq!(content[0].path, "od:d.txt");
        assert_eq!(content[0].line, Some(1));
        assert_eq!(content[0].text, "the needle");
    }
}
