//! Runs every scenario in `tests/scenarios/` through Rust and, when C++
//! reference output exists, compares the two.
//!
//! C++ output is read from `SOLOUDER_PARITY_DIR` (default
//! `build/parity/cpp`, relative to the repository root). With `SOLOUDER_REQUIRE_PARITY=1`, missing C++ output
//! is a failure; `scripts/run_parity.py` sets both.

use std::path::{Path, PathBuf};

use solouder_parity::{compare, load_golden, parse, run};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scenario_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "scn"))
        .collect();
    files.sort();
    files
}

#[test]
fn scenarios_match_the_cpp_reference() {
    let root = repository_root();
    // Relative paths are taken from the repository root, not the crate.
    let golden_dir = std::env::var_os("SOLOUDER_PARITY_DIR")
        .map(|dir| root.join(dir))
        .unwrap_or_else(|| root.join("build/parity/cpp"));
    let require = std::env::var("SOLOUDER_REQUIRE_PARITY").is_ok_and(|value| value == "1");

    let files = scenario_files(&root.join("tests/scenarios"));
    assert!(!files.is_empty(), "no .scn files in tests/scenarios");

    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    for path in &files {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(path).expect("scenario text");
        let rendered = match parse(&name, &text).and_then(|scenario| run(&scenario)) {
            Ok(rendered) => rendered,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        match load_golden(&golden_dir, &name) {
            Ok(Some(golden)) => match compare(&rendered, &golden) {
                Ok(stats) => println!(
                    "{name}: {} samples, max abs diff {:e}, rms {:e}, {} subnormal vs zero",
                    stats.samples, stats.max_abs, stats.rms, stats.flushed
                ),
                Err(message) => failures.push(format!("{name}: {message}")),
            },
            Ok(None) if require => {
                failures.push(format!("{name}: no C++ output in {}", golden_dir.display()))
            }
            Ok(None) => skipped.push(name),
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    if !skipped.is_empty() {
        println!(
            "compared nothing for {} scenario(s) without C++ output; run python scripts/run_parity.py",
            skipped.len()
        );
    }
    assert!(
        failures.is_empty(),
        "parity failures:\n{}",
        failures.join("\n")
    );
}
