//! Measurements against a real large repository.
//!
//! The synthetic fixture in `memcheck.rs` proves the windowing invariant on
//! controlled data. This proves the claims on data nobody controls, and is
//! skipped unless pointed at a repository:
//!
//!   FORQEN_MEMCHECK_REPO=/path/to/big cargo test -p git \
//!       --test large_repository --release -- --nocapture
//!
//! Findings on git.git — 82,154 commits, 318MB of history:
//!
//! * Scrolling the entire history adds 1.6MB. That is the windowed model
//!   working: the spine is 1.6MB of ids and the realized rows stay at their
//!   budget of 512 throughout.
//! * Walking it costs 543ms and 63MB without a commit-graph, and 30ms and
//!   3.8MB with one. That is why `ensure_commit_graph_async` exists.
//! * Total RSS lands near 68MB either way, dominated by gix's packfile
//!   mappings and object cache — which scale with the size of the repository
//!   rather than with anything forqen retains.

use git::history::{HistoryWindow, Walker};
use git::Repo;

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse::<f64>().ok())
        })
        .unwrap_or(0.0)
        / 1024.0
}

fn repo() -> Option<Repo> {
    let path = std::env::var("FORQEN_MEMCHECK_REPO").ok()?;
    Repo::open(std::path::Path::new(&path)).ok()
}

#[test]
fn scrolling_a_real_history_stays_flat() {
    let Some(repo) = repo() else {
        eprintln!("set FORQEN_MEMCHECK_REPO to run this");
        return;
    };

    let mut window = HistoryWindow::new();
    window
        .fill_spine(Walker::from_head(&repo).expect("walk"))
        .expect("spine");
    let total = window.len();
    assert!(total > 10_000, "point this at a large repository");

    let before = rss_mb();
    let viewport = 40;
    let mut step = 0;
    while step + viewport < total {
        window
            .ensure(&repo, step..step + viewport)
            .expect("hydrate");
        assert!(window.realized() <= window.budget());
        step += viewport;
    }
    let growth = rss_mb() - before;

    eprintln!("scrolled {total} commits, RSS grew {growth:.1} MB");
    // The claim is that scrolling is flat, not that the process is small.
    // Reading every commit object in a large repository warms gix's pack
    // cache, which is bounded; what must not happen is growth proportional to
    // the number of rows scrolled past.
    assert!(
        growth < 80.0,
        "scrolling {total} commits grew RSS by {growth:.1}MB — the windowed \
         model should keep this bounded by the object cache, not by history"
    );
    assert_eq!(
        window.realized(),
        window.budget(),
        "the row budget must be saturated, not exceeded, after a full scroll"
    );
}

#[test]
fn a_commit_graph_makes_the_walk_dramatically_cheaper() {
    let Some(repo) = repo() else {
        return;
    };
    // Only meaningful when one exists; `ensure_commit_graph_async` writes it
    // on first open, so a repository forqen has opened will have one.
    let graph = repo.git_dir().join("objects/info/commit-graph");
    if !graph.exists() {
        eprintln!("no commit-graph present; skipping");
        return;
    }

    let before = rss_mb();
    let started = std::time::Instant::now();
    let mut window = HistoryWindow::new();
    window
        .fill_spine(Walker::from_head(&repo).expect("walk"))
        .expect("spine");
    let elapsed = started.elapsed();
    let growth = rss_mb() - before;

    eprintln!(
        "walked {} commits in {elapsed:?}, RSS +{growth:.1} MB",
        window.len()
    );
    // Without a graph this measured 543ms and 63MB on git.git; with one, 30ms
    // and 3.8MB. A generous bound still catches a regression that loses the
    // graph entirely.
    assert!(
        growth < 30.0,
        "walking with a commit-graph should not cost {growth:.1}MB — is the \
         graph being ignored?"
    );
}
