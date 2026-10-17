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

- **Samples:** every `mix` appends its interleaved `f32` frames, every
  `samples` a sound's decoded data, and every `fft` its transform. The C++ runner writes them little-endian to
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
| `load` | `name`, `file`, `from` (`path`, default, `disk_file` or `memory_file`), `volume` (1), `looping` (0) | Loads a fixture: `Wav::load`, or `Wav::loadFile` of a `DiskFile` opened on it or a `MemoryFile` holding its bytes, in C++; `solouder_decoders::decode` of its bytes, or `decode_reader` of the opened file or of a cursor over its bytes, in Rust. Then `setVolume`/`setLooping`. A failed load registers nothing; replacing a name stops the old sound's voices, as for `sound`. | `load <name> <code>` |
| `stream` | `name`, `file`, `from` (`file`, default, `memory`, `bytes`, `disk_file`, `memory_file`, `disk_to_mem` or `memory_to_mem`), `volume` (1), `looping` (0), `loop_point` (0) | A streamed fixture: `WavStream::load`, `loadToMem`, `loadMem` of the file's bytes (copied), `loadFile` of a `DiskFile` or `MemoryFile` (kept as long as the stream), or `loadFileToMem` of a `DiskFile` (`disk_to_mem`) or `MemoryFile` (`memory_to_mem`) in C++; `solouder_decoders::WavStream::open`, `load_to_memory`, `from_bytes`, `from_file` of the path or of a caller's `StreamFile` over the bytes, or `from_reader` of the opened file or a cursor over the bytes, plus `Stream::new`, in Rust. Then `setVolume`, `setLooping` and `setLoopPoint`. A name in use is reloaded: C++ loads into the same `WavStream`, which stops its voices (patch 0063), and Rust stops the old stream's voices; a failed load registers nothing and forgets the name. Do not reload a stream while it is queued: the reference then reads the freed data. | `stream <name> <code>` |
| `samples` | `name` | Appends the sound's decoded samples, channel after channel, to the samples. | `samples <name> <channels> <frames> <rate>` (`rate` as `f32` bits in 8 hex digits) |
| `play` | `sound` (a sound, stream, bus, queue or generator name), `as` (handle name), `volume` (-1 = the source's; 1 with `bus`), `pan` (0), `paused` (0), `clock` (seconds; not with `paused`), `background` (0; not with `pan`, `clock`, `bus` or `via`), `bus` (a bus name), `via` (a handle name; not with `bus`) | `Soloud::play`, `Soloud::playClocked` when `clock` is given, or `Soloud::playBackground`; with `bus`, `Bus::play` or `Bus::playClocked` on that bus; with `via`, the handle is the `aBus` argument. Stores the handle under `as`. | `play <as> <handle>` |
| `sound_query` | `name` (a sound, else a stream, else a generator) | `getLength` and `Soloud::countAudioSource`; only the count for a generator. | `sound_query <name> length=<f64 bits> count=<n>`, or `sound_query <name> count=<n>` for a generator |
| `stop_sound` | `name` (a sound, stream, bus, queue or generator), `source` (0) | `Soloud::stopAudioSource`, or `AudioSource::stop` with `source=1`. | — |
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
| `filter` | `name`, `kind` (`biquad`, `lofi`, `echo`, `waveshaper`, `dc_removal`, `fft`, `bassboost`, `robotize`, `flanger`, `freeverb`), then the kind's settings: `type` (`lowpass`, `highpass`, `bandpass`), `freq`, `res`; `rate`, `bits`; `delay`, `decay`, `filter`; `amount`; `length`; none; `boost`; `freq`, `wave` (an unsigned integer); `delay`, `freq`; `freeze`, `room`, `damp`, `width` | A new filter. When any setting is given, all go to its `setParams`, missing ones at the filter's defaults (lowpass, 1000, 2; 4000, 3; 0.3, 0.7, 0; 0; 0.1; —; 2; 30, 0; 0.005, 10; 0, 0.5, 0.5, 1). `RobotizeFilter::setParams` returns nothing and logs 0. Filters are kept across `init`, and replacing a name does not change sounds that use the old filter. | `filter <name> <code>` |
| `filter_info` | `name` | `getParamCount`, then `getParamName`, `getParamType`, `getParamMax` and `getParamMin` for each parameter and one past the last. | `filter_info <name> count=<n>` then ` <i>:<name or ->:<type>:<max f32 bits>:<min f32 bits>` per index |
| `sound_filter` | `sound`, `slot`, `filter` (optional) | `AudioSource::setFilter` on a sound, queue or generator; without `filter` the slot is cleared. | — |
| `global_filter` | `slot`, `filter` (optional) | `Soloud::setGlobalFilter`; without `filter` the slot is cleared. | — |
| `filter_set` | `h` (optional), `slot`, `param`, `value` | `setFilterParameter`; without `h` on the global filter (handle 0). | — |
| `filter_fade` | `h` (optional), `slot`, `param`, `to`, `time` | `fadeFilterParameter`. | — |
| `filter_oscillate` | `h` (optional), `slot`, `param`, `from`, `to`, `time` | `oscillateFilterParameter`. | — |
| `filter_query` | `h` (optional), `slot`, `param` | `getFilterParameter`: 1.0 (`INVALID_PARAMETER`) without a filter, 0 past the last parameter. | `filter_query <h or global> <slot> <param> <f32 bits>` |
| `fft` | `size` (a power of two from 4 to 4096), `inverse` (0), `pattern`, `amp`, `period`, `seed` (as for `sound`) | Renders `size` samples of one channel of the pattern, transforms them with `FFT::fft`, or `FFT::ifft` with `inverse=1`, and appends the result to the samples. | — |
| `bus` | `name`, `channels`, `resampler` (an index: 0 point, 1 linear, 2 Catmull-Rom; others are ignored), `visualization` (0), `volume` (1) | A new `Bus`: `setChannels` if `channels` is given, `setResampler`, `setVisualizationEnable`, `setVolume`. A bus under an existing name is deleted first, which stops its voices. | `bus <name> <setChannels code>` |
| `annex` | `bus`, `h` | `Bus::annexSound`; `h` may name a voice group. | — |
| `bus_query` | `bus` | `getActiveVoiceCount` (which looks up the bus handle), `Soloud::countAudioSource` on the bus, `getApproximateVolume` of channels 0 to 9. | `bus_query <bus> voices=<n> count=<n> volumes=<hex>,...` |
| `bus_wave` | `bus`, `fft` (0) | Appends the 256 floats of `getWave`, or `calcFFT` with `fft=1`, to the samples, if the bus has an instance. | `bus_wave <bus>`, or `bus_wave <bus> none` without an instance |
| `bus_filter` | `bus`, `slot`, `filter` (absent clears the slot) | `Bus::setFilter`, which also replaces the playing instance's filter. | — |
| `bus_set` | `bus`, then any of `channels`, `resampler`, `visualization` | Applied in that order: `setChannels`, `setResampler`, `setVisualizationEnable`. | `bus_set <bus> channels <code>` if `channels` is given |
| `queue` | `name`, `channels`, `rate`, `from`, `volume` (1) | A new `Queue`: `setParams(rate, channels)` if either is given (defaults 44100 and 1), `setParamsFromAudioSource` on the sound `from`, `setVolume`. A queue under an existing name is deleted first, which stops its voices. Play a queue with `play`. | `queue <name> <setParams code>` (0 without `setParams`) |
| `queue_add` | `queue`, `sound` (a sound, else a stream, else a generator) | `Queue::play` with the sound, stream or generator. Do not replace or reload a sound while it is queued: the reference then reads the deleted `Wav`. | `queue_add <queue> <code>` |
| `queue_query` | `queue`, `sound` (optional; a sound, else a stream) | `getQueueCount`, and `isCurrentlyPlaying` for `sound`. | `queue_query <queue> count=<n>`, then ` current=<0 or 1>` with `sound` |
| `sound3d` | `name` (a sound, stream, bus, queue or generator), `min` and `max`, `model` (an unsigned integer: 0 none, 1 inverse, 2 linear, 3 exponential; others act as none) and `rolloff`, `relative` (paired keys go together) | Applied in that order: `AudioSource::set3dMinMaxDistance`, `set3dAttenuation`, `set3dListenerRelative`. Affects later plays. | — |
| `play3d` | `sound` (a sound, stream, bus, queue or generator), `as`, `x`, `y`, `z` (0), `volume` (1), `paused` (0), `via` (a handle name, the `aBus` argument) | `Soloud::play3d` with zero velocity. Stores the handle under `as`. | `play3d <as> <handle>` |
| `listener` | `x`, `y`, `z`; `ax`, `ay`, `az`; `ux`, `uy`, `uz` (each triple goes together; at least one); `params` (0) | With `params=1` (all three triples needed) `set3dListenerParameters`; otherwise `set3dListenerPosition`, `set3dListenerAt` and `set3dListenerUp` for the triples given, in that order. | — |
| `set3d` | `h` (may name a voice group), `x`, `y`, `z`, `params` (0; needs the position), `min` and `max`, `model` and `rolloff` | Applied in that order: `set3dSourcePosition` (`set3dSourceParameters` without velocity with `params=1`), `set3dSourceMinMaxDistance`, `set3dSourceAttenuation`. | — |
| `update3d` | — | `Soloud::update3dAudio`. | — |
| `speaker` | `channel`, `x`, `y`, `z` (together, optional) | `setSpeakerPosition` when a position is given, then `getSpeakerPosition` (0, 0, 0 when it fails). | `speaker <channel> set=<code> get=<code> <x> <y> <z>` (` set=` only with a position; coordinates as `f32` bits in 8 hex digits) |

### Generators

Generators are SoLoud's other audio sources (`solouder_sources` in Rust,
played through `solouder_core::Synth`). Each creating operation takes the
source settings `volume` (1), `looping` (0), `loop_point` (0), `auto_stop`
(1), `single` (0), `tick` (0) and `kill` (0), applied with `setVolume`,
`setLooping`, `setLoopPoint`, `setAutoStop`, `setSingleInstance` and
`setInaudibleBehavior(tick, kill)`. A generator under an existing name is
deleted, which stops its voices. Generator names share the space of sound
names: a sound, stream, bus or queue of the same name is found first.

