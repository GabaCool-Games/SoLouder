# Contributing to SoLouder

## Licensing

SoLouder is licensed under zlib/libpng, like SoLoud (see [LICENSE](LICENSE)).
By submitting a contribution you agree that it is provided under that license.
Do not submit code you cannot license that way. Code copied from elsewhere
must be public domain, CC0, Unlicense, WTFPL, zlib/libpng, or similarly
permissive, and its origin and notice must be added to
[THIRD_PARTY.md](THIRD_PARTY.md).

Add your name to [CONTRIBUTORS.md](CONTRIBUTORS.md) in your first pull request.

## Marking ported code

The zlib license requires altered versions to be plainly marked. Start every
Rust file translated from SoLoud with a header naming the source, for example:

```rust
// Ported from SoLoud src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.
```

## How work is done

- Read [docs/PORTING.md](docs/PORTING.md) and
  [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) first. They define the parity
  target, the ownership/real-time rules, and the 400-line limit for Rust files.
- Get the C++ reference with `python scripts/fetch_reference.py`; it is
  git-ignored. Change the reference only through patches in
  `reference/patches/` (see [reference/README.md](reference/README.md)).
- Keep commits small and focused. Known reference defects are fixed, never
  reproduced for parity; see the tracking section of PORTING.md.

Before sending a change, run:

```sh
python scripts/check_all.py --reference --parity
```

That runs the file-size and ledger checks, `cargo fmt`, `cargo clippy` and
`cargo test`, the C++ reference tests, and the C++/Rust scenario comparison.
Without CMake and a C++ compiler, drop `--reference --parity`; CI runs them.

A behavior is ported when a scenario in `tests/scenarios/` exercises it and
matches the C++ reference. Add the scenario with the code, and set the
behavior's `status` and `scenarios` columns in `docs/parity-ledger.csv`.
