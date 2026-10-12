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

- **Samples:** every `mix` appends its interleaved `f32` frames, and every
  `samples` a sound's decoded data. The C++ runner writes them little-endian to
  `<name>.f32`.
- **Log:** one text line per step that returns a value, written to `<name>.log`.

Samples must have identical bits, except that zero and subnormal values
all count as zero (the C++ build flushes subnormals). A scenario may declare
a `tolerance` (maximum absolute difference) instead. Log lines must match
exactly, and a scenario must render at least one sample.

## Operations

| Operation | Arguments | Effect | Log line |
|---|---|---|---|
| `tolerance` | `abs` (float) | Maximum absolute sample difference for this scenario; 0 (the default) asks for identical bits. Rust side only. | — |
| `init` | `channels` (default 2), `rate` (0 = default 44100), `buffer` (0 = default 2048), `clip` (`roundoff`, default, or `hard`) | A new engine: `Soloud::init` with the null driver and `CLIP_ROUNDOFF` or no flags. Forgets all sounds and handles. | `init <code>` |
| `mix` | `frames` (0 to 4096) | `Soloud::mix` for that many frames. | — |
| `mix16` | `frames` (0 to 4096) | `Soloud::mixSigned16`; each 16-bit sample is appended as an `f32` of the same integer value. | — |
| `sound` | `name`, `frames`, `channels` (1), `rate` (44100), `pattern` (`dc`, `saw`, `noise`; default `dc`), `amp` (1), `period` (`saw`, 100), `seed` (`noise`, 1), `volume` (1), `looping` (0), `bits` (32), `loop_point` (0), `auto_stop` (1), `single` (0), `tick` (0), `kill` (0) | Generates planar samples (see below) and loads them with `Wav::loadRawWave` (copied). With `bits=8` or `16` (`amp` from -1 to 1) each sample `v` becomes `(unsigned char)(int)(v * 127 + 128)` or `(short)(v * 32767)` for `loadRawWave8` or `loadRawWave16`. Then applies `setVolume`, `setLooping`, `setLoopPoint`, `setAutoStop`, `setSingleInstance` and `setInaudibleBehavior(tick, kill)`. A failed load registers nothing; a successful one under an existing name deletes the old `Wav`, which stops its voices. `channels * frames` is at most 4194304. | `sound <name> <code>` |
| `load` | `name`, `file`, `volume` (1), `looping` (0) | Loads a fixture: `Wav::load` in C++, `solouder_decoders::decode_wav` in Rust, then `setVolume`/`setLooping`. A failed load registers nothing; replacing a name stops the old sound's voices, as for `sound`. | `load <name> <code>` |
| `samples` | `name` | Appends the sound's decoded samples, channel after channel, to the samples. | `samples <name> <channels> <frames> <rate>` (`rate` as `f32` bits in 8 hex digits) |
| `play` | `sound`, `as` (handle name), `volume` (-1 = the sound's), `pan` (0), `paused` (0), `clock` (seconds; not with `paused`), `background` (0; not with `pan` or `clock`) | `Soloud::play`, `Soloud::playClocked` when `clock` is given, or `Soloud::playBackground`; stores the handle under `as`. | `play <as> <handle>` |
| `sound_query` | `name` | `Wav::getLength` and `Soloud::countAudioSource`. | `sound_query <name> length=<f64 bits> count=<n>` |
| `stop_sound` | `name`, `source` (0) | `Soloud::stopAudioSource`, or `AudioSource::stop` with `source=1`. | — |
| `stop` | `h` | `Soloud::stop`. | — |
| `stop_all` | — | `Soloud::stopAll`. | — |
| `set` | `h`, then any of `volume`, `pan`, `pause`, `looping`, `protect`, `delay` (frames), `speed`, `loop_point`, `auto_stop`, `tick` and `kill`, `abs_l` and `abs_r`, `channel` and `channel_volume`, `rate`, `seek` (paired keys go together) | Applied in that order: `setVolume`, `setPan`, `setPause`, `setLooping`, `setProtectVoice`, `setDelaySamples`, `setRelativePlaySpeed`, `setLoopPoint`, `setAutoStop`, `setInaudibleBehavior`, `setPanAbsolute`, `setChannelVolume`, `setSamplerate`, `seek`. `h` may name a voice group. | `set <h> speed <code>` if `speed` is given, `set <h> seek <code>` if `seek` is |
| `global` | any of `volume`, `clip_scale`, `resampler` (`point`, `linear`, `catmullrom`), `max_active`, `pause_all` | Applied in that order: `setGlobalVolume`, `setPostClipScaler`, `setMainResampler`, `setMaxActiveVoiceCount`, `setPauseAll`. | `global max_active <code>` if `max_active` is given |
| `query` | `h` | Reads the voice's state; a stopped voice reports the C API's defaults, and a voice group its first voice. `info` is `getInfo(h, 0)`. | `query <h> valid= paused= looping= protect= loops= volume= overall= pan= speed= rate= time= pos= loop_point= auto_stop= info=` (see below) |
| `counts` | — | `getVoiceCount` and `getActiveVoiceCount` (which updates the active set). | `counts voices=<n> active=<n>` |
| `fade` | `h` (optional), `param` (`volume`, `pan`, `speed`), `to`, `time` (seconds) | `fadeVolume`, `fadePan` or `fadeRelativePlaySpeed`; without `h`, `fadeGlobalVolume` (`param` must be `volume`). | — |
| `oscillate` | `h` (optional), `param`, `from`, `to`, `time` | `oscillateVolume`, `oscillatePan` or `oscillateRelativePlaySpeed`; without `h`, `oscillateGlobalVolume`. | — |
| `schedule` | `h`, `pause` and/or `stop` (seconds) | `schedulePause`, then `scheduleStop`. | — |
| `state` | — | Reads the engine settings. | `state volume=<f32 bits> clip_scale=<f32 bits> resampler=<0-2> max_active=<n>` |
| `version` | — | `Soloud::getVersion`. | `version <n>` |
| `group` | `as` | `createVoiceGroup`; stores the group handle under `as`, usable as `h` elsewhere. | `group <as> <handle>` |
| `group_add` | `group`, `h` | `addVoiceToGroup`. | `group_add <group> <h> <code>` |
| `group_destroy` | `group` | `destroyVoiceGroup`. | `group_destroy <group> <code>` |
| `group_query` | `group` | `isVoiceGroup`, then `isVoiceGroupEmpty` (which drops stopped voices from the group). | `group_query <group> is_group=<0/1> empty=<0/1>` |
| `filter` | `name`, `kind` (`biquad`, `lofi`, `echo`, `waveshaper`, `dc_removal`), then the kind's settings: `type` (`lowpass`, `highpass`, `bandpass`), `freq`, `res`; `rate`, `bits`; `delay`, `decay`, `filter`; `amount`; `length` | A new filter. When any setting is given, all go to its `setParams`, missing ones at the filter's defaults (lowpass, 1000, 2; 4000, 3; 0.3, 0.7, 0; 0; 0.1). Filters are kept across `init`, and replacing a name does not change sounds that use the old filter. | `filter <name> <code>` |
| `filter_info` | `name` | `getParamCount`, then `getParamName`, `getParamType`, `getParamMax` and `getParamMin` for each parameter and one past the last. | `filter_info <name> count=<n>` then ` <i>:<name or ->:<type>:<max f32 bits>:<min f32 bits>` per index |
| `sound_filter` | `sound`, `slot`, `filter` (optional) | `AudioSource::setFilter`; without `filter` the slot is cleared. | — |
| `global_filter` | `slot`, `filter` (optional) | `Soloud::setGlobalFilter`; without `filter` the slot is cleared. | — |
| `filter_set` | `h` (optional), `slot`, `param`, `value` | `setFilterParameter`; without `h` on the global filter (handle 0). | — |
| `filter_fade` | `h` (optional), `slot`, `param`, `to`, `time` | `fadeFilterParameter`. | — |
| `filter_oscillate` | `h` (optional), `slot`, `param`, `from`, `to`, `time` | `oscillateFilterParameter`. | — |
| `filter_query` | `h` (optional), `slot`, `param` | `getFilterParameter`: 1.0 (`INVALID_PARAMETER`) without a filter, 0 past the last parameter. | `filter_query <h or global> <slot> <param> <f32 bits>` |

Every operation except `tolerance`, `init` and `samples` needs a successful
`init`. Unknown sound or handle names are scenario errors. `<code>` is the
SoLoud error code (0 = no error, 1 = invalid parameter, 3 = file load failed,
5 = out of memory).

`file` is a path relative to `tests/fixtures/` (both runners find it next to
the scenario directory). It may contain letters, digits, `.`, `_`, `-` and `/`
separators, but no `..` or empty parts; a missing file is a scenario error.
Fixtures are generated by `python scripts/gen_wav_fixtures.py`.

Argument values:

- Numbers (`rate`, `amp`, `volume`, `pan`, `speed`, `clip_scale`, `from`,
  `to`) are decimal: an optional `-`, digits, and optionally `.` and more
  digits. Times in seconds (`time`, `clock`, `pause`, `stop`, `loop_point`, `seek`) use the same
  grammar but are parsed as `f64`, like SoLoud's `time`. Counts are unsigned
  integers; flags are `0` or `1`.
- In `query`, `volume`, `overall`, `pan`, `speed`, `rate` and `info` are the
  `f32` bits in 8 hex digits, and `time`, `pos` and `loop_point` the `f64`
  bits in 16, so the log comparison is exact.

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