| Operation | Arguments | Effect | Log line |
|---|---|---|---|
| `noise` | `name`, `type` (an integer: 0 white, 1 pink, 2 brownish, 3 blueish; others act as white), `s0` to `s9` (0 when not given), source settings | A new `Noise`; `setType` if `type` is given, then `setOctaveScale(s0, ..., s9)` if any `s` is given. | — |
| `noise_set` | `name` (a noise), `type`, `s0` to `s9` | As for `noise`, on an existing noise. Affects later plays: a voice keeps the weights it started with. | — |
| `vic` | `name`, `model` (0 PAL, default, or 1 NTSC), `bass`, `alto`, `soprano`, `noise` (0 to 255), `reg` and `value` (together; any index), source settings | A new `Vic`; `setModel` if `model` is given, `setRegister` for each register given in that order, then `setRegister(reg, value)`; an index past the last is ignored (patch 0064). | — |
| `vic_set` | `name` (a vic), then as for `vic` without source settings | The same on an existing vic: playing voices read the registers and model at their next block. | — |
| `vic_query` | `name`, `reg` (optional) | `getModel` and `getRegister` of the four registers, and of `reg` (0 past the last). | `vic_query <name> model=<n> regs=<b>,<a>,<s>,<n>`, then ` reg=<value>` with `reg` |

Every operation except `tolerance`, `init`, `samples` and `fft` needs a
successful `init`. Unknown sound or handle names are scenario errors. `<code>` is the
SoLoud error code (0 = no error, 1 = invalid parameter, 3 = file load failed,
5 = out of memory, 6 = not implemented).

`file` is a path relative to `tests/fixtures/` (both runners find it next to
the scenario directory). It may contain letters, digits, `.`, `_`, `-` and `/`
separators, but no `..` or empty parts; a missing file is a scenario error.
Fixtures are generated by `python scripts/gen_wav_fixtures.py`.

Argument values:

- Numbers (`rate`, `amp`, `volume`, `pan`, `speed`, `clip_scale`, `from`,
  `to`, 3D coordinates, distances and `rolloff`) are decimal: an optional
  `-`, digits, and optionally `.` and more digits. Times in seconds (`time`,
  `clock`, `pause`, `stop`, `loop_point`, `seek`) use the same grammar but are parsed as `f64`, like SoLoud's `time`. Counts are unsigned
  integers; flags are `0` or `1`.
- Keep 3D settings away from subnormal intermediate values: once the C++
  runner has mixed, its thread flushes subnormals, also in `play3d` and
  `update3d`, and Rust does not.
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
