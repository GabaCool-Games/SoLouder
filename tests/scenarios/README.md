# Parity scenarios

Each `.scn` file is one scenario: a script of engine calls that both the C++
reference runner (`reference/tests/parity_runner.cpp`) and the Rust runner
(`crates/solouder-parity`) execute. The Rust test compares the two results.

```sh
python scripts/run_parity.py      # build the C++ runner, render, compare
cargo test -p solouder-parity     # compare against existing C++ output only
```

Without C++ output in `build/parity/cpp/`, `cargo test` still runs every
scenario through Rust (catching parse errors and unsupported operations) and
skips the comparison. `run_parity.py` sets `SOLOUDER_REQUIRE_PARITY=1`, which
turns missing C++ output into a failure.

## Format

- One step per line: an operation name, then `key=value` arguments separated by
  spaces. Keys may appear at most once per step.
- Blank lines and lines starting with `#` are ignored. Start each file with a
  comment saying what it checks.
- Unknown operations and unknown keys are errors in both runners, so a scenario
  can only use behavior that both sides implement.

## Results

Each runner produces:

- **Samples:** every `mix` appends its interleaved `f32` frames. The C++ runner
  writes them little-endian to `<name>.f32`.
- **Log:** one text line per step that returns a value, written to `<name>.log`.

Samples must match within the scenario's tolerance (maximum absolute
difference, default `1e-6`). Log lines must match exactly.

## Operations

| Operation | Arguments | Effect | Log line |
|---|---|---|---|
| `tolerance` | `abs` (float) | Maximum absolute sample difference for this scenario. Rust side only. | — |
| `init` | `channels` (default 2), `rate` (0 = default 44100), `buffer` (0 = default 2048) | `Soloud::init` with the null driver and `CLIP_ROUNDOFF`. Re-initializing replaces the previous engine state. | `init <SoLoud error code>` |
| `mix` | `frames` (0 to 4096) | `Soloud::mix` for that many frames. Needs a successful `init`. | — |

`mix` is limited to 4096 frames because the C++ `Soloud::mix` writes past its
scratch buffer beyond that (the smallest scratch size). The Rust mixer accepts
any length.
