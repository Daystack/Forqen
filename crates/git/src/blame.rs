//! Blame: who last touched each line, and when.
//!
//! Read from `git blame --porcelain`. The default output interleaves the
//! author, date and line content on one line and pads them into columns, which
//! is unparseable the moment an author's name contains two spaces. The
//! porcelain format emits a header block per line group and repeats nothing,
//! so the same commit's metadata appears once and subsequent lines reference
//! it by id — which is also why a parser has to remember what it has seen.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::{GitError, ObjectId, Repo};

/// One line of a file, with the commit that last changed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    pub line_no: u32,
    pub id: ObjectId,
    pub author: String,
    /// Committer timestamp, seconds since the epoch.
    pub time: i64,
    pub summary: String,
    pub content: String,
    /// True when this line comes from an uncommitted change — git reports the
    /// all-zero id for those, and calling that "not committed" is clearer than
    /// showing forty zeroes.
    pub uncommitted: bool,
}

/// Metadata accumulated per commit while parsing.
#[derive(Default, Clone)]
struct CommitInfo {
    author: String,
    time: i64,
    summary: String,
}

/// Blame a file at its current state.
pub fn blame(repo: &Repo, path: &Path) -> Result<Vec<BlameLine>, GitError> {
    let workdir = repo.workdir().unwrap_or_else(|| repo.git_dir());
    let out = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["blame", "--porcelain", "--"])
        .arg(path)
        .output()?;

    if !out.status.success() {
        return Err(GitError::Walk(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }

    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse `git blame --porcelain`.
///
/// The shape is: a header line `<sha> <orig-line> <final-line> [<count>]`,
/// then zero or more `key value` lines, then one line beginning with a tab
/// holding the file content. Metadata is emitted only the first time a commit
/// appears; later groups repeat the sha alone, so the parser carries a map.
fn parse(text: &str) -> Vec<BlameLine> {
    let mut seen: HashMap<String, CommitInfo> = HashMap::new();
    let mut out = Vec::new();

    let mut current_sha: Option<String> = None;
    let mut current_line_no: u32 = 0;
    let mut pending = CommitInfo::default();

    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            let Some(sha) = current_sha.take() else {
                continue;
            };

            // First sighting carries the metadata; later ones reference it.
            let info = if pending.author.is_empty() && seen.contains_key(&sha) {
                seen[&sha].clone()
            } else {
                seen.insert(sha.clone(), pending.clone());
                pending.clone()
            };
            pending = CommitInfo::default();

            let Some(id) = parse_hex(&sha) else { continue };
            out.push(BlameLine {
                line_no: current_line_no,
                id,
                author: info.author,
                time: info.time,
                summary: info.summary,
                content: content.to_string(),
                uncommitted: sha.chars().all(|c| c == '0'),
            });
            continue;
        }

        let (key, value) = match line.split_once(' ') {
            Some(kv) => kv,
            None => (line, ""),
        };

        // A header line starts with a 40-character sha.
        if key.len() == 40 && key.chars().all(|c| c.is_ascii_hexdigit()) {
            current_sha = Some(key.to_string());
            // `<orig-line> <final-line> [<count>]` — the second is the line
            // number in the file as it stands, which is what to display.
            current_line_no = value
                .split_whitespace()
                .nth(1)
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            continue;
        }

        match key {
            "author" => pending.author = value.to_string(),
            "committer-time" => pending.time = value.trim().parse().unwrap_or(0),
            "summary" => pending.summary = value.to_string(),
            _ => {}
        }
    }

    out
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

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn metadata_is_carried_forward_to_later_groups() {
        // The porcelain format emits author/summary only the first time a
        // commit appears; a parser that forgets shows blank authors for most
        // of a file.
        let text = format!(
            "{A} 1 1 2\n\
             author Alice\n\
             committer-time 1600000000\n\
             summary first change\n\
             \tline one\n\
             {A} 2 2\n\
             \tline two\n\
             {B} 3 3 1\n\
             author Bob\n\
             committer-time 1600000100\n\
             summary second change\n\
             \tline three\n"
        );
        let lines = parse(&text);
        assert_eq!(lines.len(), 3);

        assert_eq!(lines[0].author, "Alice");
        assert_eq!(lines[0].summary, "first change");
        assert_eq!(
            lines[1].author, "Alice",
            "the second line of the same commit must inherit its metadata"
        );
        assert_eq!(lines[1].summary, "first change");
        assert_eq!(lines[2].author, "Bob");
    }

    #[test]
    fn line_numbers_come_from_the_final_file_not_the_original() {
        // Header is `<sha> <orig-line> <final-line> [<count>]`; a moved block
        // has different values, and the display wants the current file.
        let text = format!("{A} 47 3 1\nauthor A\ncommitter-time 1\nsummary s\n\tmoved line\n");
        assert_eq!(parse(&text)[0].line_no, 3);
    }

    #[test]
    fn content_is_taken_verbatim_after_the_tab() {
        // Source lines are frequently indented; only the leading tab git adds
        // may be stripped.
        let text = format!("{A} 1 1 1\nauthor A\ncommitter-time 1\nsummary s\n\t    indented();\n");
        assert_eq!(parse(&text)[0].content, "    indented();");
    }

    #[test]
    fn an_uncommitted_line_is_marked_rather_than_showing_zeroes() {
        let zeros = "0".repeat(40);
        let text = format!(
            "{zeros} 1 1 1\nauthor Not Committed Yet\ncommitter-time 0\nsummary x\n\tnew work\n"
        );
        let line = &parse(&text)[0];
        assert!(line.uncommitted);
        assert_eq!(line.content, "new work");
    }

    #[test]
    fn an_empty_or_malformed_blame_yields_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("garbage\nmore garbage\n").is_empty());
    }

    // --- against a real repository -------------------------------------------

    #[test]
    fn blames_a_real_file_and_attributes_every_line() {
        let dir = fixture(3);
        let repo = crate::Repo::open(dir.path()).unwrap();

        let lines = blame(&repo, Path::new("f.txt")).unwrap();
        assert!(!lines.is_empty());
        for l in &lines {
            assert_eq!(l.author, "Fixture");
            assert!(
                !l.summary.is_empty(),
                "every line names the commit that set it"
            );
            assert!(l.time > 0);
            assert!(!l.uncommitted);
        }
        assert_eq!(lines[0].line_no, 1);
    }

    #[test]
    fn a_working_tree_edit_is_reported_as_uncommitted() {
        let dir = fixture(2);
        let repo = crate::Repo::open(dir.path()).unwrap();
        std::fs::write(dir.path().join("f.txt"), "edited but not committed\n").unwrap();

        let lines = blame(&repo, Path::new("f.txt")).unwrap();
        assert!(
            lines.iter().any(|l| l.uncommitted),
            "an edit that is not committed has no commit to blame"
        );
    }

    #[test]
    fn blaming_a_missing_file_reports_gits_message() {
        let dir = fixture(1);
        let repo = crate::Repo::open(dir.path()).unwrap();
        let err = blame(&repo, Path::new("nope.txt")).unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
