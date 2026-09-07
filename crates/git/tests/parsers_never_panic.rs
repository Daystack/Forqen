//! Every parser must survive hostile input without panicking.
//!
//! These read the output of a `git` subprocess. That output is normally
//! well-formed, but it is not a contract: a git version can change a field, a
//! locale can translate a word, a repository can contain a path with a newline
//! in it, and a truncated pipe can cut a record in half. A panic in any of
//! them takes the whole window down while someone is mid-commit.
//!
//! Not a fuzzer — a fixed corpus of the shapes that actually break parsers,
//! run against every parser that accepts free-form text.

use git::{diff, search, worktree};

/// Inputs chosen to break the specific assumptions these parsers make.
fn hostile_corpus() -> Vec<String> {
    let mut out: Vec<String> = vec![
        String::new(),
        "\n".into(),
        "\0".into(),
        "\0\0\0".into(),
        // Truncated mid-record: a pipe closed early.
        "diff --git a/x b/x\n--- a/x\n".into(),
        "@@ -1,2 +1,2 @@".into(),
        "@@".into(),
        "@@ -a,b +c,d @@".into(),
        // Counts that disagree with the body.
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,999 +1,999 @@\n one\n".into(),
        // Negative and huge numbers where a count is expected.
        "@@ --1,-1 +-1,-1 @@".into(),
        "@@ -99999999999999999999,1 +1,1 @@".into(),
        // A path containing the separators the format uses.
        "worktree /a\nb\nHEAD 1\n".into(),
        "worktree /a\0b\nHEAD 1\n".into(),
        // Invalid UTF-8 replacement characters, as from_utf8_lossy produces.
        "\u{fffd}\u{fffd}\n".into(),
        // Very long single line.
        format!("{}\n", "x".repeat(100_000)),
        // Deep repetition, to catch anything quadratic or recursive.
        "@@ -1,1 +1,1 @@\n".repeat(5_000),
    ];

    // Every prefix of a realistic diff: catches parsers that assume a record
    // is complete once it has started.
    let realistic = "diff --git a/src/main.rs b/src/main.rs\n\
                     index 1234567..89abcde 100644\n\
                     --- a/src/main.rs\n\
                     +++ b/src/main.rs\n\
                     @@ -1,3 +1,4 @@\n\
                      fn main() {\n\
                     -    old();\n\
                     +    new();\n\
                     +    more();\n\
                      }\n";
    for i in 0..realistic.len() {
        out.push(realistic[..i].to_string());
    }

    out
}

#[test]
fn the_diff_parser_survives_hostile_input() {
    for input in hostile_corpus() {
        // The contract is only "does not panic"; whatever it returns for
        // nonsense is nonsense, and that is fine.
        let files = diff::parse(&input);

        // What it does return must still be self-consistent, or downstream
        // code indexing by hunk and line will panic instead.
        for f in &files {
            for h in &f.hunks {
                assert!(
                    h.lines.len() < 10_000_000,
                    "a hunk claimed an implausible number of lines"
                );
            }
        }
    }
}

#[test]
fn the_worktree_parser_survives_hostile_input() {
    for input in hostile_corpus() {
        let list = worktree::parse_list_for_test(&input);
        for w in &list {
            // A record without a path is meaningless and must not be emitted.
            assert!(
                !w.path.as_os_str().is_empty(),
                "a worktree with no path was produced from {input:?}"
            );
        }
    }
}

#[test]
fn hunk_headers_with_absurd_counts_do_not_allocate_wildly() {
    // `@@ -1,999999999 +1,999999999 @@` must not make the parser try to
    // reserve a billion lines: the count is a claim, not a fact.
    let input = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,4294967295 +1,4294967295 @@\n one\n";
    let files = diff::parse(input);
    for f in &files {
        for h in &f.hunks {
            assert_eq!(
                h.lines.len(),
                1,
                "the body has one line regardless of what the header claims"
            );
        }
    }
}

#[test]
fn search_survives_a_repository_that_does_not_exist() {
    // The engine shells out; a missing path must be an error, not a panic.
    let dir = tempfile::tempdir().unwrap();
    let Ok(repo) = git::Repo::open(dir.path()) else {
        return; // not a repository, which is the expected outcome
    };
    let _ = search::search(&repo, "anything", 10);
}
