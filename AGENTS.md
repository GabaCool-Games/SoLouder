# Notes for coding agents

SoLouder is a Rust/Bevy port of the SoLoud C++ audio engine. Read
[docs/PORTING.md](docs/PORTING.md) (parity target, reference rules, tracked
defects, milestones) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
(workspace, vertical slices, real-time rules) before changing code.

## C++ reference

- The C++ source is **not committed**. Run `python scripts/fetch_reference.py`
  to create `reference/soloud/` (upstream at a pinned commit plus
  `reference/patches/`). Read it freely; it is the behavioral reference.
- Never edit `reference/soloud/` to change behavior. Reference changes are
  commits on a SoLoud branch, exported with
  `python scripts/export_reference_patches.py <fork>`, each with a failing-then-
  passing test in `reference/tests/regressions.cpp`.
- `python scripts/test_reference.py` builds and runs those tests (CMake, MSVC
  on Windows).

## Rules that are easy to miss

- Every `.rs` file stays at or below 400 physical lines
  (`python scripts/check_rust_file_lines.py`). Split by responsibility; do not
  compress code to fit.
- Organize by feature slice (`features/playback/`, ...), not by layer.
- No allocation, blocking, locking, logging, or ECS access in the audio
  callback.
- Known reference defects are fixed in Rust, never reproduced for parity.
- Rust files translated from SoLoud start with the attribution header from
  [CONTRIBUTING.md](CONTRIBUTING.md#marking-ported-code).
- Update `docs/parity-ledger.csv` tracking columns when a behavior is ported;
  `python scripts/gen_parity_ledger.py --check` must pass.
