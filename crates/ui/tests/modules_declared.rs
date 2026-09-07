//! Every source file must actually be part of the crate.
//!
//! Rust ignores a `.rs` file nothing declares, so a module added without its
//! `pub mod` line is silently dead — it compiles, the tests pass, and the
//! feature simply does not exist. That happened twice in one sitting, both
//! times invisible until a dialog failed to open.

use std::collections::HashSet;

#[test]
fn every_source_file_is_declared_in_lib_rs() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("lib.rs");

    let declared: HashSet<String> = lib
        .lines()
        .map(str::trim)
        .filter_map(|l| {
            l.strip_prefix("pub mod ")
                .or_else(|| l.strip_prefix("mod "))
        })
        .filter_map(|l| l.strip_suffix(';'))
        .map(|m| m.trim().to_string())
        .collect();

    let mut undeclared = Vec::new();
    for entry in std::fs::read_dir(&src).expect("read src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem == "lib" {
            continue;
        }
        if !declared.contains(stem) {
            undeclared.push(stem.to_string());
        }
    }

    assert!(
        undeclared.is_empty(),
        "these files exist but are not declared in lib.rs, so they are not \
         compiled and their code never runs: {undeclared:?}"
    );
}
