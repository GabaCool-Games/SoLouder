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
| `init` | `channels` (default 2), `rate` (0 = default 44100), `buffer` (0 = default 2048), `clip` (`roundoff`, default, or `hard`) | A new engine: `Soloud::init` with the null driver and `CLIP_ROUNDOFF` or no flags. Forgets all sounds and handles. | `init <code>` |
| `mix` | `frames` (0 to 4096) | `Soloud::mix` for that many frames. | — |
| `sound` | `name`, `frames`, `channels` (1), `rate` (44100), `pattern` (`dc`, `saw`, `noise`; default `dc`), `amp` (1), `period` (`saw`, 100), `seed` (`noise`, 1), `volume` (1), `looping` (0) | Generates planar samples (see below), loads them with `Wav::loadRawWave` (copied) and applies `setVolume`/`setLooping`. A failed load registers nothing. `channels * frames` is at most 4194304. | `sound <name> <code>` |
| `play` | `sound`, `as` (handle name), `volume` (-1 = the sound's), `pan` (0), `paused` (0) | `Soloud::play`; stores the handle under `as`. | `play <as> <handle>` |
| `stop` | `h` | `Soloud::stop`. | — |
| `stop_all` | — | `Soloud::stopAll`. | — |
| `set` | `h`, then any of `volume`, `pan`, `pause`, `looping`, `protect`, `delay` (frames), `speed` | Applied in that order: `setVolume`, `setPan`, `setPause`, `setLooping`, `setProtectVoice`, `setDelaySamples`, `setRelativePlaySpeed`. | `set <h> speed <code>` if `speed` is given |
| `global` | any of `volume`, `clip_scale`, `resampler` (`point`, `linear`, `catmullrom`), `max_active` | Applied in that order: `setGlobalVolume`, `setPostClipScaler`, `setMainResampler`, `setMaxActiveVoiceCount`. | `global max_active <code>` if `max_active` is given |
| `query` | `h` | Reads the voice's state; a stopped voice reports the C API's defaults. | `query <h> valid= paused= looping= protect= loops= volume= overall= pan= speed= rate= time= pos=` (see below) |
| `counts` | — | `getVoiceCount` and `getActiveVoiceCount` (which updates the active set). | `counts voices=<n> active=<n>` |

Every operation except `tolerance` and `init` needs a successful `init`.
Unknown sound or handle names are scenario errors. `<code>` is the SoLoud
error code (0 = no error, 1 = invalid parameter).

Argument values:

- Numbers (`rate`, `amp`, `volume`, `pan`, `speed`, `clip_scale`) are decimal:
  an optional `-`, digits, and optionally `.` and more digits. Counts are
  unsigned integers; flags are `0` or `1`.
- In `query`, `volume`, `overall`, `pan`, `speed` and `rate` are the `f32` bits
  in 8 hex digits, and `time` and `pos` the `f64` bits in 16, so the log
  comparison is exact.

Patterns, written channel after channel (`c` is the channel, `f` the frame),
all multiplied by `amp`:

- `dc`: 1.
- `saw`: `((f + 7c) % period) / period * 2 - 1`, in `f32`.
- `noise`: one xorshift32 generator (`x ^= x << 13; x ^= x >> 17; x ^= x << 5`)
  seeded with `seed` and continued across channels; each sample is
  `(x >> 8) / 2^23 - 1`.

`mix` is limited to 4096 frames because the C++ `Soloud::mix` writes past its
scratch buffer beyond that (the smallest scratch size). The Rust mixer accepts
any length.
