//! Runs the search engine against whatever repository the tests are run from.
//! Skips silently elsewhere, so it never fails on a machine without one.

use git::search::{search, Kind};

#[test]
fn searching_the_current_repository_returns_all_three_categories() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let Ok(repo) = git::Repo::open(here) else {
        return;
    };

    let hits = search(&repo, "worktree", 20).unwrap();
    if hits.is_empty() {
        return; // a checkout without this term; nothing to assert
    }

    for kind in [Kind::Content, Kind::Path, Kind::Message] {
        let found: Vec<_> = hits.iter().filter(|h| h.kind == kind).collect();
        eprintln!("--- {kind:?}: {} hits", found.len());
        for h in found.iter().take(3) {
            match h.kind {
                Kind::Content => eprintln!("  {}:{}  {}", h.path, h.line.unwrap_or(0), h.text),
                Kind::Path => eprintln!("  {}", h.path),
                Kind::Message => eprintln!(
                    "  {} {}",
                    h.commit.map(|c| c.short()).unwrap_or_default(),
                    h.text
                ),
            }
        }
    }

    assert!(
        hits.iter().any(|h| h.kind == Kind::Content),
        "the term appears in tracked source"
    );
    assert!(
        !hits.iter().any(|h| h.path.starts_with("target/")),
        "build output must never be searched"
    );
}
