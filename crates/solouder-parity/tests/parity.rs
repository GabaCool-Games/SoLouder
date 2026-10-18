//! Runs every scenario in `tests/scenarios/` through Rust and, when C++
//! reference output exists, compares the two.
//!
//! C++ output is read from `SOLOUDER_PARITY_DIR` (default
//! `build/parity/cpp`, relative to the repository root). With `SOLOUDER_REQUIRE_PARITY=1`, missing C++ output
//! is a failure; `scripts/run_parity.py` sets both. `SOLOUDER_SCENARIO`, a
//! comma-separated list of scenario names, runs only those.
//! `SOLOUDER_PARITY_PROFILE` names a profile in
//! `tests/scenarios/tolerances.txt` whose declared tolerances apply, for
//! output from another platform's reference build or Rust on another
//! platform; unlisted scenarios must still match exactly.

use std::path::{Path, PathBuf};

use std::collections::HashMap;

use solouder_parity::{compare_within, load_golden, parse, profiles, run};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scenario_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "scn"))
        .filter(|path| {
            let only = std::env::var("SOLOUDER_SCENARIO").unwrap_or_default();
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            only.is_empty() || only.split(',').any(|name| name.trim() == stem)
        })
        .collect();
    files.sort();
    files
}

/// The tolerances of `SOLOUDER_PARITY_PROFILE`, if set.
fn declared_tolerances(root: &Path) -> HashMap<String, f32> {
    let Ok(profile) = std::env::var("SOLOUDER_PARITY_PROFILE") else {
        return HashMap::new();
    };
    let path = root.join("tests/scenarios/tolerances.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    profiles::tolerances(&text, &profile)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn scenarios_match_the_cpp_reference() {
    let root = repository_root();
    // Relative paths are taken from the repository root, not the crate.
    let golden_dir = std::env::var_os("SOLOUDER_PARITY_DIR")
        .map(|dir| root.join(dir))
        .unwrap_or_else(|| root.join("build/parity/cpp"));
    let require = std::env::var("SOLOUDER_REQUIRE_PARITY").is_ok_and(|value| value == "1");
    let declared = declared_tolerances(&root);

    let files = scenario_files(&root.join("tests/scenarios"));
    assert!(
        !files.is_empty(),
        "no matching .scn files in tests/scenarios"
    );

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
            Ok(Some(golden)) => {
                let declared = declared.get(&name).copied().unwrap_or(0.0);
                let tolerance = rendered.tolerance.max(declared);
                match compare_within(&rendered, &golden, tolerance) {
                    Ok(stats) => println!(
                        "{name}: {} samples, {} differ, max abs diff {:e}, rms {:e}, {} subnormal vs zero{}",
                        stats.samples,
                        stats.differing,
                        stats.max_abs,
                        stats.rms,
                        stats.flushed,
                        if declared > 0.0 {
                            format!(" (declared {declared:e})")
                        } else {
                            String::new()
                        }
                    ),
                    Err(message) => failures.push(format!("{name}: {message}")),
                }
            }
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
