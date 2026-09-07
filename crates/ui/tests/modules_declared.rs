//! Every source file in the workspace must actually be part of its crate.
//!
//! Rust ignores a `.rs` file nothing declares: it is not compiled, produces no
//! warning, and its code never runs. `crates/ui/src/search.rs` sat that way
//! through a commit claiming search worked — the button existed, its keyboard
//! shortcut existed, and activating it did nothing at all.
//!
//! Lives in `ui` but checks the whole workspace, because one test that cannot
//! be forgotten beats six that can.

use std::collections::HashSet;
use std::path::Path;

#[test]
fn every_source_file_is_declared_in_its_crate_root() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");

    let mut undeclared = Vec::new();

    for entry in std::fs::read_dir(workspace.join("crates")).expect("crates/") {
        let crate_dir = entry.expect("entry").path();
        let src = crate_dir.join("src");
        if !src.is_dir() {
            continue;
        }

        // A crate root is lib.rs or main.rs; a binary crate may have both.
        let roots: Vec<_> = ["lib.rs", "main.rs"]
            .iter()
            .map(|f| src.join(f))
            .filter(|p| p.is_file())
            .collect();
        if roots.is_empty() {
            continue;
        }

        let declared: HashSet<String> = roots
            .iter()
            .filter_map(|r| std::fs::read_to_string(r).ok())
            .flat_map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter_map(|l| {
                        l.strip_prefix("pub mod ")
                            .or_else(|| l.strip_prefix("mod "))
                    })
                    .filter_map(|l| l.strip_suffix(';'))
                    .map(|m| m.trim().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();

        for file in std::fs::read_dir(&src).expect("src/") {
            let path = file.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if matches!(stem, "lib" | "main") {
                continue;
            }
            if !declared.contains(stem) {
                undeclared.push(format!(
                    "{}/src/{stem}.rs",
                    crate_dir.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }

        // Directory modules count too: `foo/mod.rs` needs `mod foo;` as much as
        // `foo.rs` does.
        for file in std::fs::read_dir(&src).expect("src/") {
            let path = file.expect("entry").path();
            if !path.is_dir() || !path.join("mod.rs").is_file() {
                continue;
            }
            let Some(stem) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !declared.contains(stem) {
                undeclared.push(format!(
                    "{}/src/{stem}/mod.rs",
                    crate_dir.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
    }

    undeclared.sort();
    assert!(
        undeclared.is_empty(),
        "these files exist but are not declared in their crate root, so they \
         are not compiled and their code never runs: {undeclared:?}"
    );
}
