# SoLouder

SoLouder is a native Rust port of the [SoLoud](https://github.com/jarikomppa/soloud)
audio engine, with a Bevy plugin, carrying the fixes and extensions developed
for the game GoreGrind.

**SoLouder is an altered version of SoLoud. It is not the original SoLoud and
is not affiliated with or endorsed by SoLoud's authors.** SoLoud was created by
Jari Komppa; see [UPSTREAM_AUTHORS.md](UPSTREAM_AUTHORS.md).

## Status

Early. The Rust crates do not exist yet. What is here today:

- **A patched C++ reference.** Upstream SoLoud, pinned in
  `reference/upstream.json`, plus 14 patches in `reference/patches/`. They fix
  upstream crashes, hangs, and memory-safety bugs that were never merged
  upstream, including the WavStream seek heap corruption
  ([#405](https://github.com/jarikomppa/soloud/issues/405)). Every behavioral
  fix has a C++ regression test in `reference/tests/`.
- **A parity ledger.** [docs/parity-ledger.csv](docs/parity-ledger.csv) lists
  every behavior of SoLoud's C API, 208 of them. That is the 1:1 target.
- **Design and porting guides.** [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
  and [docs/PORTING.md](docs/PORTING.md).

## Getting started

Requirements: Python 3.10+ and Git. Building the C++ reference tests also
needs CMake and a C++ compiler (Visual Studio 2022 on Windows).

```sh
python scripts/fetch_reference.py    # creates the git-ignored reference/soloud
python scripts/test_reference.py     # builds and runs the C++ regression tests
python scripts/gen_parity_ledger.py --check
python scripts/check_rust_file_lines.py
```

## Repository layout

| Path | Contents |
|---|---|
| `docs/` | Architecture, porting guide, parity ledger |
| `reference/upstream.json` | Upstream URL and pinned commit |
| `reference/patches/` | Patches applied to upstream, in order |
| `reference/tests/` | C++ regression tests for the patches (CMake) |
| `reference/soloud/` | Git-ignored C++ checkout created by `scripts/fetch_reference.py` |
| `scripts/` | Reference fetch/export/test, ledger generator, Rust file-size check |

## License

zlib/libpng, the same as SoLoud ([LICENSE](LICENSE)). Third-party code is
listed in [THIRD_PARTY.md](THIRD_PARTY.md). Contributions: see
[CONTRIBUTING.md](CONTRIBUTING.md) and [CONTRIBUTORS.md](CONTRIBUTORS.md).
