# SoLouder porting guide: SoLoud C++ → Rust for Bevy

## Goal and current status

SoLouder ports the SoLoud audio engine to **native Rust**, with an idiomatic
**Bevy plugin** for assets, playback, buses, filters, and spatial audio. It
keeps SoLoud's mixing behavior without requiring the C++ library in the normal
Rust build, and adds the fixes and extensions developed while shipping SoLoud
in a Unity game.

**Current status:** milestone E is done. This repository contains the
patched C++ reference tooling (pinned upstream commit, patch series, fetch
script, C++ regression tests), the parity ledger, and a Cargo workspace with
the scenario harness: `tests/scenarios/*.scn` run through a C++ null-driver
runner and through the Rust crates, and `python scripts/run_parity.py`
compares them. `solouder-core` plays prepared PCM (play, stop, pause, loop,
volume, pan, speed, three resamplers, 1 to 8 channels, clipping) and
`solouder-decoders` decodes WAV files into it; both match the reference in
every scenario (bit for bit, except the documented 8-bit WAV conversion).
The mixer runs on the audio thread behind bounded, wait-free queues
(`solouder_core::realtime`), `solouder-cpal` plays it on the default device,
and `bevy_solouder` plays `.wav` assets from entities that stop when
despawned. Milestone D added fades, scheduling, the remaining voice
controls, voice groups, filters, buses and queues, offline and through the
real-time controller. Milestone E added streamed and decoded WAV, FLAC,
Ogg Vorbis and MP3, and 3D audio offline, through the real-time controller
and in Bevy (see the [review](#milestone-e-review) and
[port-status.md](port-status.md)).

[ARCHITECTURE.md](ARCHITECTURE.md) owns the target workspace, Vertical Slice
Architecture (VSA), ownership, real-time boundaries, and Bevy contracts. This
guide owns translation, reference behavior, patch preservation, the parity
target, milestones, and validation. Do not duplicate the target architecture
here.

The deliverables are:

1. A Bevy-independent Rust mixer with deterministic offline rendering.
2. Rust sources, decoding, streaming, and DSP, delivered in compiling milestones
   and tested against the patched C++ reference.
3. A device-output adapter, initially targeting Windows through CPAL.
4. A Bevy plugin for asset loading, playback controls, lifecycle notifications,
   and listener/emitter synchronization.

A C API wrapper is **not** the final port. Temporary FFI is acceptable for
reference tests or an explicitly labeled migration tool, but must not become
a hidden native-build dependency.

## The C++ reference

The reference is upstream SoLoud at the commit pinned in
`reference/upstream.json`, plus the patches in `reference/patches/`, applied
in order. It lives in `reference/soloud/`, which is git-ignored. Create or
refresh it with:

```sh
python scripts/fetch_reference.py           # clone upstream and apply patches
python scripts/fetch_reference.py --force   # recreate after the patches change
python scripts/test_reference.py            # build and run the C++ regression tests
```

The fetch is reproducible: the resulting tree is identical to the patched fork
it was exported from. Every path in this guide that starts with `include/`,
`src/`, `contrib/` or `docsrc/` is relative to `reference/soloud/`.

Change the reference only through patches. Make the change as a commit on a
SoLoud branch (the working branch of the kelo221/soloud fork is the current source),
then run `python scripts/export_reference_patches.py <fork>` and the commands
above. Each patch states its upstream issue or PR. C++ regression tests for
reference fixes live in `reference/tests/regressions.cpp`, one test per fix,
and must fail on the unpatched code.

## Ground rules

- **Preserve the C++ reference.** Do not edit `reference/soloud/` directly,
  remove backends, regenerate bindings, or change CMake merely to simplify the
  Rust port. Reference changes are reviewed patches.
- **Read headers and implementations together.** Trace callers, factories,
  teardown, and buffer contracts before translating ownership. A raw pointer
  alone does not identify its owner.
- **Port behavior, not C++ layout.** Use Rust modules, traits, enums, ownership,
  and typed errors rather than recreating inheritance or `m` field prefixes.
  Use snake_case and keep source mappings for implementation review.
- **Mark ported code.** Each Rust file translated from SoLoud starts with the
  header described in [CONTRIBUTING.md](../CONTRIBUTING.md#marking-ported-code).
- **Follow VSA.** Keep feature-specific operations, types, integration, and
  tests together within the dependency boundaries in
  [ARCHITECTURE.md](ARCHITECTURE.md#vertical-slice-architecture).
- **Keep every first-party Rust file at or below 400 physical lines**, including
  comments, blanks, tests, examples, benchmarks, and build scripts. Split by
  cohesive responsibility, not arbitrary chunks. Markdown has no line cap.
  Scope/exclusions and enforcement are defined in
  [ARCHITECTURE.md](ARCHITECTURE.md#rust-file-size-rule).
- **Keep each milestone compiling.** Omit unsupported features explicitly or
  return typed errors; never silently succeed with placeholder output.
- **Use stable Rust.** Declare an edition/minimum Rust version when introducing
  the workspace. Pin/test a Bevy release rather than guessing its APIs.
- **Keep Bevy out of core/decoder code.** Offline tests must not need a Bevy
  `App` or an audio device.
- **Prefer safe Rust.** Do not copy undefined C++ behavior. Confine necessary
  `unsafe` and document actual invariants. Do not add unsafe `Send`/`Sync`
  implementations merely to satisfy Bevy bounds.
- **Reshape borrows rather than introducing aliasing.** Split state, shorten
  borrows, or use slot IDs instead of pointers to mutable voices/buses.
- **Respect the real-time contract.** Loading, blocking I/O, allocation, and
  retirement/destruction stay outside callbacks as specified in
  [ARCHITECTURE.md](ARCHITECTURE.md#ownership-and-the-real-time-boundary).
  Standard I/O and async asset loading are allowed outside that boundary.
- **Do not install a global allocator.** Libraries must not choose one for the
  Bevy application.
- **Record uncertainty.** `TODO(port)` identifies unresolved behavior;
  `PERF(port)` identifies performance concerns. Neither replaces tests or
  ledger entries. Document deliberate changes and memory-safety fixes.
- **Do not port known reference defects.** Crashes, hangs, and memory-safety
  bugs tracked in [Reference patches and upstream tracking](#reference-patches-and-upstream-tracking)
  are fixed in Rust, never reproduced for parity. Prefer fixing them in the
  reference first, so fixtures come from correct code. Fixtures that still
  exercise an unpatched defect are labeled `reference-defect`, and the Rust
  result and the departure are recorded in `docs/port-status.md`.

## Parity target and ledger

The target is **behavioral parity with the whole SoLoud C API**
(`include/soloud_c.h`): every function either has a ported, tested Rust
equivalent or is excluded with a written reason. API names and handle
encodings are not part of the target; audible behavior is.

[`docs/parity-ledger.csv`](parity-ledger.csv) enumerates that surface. It is
generated by `python scripts/gen_parity_ledger.py` from the reference header:
496 C declarations collapse into 208 behaviors, because the `Ex` variants
share their base function and the methods every source or filter exposes
(`Wav_setVolume`, `Bus_setVolume`, ...) share one `AudioSource::` or
`Filter::` row. The generator rewrites the `behavior`, `kind` and
`c_functions` columns and preserves the tracking columns:

| Column | Meaning |
|---|---|
| `slice` | Owning vertical slice (see [ARCHITECTURE.md](ARCHITECTURE.md#vertical-slice-architecture)) |
| `milestone` | Milestone that delivers it (proposed automatically for new rows; edit freely) |
| `status` | `todo`, `in-progress`, `ported`, or `excluded` |
| `scenarios` | IDs of the parity scenarios that verify it |
| `notes` | Exclusion reasons, known differences, decisions |

A row is `ported` only when its scenarios pass against the reference. Run
`python scripts/gen_parity_ledger.py --check` in CI.

Numerical tolerance: compare against a scalar (non-SIMD) reference build.
Scenarios are bit-exact by default, with zero and subnormal values counted as
equal because the reference flushes subnormals. A scenario that cannot be
bit-exact declares a `tolerance` (at most 1e-6 unless documented, for example
for lossy decoders), and the runner prints the measured maximum and RMS
difference.

C++-only extension points that the C API cannot express (custom
`AudioSource`/`Filter` subclasses, `File` implementations, the
`AudioSourceInstance` contract) are covered by the Rust trait design in
ARCHITECTURE.md rather than by ledger rows.

## Reference source map

Responsibilities identify what to inspect, not a mandate for one Rust crate or
file per C++ class. Paths are relative to `reference/soloud/`.

| C++ reference | Porting responsibility |
|---|---|
| `include/soloud.h`, `src/core/soloud.cpp` | Configuration, mixer, resampling, clipping, visualization |
| `include/soloud_audiosource.h`, `src/core/soloud_audiosource.cpp` | Source definitions and playback instance contracts |
| `src/core/soloud_core_basicops.cpp`, `src/core/soloud_core_voiceops.cpp`, `src/core/soloud_core_voicegroup.cpp` | Playback, lifecycle, handles, groups |
| `src/core/soloud_core_getters.cpp`, `src/core/soloud_core_setters.cpp` | Controls, validity, voice limits |
| `include/soloud_fader.h`, `src/core/soloud_fader.cpp`, `src/core/soloud_core_faderops.cpp` | Fades, oscillation, scheduled changes |
| `include/soloud_bus.h`, `src/core/soloud_bus.cpp` | Routing and nested mixing |
| `include/soloud_queue.h`, `src/core/soloud_queue.cpp` | Queued sources and lifecycle |
| `src/core/soloud_core_3d.cpp` | Attenuation, Doppler, listener/speaker geometry, distance delay |
| `include/soloud_filter.h`, `src/core/soloud_filter.cpp`, `src/core/soloud_core_filterops.cpp`, `src/filter/` | DSP state, parameters and automation |
| `include/soloud_wav.h`, `src/audiosource/wav/soloud_wav.cpp` | Fully decoded audio |
| `include/soloud_wavstream.h`, `src/audiosource/wav/soloud_wavstream.cpp` | Streaming, seek, rewind, codec state |
| `src/core/soloud_file.cpp`, `src/core/soloud_thread.cpp` | Loading and control/output coordination |
| `src/core/soloud_fft.cpp`, `src/core/soloud_fft_lut.cpp` | FFT and visualization/DSP helpers |
| `src/backend/` | Device behavior to preserve while replacing backends |
| `src/c_api/soloud_c.cpp`, `include/soloud_c.h` | Parity surface; optional reference-test boundary |
| `src/audiosource/` | Additional generators, speech, music sources |

Build descriptions are in `contrib/CMakeLists.txt`, `contrib/src.cmake`, and
`build/genie.lua`. Check coverage across build configurations rather than
assuming every source file is always included.

## Mixer behavior to preserve

Audible compatibility is the default. Record deliberate departures with reasons
and regression tests in `docs/port-status.md`.

### Buffers, timing, and numerics

- Internal audio is **planar `f32`**, with an explicit channel stride. Source
  methods return frames written, not scalar samples. Keep frames, samples,
  channels, bytes, and allocated stride distinct.
- Interleave/convert at the output boundary. Specify channel order. Reference
  layouts have 1, 2, 4, 6, or 8 channels; reject unsupported mappings.
- Preserve 512-frame processing granularity where DSP depends on it. Handle
  variable callback lengths and partial blocks without uninitialized reads.
- Preserve source-to-bus resampling, point/linear/Catmull-Rom modes, the linear
  default, and 20-bit fractional playhead arithmetic. Test interpolation history
  and loop boundaries, not only steady-state playback.
- Preserve volume/pan ramps, filter order, hard/soft clipping, and default 0.95
  post-clip scaling. Do not clamp individual voices prematurely.
- Scheduling/fades follow rendered audio time, not Bevy frame delta. Game frame
  updates submit controls but do not advance the audio clock.
- The reference adopts the device sample rate. A fixed engine rate plus output
  resampling would be a documented design change, not existing behavior.
- Keep scalar DSP as the correctness baseline. Benchmark SIMD, prove alignment,
  and use cross-target tolerances. Denormal/flush-to-zero behavior must apply on
  the audio-processing thread rather than relying on game-thread settings.

### Voices and playback

- Reusable sources and mutable instances have different lifetimes. Two plays of
  one asset need independent cursors, loops, filters, and decoder state.
- Preserve handle validity: stale IDs cannot control reused slots. Raw C++
  encoding matters only to an explicit compatibility boundary; document Rust
  error/no-op semantics.
- Reference limits are 1024 voice slots, a default active cap of 16 (any value
  up to 1023 is accepted), and eight filter slots per stream. Distinguish
  logical, active, protected, and inaudible voices. Preserve loudness selection,
  "protected voices are always played", and inaudible-tick behavior rather than
  stopping every non-active voice.
- Preserve pause, stop, seek, rewind, loop point/count, speed, fades, scheduling,
  auto-stop, single-instance behavior, stealing, and groups as ported.
- Mixer-controlled looping handles short source blocks. Empty sources/failed
  rewinds must not loop forever; unused output tails must be zeroed.

### Routing, filters, and spatial audio

- Reference buses are playable sources with recursive routed mixing. Preserve
  nesting and child teardown semantics; the safe target rejects routing cycles.
- Keep filter history per playback/bus instance. Source filters run before
  resampling; global filters run after root mixing. Trace/test bus filter order
  rather than assuming every filter stage is interchangeable.
- Preserve automation as well as filter equations. Validate restrictions such
  as Freeverb's stereo assumption.
- Preserve attenuation, Doppler, distance delay, and speaker panning. Coherent
  spatial snapshots replace unsynchronized shared-state updates, not the math.
- Visualization/FFT observations must not expose buffers while the callback
  mutates them; verify bounded snapshot behavior.

## C++ → Rust type and idiom map

| C++ pattern | Rust translation |
|---|---|
| Owning `new`/`delete`, `new[]`/`delete[]` | `Box<T>`, `Vec<T>`, `Box<[T]>`; automatic cleanup |
| Shared immutable PCM | `Arc<[f32]>` with validated layout/rate metadata |
| Pointer + length input/output | Slices with explicit frame/stride contracts |
| Nullable borrowed pointer | `Option<&T>` / `Option<&mut T>` for local borrowing |
| Stored voice/source/bus pointer | Ownership or checked IDs; no fabricated `'static` reference |
| Source/instance inheritance | Definition/factory and mutable instance traits |
| Virtual source/filter methods | Object-safe traits with construction off the callback |
| Decoder union + tag | Enum variants owning matching codec state |
| Numeric result/error codes | Typed `Result`; retain numeric codes only at compatibility boundaries |
| Raw handle | Distinct generation-checked IDs, not pointers or Bevy entities |
| Reference time | Preserve `f64` seconds and checked frame conversions |
| Integer lengths/offsets | `usize` indexing and checked external-data conversions/multiplication |
| Bit flags | Typed flags; no integer-to-enum transmutation |
| Destructor cleanup | Ownership/`Drop`, with coordinated runtime/worker shutdown |
| `memcpy`/`memset` | Slice copies/fills, overlap-safe when needed |
| Platform macros | Cargo features / `#[cfg]`; `cfg!` still type-checks both branches |
| C file/path APIs | `Read`/`Seek`, bytes, `Path`/`PathBuf`; C strings only at FFI |

Preserve algorithmic widths: samples are `f32`, reference time is `f64`, and
fixed-point operations must explicitly be checked, saturating, or wrapping.
Do not bypass external-data, channel, or ID validation with narrowing casts
or `transmute`.

`Vec<f32>` guarantees `f32` alignment, not every SIMD alignment. Use proven
aligned allocations or unaligned loads. Initialize buffers before exposing
slices; `MaybeUninit` does not excuse partial-decoder uninitialized tails.

## Decoding, streaming, and patch preservation

`Wav` fully decodes WAV, Ogg Vorbis, MP3, and FLAC to planar samples.
`WavStream` keeps format-specific playback state. Port their distinct policies
against the [decoder/streaming design](ARCHITECTURE.md#decoding-and-streaming-design).

- Test metadata validation: sample rate, channel layout, frame count, checked
  allocation size, malformed/truncated data, and decoded-memory limits. The
  patched reference rejects more than 8 channels for WAV/FLAC/MP3 (resident and
  streamed) and unknown Ogg lengths; Ogg keeps dropping channels beyond 8.
- Test multiple plays independently; mutable decoder cursors cannot be shared
  through one asset. Verify prebuffering, underflow, end-of-stream, cancellation,
  and pause without blocking the callback.
- Seek/rewind/loop must discard old buffered PCM, partial decoder output, and
  interpolation history. Verify stale worker output cannot play after seek and
  document seek accuracy/acknowledgement.
- Codec state is an enum, which prevents the #405 class of codec-memory
  reinterpretation. The streaming fixes are listed in the
  [patch ledger](#reference-patch-ledger).
- Preserve FLAC in both resident and streaming paths; it already exists here.
- Windows/CMake reference-build edits must not impose a C++ compiler, DLL, or
  static-CRT policy on normal Rust users.

Rust decoders may differ numerically from vendored C decoders, especially for
lossy formats and seek/preroll. Compare decoders separately from mixer DSP,
which uses identical prepared PCM. Do not blindly translate vendored codec
internals, generated bindings, or third-party backends as first-party modules.

WAV decoding is first-party code in `solouder-decoders`: a bounds-checked
RIFF reader (`riff.rs`) that validates the whole format chunk (channel count,
sample rate, format and width) before anything is allocated, and dr_wav's
sample conversions (`convert.rs`). The one allocation is sized from the bytes
present, not from header claims. Where files are ambiguous it follows dr_wav:
chunks padded to even sizes, an untrusted RIFF size, the extensible
sub-format taken from the GUID's first two bytes, and the channel mask
ignored. The 16, 24 and 32-bit integer, float, A-law and mu-law fixtures in
`tests/fixtures/wav/`, plain and extensible, decode bit for bit like the
reference (`wav_decode`), and the reference rejects the same malformed files.
`mutated_files_never_panic` fuzzes the reader deterministically, and
`check_all.py` runs the decoder tests in release as well as debug.

Symphonia 0.6 decoded WAV until review found that it panics on headers whose
16-bit block size overflows, lets a zero channel mask turn 33 channels into
one, rejects extensible A-law and mu-law, and reserves memory from metadata
size claims before reading. It decodes FLAC (milestone E unit 4), after
the same malformed-input testing, and stays the candidate for Ogg Vorbis
and MP3. ADPCM, 12-bit and 64-bit integer WAV, which
dr_wav also reads, are not supported (`DecodeError::Unsupported`).

## Reference patches and upstream tracking

Upstream SoLoud (`jarikomppa/soloud`) is effectively unmaintained: the last
upstream commit is `e82fd32` (2024-08), and many open issues and pull requests
describe real crashes that were never merged. This section records what the
reference patches fix, what the port must still avoid, and what it must
decide. It is a snapshot taken on **2026-10-12**; refresh it before relying on
upstream states:

```sh
gh api "repos/jarikomppa/soloud/issues?state=open&per_page=100" --paginate
gh api "repos/jarikomppa/soloud/pulls?state=open&per_page=100" --paginate
```

"Confirmed" means the defect was verified in the reference source (for the
fixed ones, by a C++ test that fails without the patch); "reported" means it is
described upstream but not reproduced here.

### Reference patch ledger

Patches in `reference/patches/`, in application order. Every behavioral fix has
a C++ regression test in `reference/tests/regressions.cpp` that fails on the
unpatched reference, and each needs a matching Rust test.

| Patch | Upstream | Fix | C++ test |
|---|---|---|---|
| 0001 | — (found in production) | CoreAudio `AudioQueueRef` moved from a file-level static into per-engine `mBackendData`; deiniting one engine no longer stops every engine (on macOS, every engine went silent). | — (macOS only) |
| 0002 | #405, PR #379 | `WavStreamInstance::seek` tested the codec union member `mCodec.mOgg`, which is non-null for every live WAV/FLAC/MP3 decoder, so `stb_vorbis_seek` ran on a `drwav*`/`drflac*`/`drmp3*` and corrupted the heap at every loop wrap of a looping non-Ogg stream (a crash seen in production). Gated on `mFiletype == WAVSTREAM_OGG`; other formats use generic seek. | covered by `wavstream_loop_channel_stride` |
| 0003 | — | Build only: CMake ≥ 3.5 (CMake 4 requires it), static MSVC runtime, `speech/Elements.def` header-only. | — |
| 0004 | PR #379 | `WavStreamInstance::getAudio` wrote FLAC/MP3/WAV channels at stride `aSamplesToRead` instead of `aBufferSize`; the loop-wrap refill reads fewer frames than the stride, so channels ≥ 1 landed at the wrong offset. | `wavstream_loop_channel_stride` |
| 0005 | PR #379 | `WavStreamInstance::hasEnded` ignored `LOOPING`. | `wavstream_looping_not_ended` |
| 0006 | #279, #205, #327 | `calcActiveVoices_internal` returned before `mapResampleBuffers_internal` when inaudible-tick voices filled the active cap; voices were mixed with null resample buffers (segfault). | `inaudible_tick_over_active_cap` |
| 0007 | #298 | Protected voices join the must-live group ahead of the volume sort; previously a quiet protected voice could be made inactive, contradicting "Protected voices are always played". | `protected_voice_stays_active` |
| 0008 | (found auditing #343) | A voice whose fixed-point step is zero (speed near zero, or a step beyond the fixed-point range) skipped resampling but was still panned, mixing the previous voice's scratch data into the output. Unwritten scratch is now zeroed. | `zero_step_voice_no_stale_scratch` |
| 0009 | #392 | `AudioSourceInstance::seek` looped forever with less than one frame of scratch; it now rejects undersized scratch and stops on a non-positive count. | `generic_seek_small_scratch` |
| 0010 | #345, #404, #324 | Stopping a voice before a successful init (or after init failed) walked a null `mResampleDataOwner`. The readers are guarded instead of resetting the cap (PR #345 resets it to 16 on every init, discarding a pre-init `setMaxActiveVoiceCount`). `postinit_internal` no longer leaks the previous arrays. | `play_stop_without_init`, `play_after_failed_init` |
| 0011 | — (found in production) | `mapResampleBuffers_internal` used `char live[256]` guarded by `SOLOUD_ASSERT(mMaxActiveVoices < 256)`, while `setMaxActiveVoiceCount` accepts up to 1023. On MSVC the assert is a `__debugbreak` even in release builds (why the shipping game capped at 255); without asserts it overran the stack. Sized by `VOICE_COUNT`. | `active_voice_cap_above_255` |
| 0012 | #401, #317 | Decoder metadata validation: `Wav` WAV/FLAC/MP3 overflowed `float tmp[512 * MAX_CHANNELS]` with more than 8 channels and truncated 64-bit frame counts when allocating; `Wav::loadogg` accepted length 0 and could copy past the reported length; `WavStream` FLAC/MP3/WAV clamped `mChannels` but still decoded every channel into the fixed block. Invalid layouts are rejected, allocation is `nothrow`, short reads are zero-filled. | `wav_rejects_too_many_channels`, `wavstream_rejects_too_many_channels` |
| 0013 | #361 | `AudioSourceInstance3dData::init` kept the previous voice's 3D flags; `play3d` in a slot last used by a listener-relative voice started at volume 1.0 instead of 1/99 at 99 units. | `play3d_reused_slot_flags` |
| 0014 | PR #375 | `Wav` and `WavStream` are non-copyable (implicit copies double-freed owned buffers). | compile-time |
| 0015 | — | Build only: `SOLOUD_BACKEND_MINIAUDIO` CMake option (the shipped Linux library uses MiniAudio, which loads ALSA/PulseAudio/JACK at run time). | — |
| 0016 | — (found in production) | The MiniAudio backend kept one global `ma_device`: a second engine re-initialized the device the first was playing on, and deiniting any engine stopped all of them. The device is now per engine in `mBackendData`, as in 0001. | — (device only; checked with two engines on a PulseAudio null sink) |
| 0017 | — | Build only: the static MSVC runtime (from 0003) is opt-in through `SOLOUD_MSVC_STATIC_RUNTIME`, so static-library users keep upstream's `/MD`. | — |
| 0018 | — (found porting milestone B) | The SSE 1→2 and 2→2 paths in `panAndExpand` multiplied the per-frame gain step by 4 for the four-frame loop and reused it, restarting from the initial gain, for the frames left over when a mix call is not a multiple of 4 frames. A volume or pan change then overshot at the end of the call (4/3 of the target in a 6-frame call). | `pan_ramp_leftover_frames` |
| 0019 | — (found porting milestone B) | `panAndExpand` 6→4 and 8→4 added the center/LFE fold-down to the front pair without the speaker gain, so a voice at volume 0 was audible; 4→8 scaled the rear pair by the side speakers' gains (different under 3D). | `surround_to_quad_center_follows_volume`, `quad_to_7_1_rear_uses_rear_volume` |
| 0020 | — (found porting milestone B) | With every voice protected, `findFreeVoice_internal` called `stopVoice_internal(-1)` and `play` returned `UNKNOWN_ERROR` (7) as a handle, which decodes as voice slot 6. Now no voice is stopped and `play` returns 0. Handles whose slot field is 0 or above `VOICE_COUNT` indexed `mVoice`/`m3dData` out of bounds and are now invalid. | `play_without_free_voice`, `handle_slot_out_of_range` (the second passes by luck without the patch) |
| 0021 | — (found porting milestone B) | `Wav::loadRawWave`, `loadRawWave8` and `loadRawWave16` accepted more than `MAX_CHANNELS` channels (overflowing the resample blocks), data shorter than one frame, and a NaN sample rate. | `raw_wave_rejects_bad_layout` |
| 0022 | #343 | With a step of 512 source frames or more per output frame (for example speed 600 at equal rates), no output frame fits in a block, so the resampling loop loaded block after block forever and `mix` never returned. The old guard only caught steps above 4096, and the inaudible-tick branch had none. Such steps (and NaN) now take the stopped-voice path (step 0) in both branches. | `extreme_speed_makes_progress` |
| 0023 | — (found in the milestone C review) | `setProtectVoice` changed the flag without marking the active voice set dirty, so a newly protected voice stayed unmixed (and an unprotected one kept its place) until an unrelated play, stop, pause or volume change recomputed the set. | `protecting_a_voice_makes_it_active` |
| 0024 | — (related to #76) | A non-looping voice was stopped at the end of the mix call in which its source reported the end, which happens once the last block is read, not played. When the call ended partway through that block (any device buffer not a multiple of 512 frames, such as WASAPI's, or any resampling) the rest was dropped with a click: up to one source block, 600 frames of a 1000-frame sound at speed 0.73 in 256-frame calls. A voice now records the real samples in its newest block and stops once the position has passed them by the resamplers' two-frame history; a voice that cannot advance still stops when its source ends. | `sound_tail_survives_unaligned_mix_calls` |
| 0025 | — (found testing the shipped libraries) | `AudioSource::mSoloud` kept pointing at the last engine a sound was played on. Stopping or destroying the sound after that engine was destroyed used freed memory (it hung on the freed mutex); a sound played on two engines was stopped only on one, so destroying it left the other voice reading freed samples; sound IDs were counted per engine, so stopping one sound could stop another first played on a different engine. Engines are now in a process-wide list: `AudioSource::stop` stops the sound on every live engine, `~Soloud` leaves the list first, and IDs are process-wide. `BusInstance` keeps its own engine, and bus and queue instances clear their parent's pointer and handle when destroyed, so `Bus` and `Queue` calls after their engine is gone do nothing; playing one again on any engine moves it. An engine's audio mutex lives as long as the engine and `postinit_internal` holds it, since another thread may stop a sound on an engine being initialized. `SINGLE_INSTANCE` stops only on the engine playing the sound. Still open upstream: a sound destroyed while queued in a `Queue`, and `DuckFilter`'s engine pointer. | `source_outlives_its_engine`, `source_stops_on_every_engine`, `source_ids_unique_across_engines`, `bus_outlives_its_engine`, `queue_outlives_its_engine`, `bus_moved_to_another_engine`, `sources_and_engines_on_two_threads` |
| 0026 | — (found porting milestone D) | `setInaudibleBehavior` changed the ticking flag, which decides which voices are active, without marking the active set dirty (the defect of 0023). | `ticking_a_voice_makes_it_active` |
| 0027 | — (found porting milestone D) | `setSamplerate` accepts any rate, and a negative one made the fixed-point step a huge unsigned value: no output frame was ever written and `mix` hung. A negative step now takes the stopped-voice path, like the steps of 0022. | `negative_sample_rate_renders_silence` |
| 0028 | — (found porting milestone B) | `AudioSourceInstance::seek` turned seconds into frames at `mSamplerate`, which includes the relative play speed, while `mStreamPosition` counts seconds of source: at speed 2, `Soloud::seek` and every loop wrap to a loop point went twice as far. It now uses `mBaseSamplerate`, as `WavStreamInstance::seek` does for Ogg. | `seek_ignores_play_speed` |
| 0029 | — (found porting milestone B) | The loop wrap seeks with `mScratch` as its discard buffer, but `mScratch` then holds the frames of the voice resampled earlier in the same mix call. Any loop point above 0 replaced those frames (the rest of the previous block, up to 511 frames) with discarded source audio. The wrap now has a scratch of its own. | `loop_point_keeps_earlier_frames` |
| 0030 | — (found porting milestone B) | `soloud.h` defines `M_PI` as `3.14159265359` when `<math.h>` has none (MSVC), but glibc's is exact, so every use (the pan law, 3D, filters, generators) differed between Windows and Linux; a hard-panned voice leaked about 1e-12 into the other side on Windows. The fallback now has full precision. | `hard_pan_is_silent_on_the_far_side` |
| 0031 | — (found in production; the cause of a game's NaN audio) | Voice filters run at the voice's own rate, which falls with its play speed. `BiquadResonantFilter` at or above that rate's Nyquist frequency put its poles outside the unit circle, and its output grew to infinity and NaN, which then poisoned the whole mix. The frequency is limited to 0.49 times the rate (NaN included) when the coefficients are computed; the parameter keeps its value. | `biquad_above_voice_nyquist_stays_finite` |
| 0032 | — (found porting milestone D) | `LofiFilterInstance` kept state for 2 channels but filtered up to 8, so channels 2 to 7 read and wrote past the instance. State now covers `MAX_CHANNELS`. | `lofi_filters_eight_channels` |
| 0033 | — (found porting milestone D) | An `EchoFilter` delay set or faded to 0 (or below one frame) made the delay line 0 frames long: `% 0` crashed with a division by zero. The line is at least one frame. | `echo_with_zero_delay_survives` |
| 0034 | — (found porting milestone D) | `WaveShaperFilter` accepts amounts down to -1, where the curve's denominator `1 + k|x|` is 0 at `|x| = 1` (0 / 0 gives NaN) and negative beyond `-1 / k` for other negative amounts. Past the pole the output is 0. | `waveshaper_minimum_amount_is_finite` |
| 0035 | — (found porting milestone D) | `DCRemovalFilterInstance` read its length from the parent filter at the first mix, after `play` returned, so a filter destroyed after play was read after being freed. The length is copied when the instance is created, like every other filter's settings. | `dc_removal_outlives_its_filter` |
| 0036 | — (found porting milestone D) | A `DCRemovalFilter` length whose frame count does not fit an `int` converts to `INT_MIN`, so channel 1 was written at a negative offset. The window is at least one frame, like the echo's. | `dc_removal_long_length_is_safe` |
| 0037 | — (found porting milestone D) | Every time-domain filter's `setParams` tested `x <= 0` and similar, which NaN passes, and a NaN setting filtered the voice to NaN. The checks now reject NaN. | `filter_params_reject_nan` |
| 0038 | — (found porting milestone D) | For a one-sample buffer (a global filter in a one-frame mix) `BiquadResonantFilter` copied the sample before the buffer into it. A single sample is now left dry. | `biquad_single_sample_stays_in_bounds` |
| 0039 | — (found porting milestone D) | `FreeverbFilterInstance` asserted on mono audio (a breakpoint even in MSVC release builds) and, without asserts, processed the next block of the buffer as the right channel. Mono audio is now left unchanged. | `freeverb_leaves_mono_unchanged` |
| 0040 | — (found porting milestone D) | A new freeverb instance copied the filter's settings into its parameters without marking them changed, so the reverb model kept its own defaults (room size, damping and width from `setParams` were ignored) until a parameter was set. All parameters now apply at the first filter call. | `freeverb_uses_its_settings` |
| 0041 | — (found porting milestone D) | `FreeverbFilterInstance::filter` and `RobotizeFilterInstance::filterChannel` never called `updateParams`, so `fadeFilterParameter` and `oscillateFilterParameter` did nothing on them. | `freeverb_and_robotize_fade_parameters` |
| 0042 | — (found porting milestone D) | Robotize's period, `(int)(rate / frequency)`, is 0 for a frequency above the rate, negative for a negative frequency and `INT_MIN` for 0: `% 0` crashed, and the stream time in samples overflowed `int` after 13.5 hours. The period is at least 1 and the phase start uses `fmod` in double. | `robotize_period_and_phase_are_safe` |
| 0043 | — (found porting milestone D) | The flanger advanced its LFO once per channel per frame, so each channel got a different sweep and a stereo flanger ran twice as fast as set. All channels now start from the same phase. | `flanger_channels_share_the_lfo` |
| 0044 | — (found porting milestone D) | A flanger delay of 0 (by fade or set) made the line 0 samples long (`% 0` crashed), and a delay past the `int` range made its length `INT_MIN`. The line is at least one sample, and the delay is converted once and used for both the line and the sweep. | `flanger_delay_extremes_are_safe` |
| 0045 | — (found porting milestone D) | `BassboostFilter`, `FlangerFilter` and `FreeverbFilter` `setParams` accepted NaN, as 0037 fixed for the others. | `more_filter_params_reject_nan` |
| 0046 | — (found porting milestone D) | `Soloud::seek` on a bus discards audio through the generic seek, which calls `BusInstance::getAudio` with passes of the engine's scratch size divided by the bus's channels and uses that as the channel stride. `panAndExpand` then asserted (a breakpoint in MSVC builds) or faulted in its aligned SSE loads for strides that are not a multiple of 16, and voices with more channels than the bus were written past its one-block scratch. Such calls are now mixed one block at a time. | `bus_seek_mixes_in_blocks` |
| 0047 | — (found porting milestone D) | `Bus::getApproximateVolume` checked the channel with `>` instead of `>=`, so channel 8 of an 8-channel bus read past the volume array. | `bus_volume_channel_in_range` |
| 0048 | — (found porting milestone D) | The active voice list is only recomputed when a mix starts, so a bus seek after a voice was stopped and another played into its slot mixed the new voice, whose resample buffers are not assigned yet, through a null pointer. `mixBus_internal` skips voices without resample buffers. | `bus_seek_skips_voices_without_buffers` |
| 0049 | — (found porting milestone D) | `QueueInstance::getAudio` moved to the next queued sound only when the current one reported `hasEnded`. A looping sound never does, and once read to its end gives no audio, so the audio thread spun forever holding the engine mutex. A sound that gives no audio is now dropped like one that ended. | `queue_looping_source_does_not_hang` |
| 0050 | — (found porting milestone D) | A queued sound with fewer channels than the queue left the queue's other channels of the resample block as they were, which two blocks back held the previous sound's audio. Those channels are now silent. | `queue_silences_missing_channels` |
| 0051 | — (found reviewing milestone D) | `Fader::setLFO` made the half range positive and centred the oscillation on `aFrom` plus it, so `oscillate*` from a higher value to a lower one ran between `aFrom` and `aFrom + abs(aTo - aFrom)`: a volume oscillation from 1 to 0 reached 2. The half range keeps its sign, so the oscillation runs between the two values and moves toward `aTo` first; oscillations upward are unchanged. | `lfo_from_above_to_stays_in_range` |
| 0052 | #314 | When a source block ran out, `mixBus_internal` tested the frame after the last one it would write to stay inside the block, so it always wrote one frame too few; unless the rest of the block was a whole number of steps, the next block then started at its first sample, skipping up to one step of source per 512 frames. A 44.1 kHz sound on a 48 kHz engine played about 0.09% fast (0.1 s in three minutes, as reported). The check now tests the last frame written, in the mixing and inaudible-tick paths. Every resampled voice's output changes. | `resampled_playhead_does_not_drift` |
| 0053 | — (found porting milestone E) | Every `WavStream` instance reopens the file `load` named, and ignored `DiskFile::open`'s result: a file deleted or locked after loading left the instance reading a null `FILE`, and `fread` crashed. The instance now has no file, and `hasEnded` reports an instance without one (also when its decoder failed to start) as ended. | `wavstream_missing_file_ends` |
| 0054 | — (found porting milestone E) | WAV, FLAC and MP3 streams take their length from the header, and `hasEnded` waits for the read position to reach it. A file whose data ends early read fewer frames, so the position never got there and a voice that does not loop played silence forever. A short read now moves the position to the end. | `wavstream_truncated_data_ends` |
| 0055 | — (found reviewing milestone E) | After 0053 a stream without a file reported itself ended, but the mixer never stops a looping voice, so a looping `WavStream` whose file was gone played silence forever. `hasEnded` now also clears the instance's looping flag, so the voice ends. | `wavstream_missing_file_ends` |
| 0056 | — (found reviewing milestone E) | `voiceHasFinished` checked the looping flag before asking the source, so 0055's flag clearing only happened when a block was read; a voice that cannot advance (a step of a whole block or more) never reads one and stayed alive. The source is now asked first; for every other source the result is unchanged. | `wavstream_missing_file_ends` |
| 0057 | dr_libs 713ffe6, a4774b5 (dr_flac 0.12.35) | dr_flac summed an LPC prediction in 32 bits whenever the sample size plus the prediction shift was at most 32, ignoring the coefficient precision and order, so a predictor that overshoots full scale wrapped around and decoded wrong samples (found porting milestone E on a 20-bit stereo file's side channel). Backported upstream's test, `bitsPerSample + precision + ilog2(order) > 32`, in the scalar, SSE4.1 and NEON paths. | `flac_lpc_prediction_uses_64_bits` |
| 0058 | — | A rewound Ogg Vorbis stream kept the Vorbis frame it was reading, whose buffers stb_vorbis reuses, so a looping Ogg stream played the rest of that frame, by then holding other audio, before its start; `stb_vorbis_seek` leaves the rest of the frame holding its target in a buffer that `stb_vorbis_get_frame_float` skips, so a seek went on after its target. Seeks now land where the generic seek does (from the frames read, or from the start after a rewind) via `stb_vorbis_seek_frame`, decoding up to the target when that fails; a seek past the end ends the stream; a packet that fails to decode ends the data, as in `Wav::loadogg` (the end also counted the last read twice). | `oggstream_rewind_restarts`, `oggstream_seek_lands_on_target`, `oggstream_seek_past_end_ends` |
| 0059 | — (found porting milestone E) | stb_vorbis reads an Ogg page without segments as `fread(data, 0, 1)`, and `Soloud_Filehack_fread` divided the bytes read by the element size of 0, so a `WavStream` of an Ogg file cut inside its first page crashed with an integer division by zero. An empty request now reads nothing, as `fread` does, and the file fails to load. | `oggstream_cut_first_page_fails` |
| 0060 | — (found porting milestone E) | The inaudible-tick path of `mixBus_internal` called `getAudio` until the block was full; a looping voice whose source gives no audio (a loop point at or past its end) returned 0 every time, so the mix call never returned. It now stops when a read returns nothing, as the mixing path does. | `inaudible_tick_empty_loop_ends` |
| 0061 | — (found porting milestone E) | The inaudible-tick path did not clear the rest of a block that ended short, as the mixing path does, so a voice ticked to its end and then made audible again (by `update3dAudio`) played the stale samples left in its resample buffer after its last frame. It now clears them. | `inaudible_tick_clears_short_block` |
| 0062 | — (found porting milestone E) | `doppler()` projected the velocities on the listener-to-source vector, where OpenAL 1.1's formula, which SoLoud's documentation cites, uses source-to-listener: approaching sources played lower and receding ones higher (a source approaching at 100 units per second played at 0.774 instead of 1.41). The offset is negated first, so the shift goes the right way; a source with no velocity on the line between them is unchanged. | `doppler_follows_openal` |
| 0063 | #97 (follow-up) | Once a source or listener reached the speed of sound over the Doppler factor, `doppler()` divided by 0 or less, or computed 0 / 0, and gave infinity or NaN as the play speed. The mixer renders such a step as silence (patch 0022), but the stream position advanced by infinity or NaN and never recovered. A result that is not a finite value of 0 or more is now 0, which holds the voice in place until it slows down. | `doppler_at_sound_speed_stays_finite` |
| 0064 | — (found porting milestone E) | `set3dSoundSpeed` rejected only speeds of 0 or less, so NaN and infinity were accepted: either made the Doppler value of every 3D voice away from the listener NaN (infinity over infinity), and NaN made distance delays an undefined `int` conversion. They now return `INVALID_PARAMETER`. | `sound_speed_rejects_nan_and_infinity` |
| 0065 | — (found porting milestone E) | `play3dClocked` computed the distance delay from the source's position alone, ignoring the listener, where `play3d` subtracts the listener's position (unless the source is listener-relative). It now does the same. | `play3d_clocked_distance_delay_from_listener` |
| 0066 | #97 (follow-up; found reviewing milestone E) | Patch 0063 held a voice only when `doppler()` came out infinite or NaN, but the projected speeds were first clamped to the speed of sound over the Doppler factor, and for some factors `f32` rounding left the divisor a hair above 0: with a factor of 1.032 and a source approaching at 400 it was about 3e-5, so the voice played about 11 million times too fast and raced through its sound while the mixer rendered silence. The hold is now decided before dividing, on the speeds times the factor: a source approaching, or a listener receding, at or past the speed of sound holds the voice, and any other divisor is above 0 (a value that still overflows also holds). A still voice with a negative factor now plays at 1 instead of being held. | `doppler_past_sound_speed_holds_despite_rounding` |
| 0067 | — (found porting milestone F) | `Soloud::getApproximateVolume` checked the channel with `>` instead of `>=`, as patch 0047 fixed for buses, so channel 8 of an 8-channel engine read past the volume array (into the wave data after it). | `approximate_volume_channel_in_range` |
| 0068 | — (found porting milestone F) | With visualization enabled, a mix of no frames (`mix(buffer, 0)`, which still advances fades) took the short-mix path and computed `i % 0`, a division by zero that crashed the process. A mix of no frames now keeps the last visualization data. | `visualization_survives_an_empty_mix` |
| 0069 | — (found porting milestone F) | `NoiseInstance::getAudio` divided its sum by the sum of the octave weights, so weights summing to 0 (all 0, or 1 and -1) made every sample infinite or NaN, silencing the whole mix. Such weights now give silence. | `noise_zero_octave_sum_is_silent` |
| 0070 | — (found porting milestone F) | `WavStream::load`, `loadMem`, `loadFile` and `loadFileToMem` replaced the memory file and file name that playing instances read without stopping them, so those voices read freed memory. They now stop the stream's voices first, as `Wav`'s loaders do. | `wavstream_reload_stops_voices` |
| 0071 | — (found porting milestone F) | `Vic::setRegister` and `getRegister` indexed the 4 registers with any `int`, writing and reading the noise table after them and the clocks before them. An index out of range is now ignored and reads 0. | `vic_register_index_in_range` |
| 0072 | — (found porting milestone F) | `Sfxr::loadParamsFile` read fields one after another into the settings and succeeded whatever the file held, so a short file loaded with some settings changed. Fields are now read into a copy kept only when the file holds all of them; otherwise `FILE_LOAD_FAILED`. | `sfxr_short_file_fails` |
| 0073 | — (found porting milestone F) | `Sfxr::loadParamsFile` read the filter flag's byte straight into a `bool`, so a byte other than 0 or 1 made a `bool` that is neither. It is now read as a byte; any value but 0 is true. | `sfxr_filter_flag_is_a_bool` |
| 0074 | — (found porting milestone F) | `SfxrInstance::getAudio` called again after the sound's end, as the generic seek does while discarding, went on past the envelope's last stage and read `env_length[3]`. An ended sound now returns 0 frames. | `sfxr_ended_sound_stays_silent` |
| 0075 | — (found porting milestone F) | `Monotone`'s constructor left the song's track count and order uninitialised, and a failed load kept the track count it had read (or the previous song's) without pattern data: playing either read through a null pointer. The song now starts empty and a failed load empties it. | `monotone_empty_song_is_silent` |
| 0076 | — (found porting milestone F) | `Monotone::loadFile` ignored how many bytes each read gave, so a short file loaded with its missing bytes taken from the buffer's earlier, or uninitialised, contents. A short read now fails with `FILE_LOAD_FAILED`. | `monotone_short_file_fails` |
| 0077 | — (found porting milestone F) | `Monotone::loadFile` ended a Pascal title or comment at the length in its first byte, up to 255, in a 200-byte buffer holding the 41 bytes read: over 40 it took stale bytes, over 198 it wrote past the buffer. The length is now at most 40. | `monotone_long_title_is_cut` |
| 0078 | — (found porting milestone F) | Voices keep 12 channels and 12 hardware channels, but a song could have 255 tracks and `setParams` took any number of hardware channels: both overran the arrays. More than 12 now fail (`FILE_LOAD_FAILED`, `INVALID_PARAMETER`). | `monotone_at_most_12_channels` |
| 0079 | — (found porting milestone F) | `MonotoneInstance::getAudio` indexed the 800-entry note table with notes up to 126 (and arpeggios 7 above) times 8, read the order past its 256 entries when a song had no end marker, and read patterns the song does not have. The table's last entry is used past its end, the order starts again after its last entry, and a missing pattern plays as empty rows. | `monotone_indices_in_range` |
| 0080 | — (found porting milestone F) | `Monotone`'s constructor filled the note table below note 1 down to entry 2 only; vibrato on note 1 reads entry 1, which was uninitialised. The table now continues to entry 1. | `monotone_note_table_is_filled` |
| 0081 | — (found porting milestone F) | `MonotoneInstance::getAudio` took `floor(rate / 60)` samples a tick, so below 60 Hz (a slow play speed) it took the sample count modulo 0. A tick is now at least a sample. | `monotone_slow_rate_does_not_divide_by_zero` |
| 0082 | — (found porting milestone F) | `MonotoneInstance`'s constructor left each channel's frequencies and slide uninitialised, and a slide to a note on a channel's first row started from them. They now start at 0. | `monotone_first_slide_starts_at_zero` |
| 0083 | — (milestone F plan) | `TedSid::loadFile` picked the SID model from bits 4 and 5 of `mFlags`, the source's own 3D flags, instead of the flags byte just read from the dump, so every dump played as a 6581 unless the source had those 3D settings. | `tedsid_model_from_dump` |
| 0084 | — (milestone F plan) | `TedSid::loadFile` freed the ops without stopping the voices, which then read freed memory. It now stops them first, as the destructor does. | `tedsid_reload_stops_voices` |
| 0085 | — (milestone F plan) | `TedSid::loadFile` used the header's sizes as they were: no chunks asked `new[]` for a negative size, a loop position at or past the end read past the ops, a data offset past the file's end read a negative length, and a short file left ops uninitialised; the loop position's halves were read in one expression, in either order. Such dumps now fail with `FILE_LOAD_FAILED`, the halves are read in turn, and the ops are zeroed and set only once loaded. | `tedsid_header_is_checked` |
| 0086 | — (milestone F plan) | `TedSidInstance::tick` reads ops until one waits, so a dump whose loop held no wait, or only waits of 0, spun forever on the audio thread. Such dumps now fail with `FILE_LOAD_FAILED`. | `tedsid_loop_without_wait_fails` |
| 0087 | — (milestone F plan) | `zx7_decompress` read and wrote without bounds, so a dump's compressed chunks could write past its ops and read past its data. A new `zx7_decompress_bounded` stops at both ends and a chunk going outside fails the load. | `tedsid_zx7_stays_in_bounds` |
| 0088 | — (found porting milestone F) | `SIDsound::doEnvelopeGenerator`: an attack from level 0xFF (a gate turned off and on while sustaining at full level) counted on to 0x1FF, reading `envGenDRdivisors` past its end. The level now wraps from 0xFF to 0, as in reSID. | `sid_attack_from_full_level_wraps` |
| 0089 | — (found porting milestone F) | `SIDsound::reset` set `envCounterCompare`, an index into `RateCountPeriod`, to `RateCountPeriod[0]` (0x7F00), so until a gate or rate write every envelope step read far past the table (a crash in GCC builds). It now starts at 0. | `sid_envelope_rate_starts_in_range` |
| 0090 | — (found porting milestone F) | `SNDCHIP`'s constructor left the tone, noise and envelope counters, the noise output, the chip type and the registers uninitialised, so each voice's first periods (and two voices of one dump) depended on whatever memory the chip was built in. They now start at 0. | `ay_chip_starts_at_zero` |
| 0091 | — (milestone F plan) | `AyInstance::getAudio` wrote the right channel `aSamplesToRead` floats after the left instead of `aBufferSize`, as patch 0004 fixed for `WavStream`. | `ay_right_channel_at_buffer_stride` |
| 0092 | — (milestone F plan) | `Ay::loadFile` replaced the ops its voices were reading without stopping them, and leaked the old ones. | `ay_reload_stops_voices` |
| 0093 | — (milestone F plan) | `Ay::loadFile` used the header's sizes as they were, as `TedSid` did before patch 0085 (no chunks asked `new[]` for a negative size, a data offset past the end, a short file, the loop position's halves read in either order). The loop position, which Ay does not use, is not checked. | `ay_header_is_checked` |
| 0094 | — (milestone F plan) | `Ay::loadFile` decompressed zx7 without bounds; it now uses `zx7_decompress_bounded` (patch 0087), and the unbounded `zx7_decompress` is removed. | `ay_zx7_stays_in_bounds` |
| 0095 | — (milestone F plan; extended while porting) | `ChipPlayer` divided by the CPU speed and the chip speed / 8, its frames of `(cpuspeed * 8172 / 44100) / 2` ticks never advanced below 11 Hz (spinning on the audio thread), and a chip clock below 8 x 44100 Hz rendered a chip tick's samples at once, which could overrun the 8192-sample buffer. Such dumps now fail with `FILE_LOAD_FAILED`. | `ay_speeds_are_checked` |
| 0096 | — (found porting milestone F) | `ChipPlayer::play` stopped at the end of the ops before playing the op it had just read, so a dump's last register write was dropped. | `ay_last_write_is_played` |
| 0097 | — (found porting milestone F) | `ChipPlayer::play` played a wait whole within its frame, so a wait longer than about 8000 samples overran the buffer onto its own samples (and rendered at once on the audio thread). A frame now takes at most twice its ticks of a wait; dumps without such waits play as before. | `ay_long_wait_is_spread` |
| 0098 | — (found porting milestone F) | `AyInstance::hasEnded` turned true once the ops were read, with up to a frame of samples still buffered, cutting every dump's end. | `ay_tail_is_played` |
| 0099 | — (milestone F plan) | `AyInstance::rewind` only went back to the first op: the chips kept their state and the buffer its samples, so loops did not restart the dump and seeks back were offset. It now starts a new player. | `ay_rewind_restarts` |
| 0100 | — (milestone F plan) | `Speech::setParams` took any speed: 0 or below gave klatt frames of no or a negative number of samples, so `SpeechInstance` asked `new[]` for a negative size or played nothing, and a speed past the `int` range of 11025 * speed did the same. Speeds now give frames of 1 to 11025 samples (up to 1000 ms); others return `INVALID_PARAMETER` and change nothing. | `speech_speed_is_checked` |
| 0101 | — (milestone F plan) | `SpeechInstance` sized its sample buffer for the speed at play, but `getAudio` and `rewind` took `Speech`'s speed every time: after `setParams` lengthened the frames, a long element wrote past the buffer. A voice now keeps its speed; the frequency, declination and waveform still reach playing voices. | `speech_voice_keeps_its_speed` |
| 0102 | — (milestone F plan) | `klatt::synth`'s stress lookahead read a duration past the end of the elements: `darray`'s zero-filled spare usually held it, but an element array that filled its allocation read past the heap block. It now reads 0, as the spare gave. | `speech_stress_lookahead_in_bounds` |
| 0103 | — (found porting milestone F) | `klatt::phone_to_elm` looked each phoneme up together with the byte after it, also after the last: phonemes that filled their allocation read past the heap block, and a byte there could make the last phoneme a two-byte one. The byte past the end now reads as 0. | `speech_phoneme_lookup_in_bounds` |
| 0104 | — (found porting milestone F) | `klatt::phone_to_elm` read a byte that is no phoneme from the position after it and skipped that one too: stress marks in `[phonemes]` never applied, the phoneme after one was dropped, and an unknown last byte read past the phonemes. Text through the rules is unchanged. | `speech_stress_marks_apply` |
| 0105 | — (milestone F plan) | `VizsnInstance` took its noise from the C runtime's `rand()`, shared by the process: each voice continued where the last left off, and MSVC and glibc gave different noise. Each voice now runs MSVC's generator, seeded 1. | `vizsn_voices_are_deterministic` |
| 0106 | — (milestone F plan) | A `Vizsn` played before `setText` read its text through a null pointer. Its voices now end at once and play silence. | `vizsn_without_text_ends` |
| 0107 | — (milestone F plan) | `VizsnInstance::hasEnded` turned true once the last phone was buffered, with up to 800 of its samples unplayed, and the read that reached the end of the text filled its block with old samples. Voices now end once the buffer is played, and the rest of the block is silent. | `vizsn_tail_is_played` |
| 0108 | — (found porting milestone F) | `VizsnInstance::getAudio` buffered whole phones until the read was covered, so a read of more than 1248 samples (a seek's discard pass, or a loop's seek of 4096) wrote past its 2048-sample buffer. It now buffers one phone at a time. | `vizsn_long_reads_stay_in_bounds` |
| 0109 | — (milestone F plan) | `VizsnInstance` had no `rewind`, so seeks back failed and loops could not restart. `rewind` now restarts the voice as a new one, noise included. | `vizsn_rewind_restarts` |
| 0110 | — (found porting milestone F) | `VizsnInstance::getAudio` said it filled every read, also past the end of the text, so the mixer never saw the voice end: a looping Vizsn never looped and played silence. The read now ends with the text. | `vizsn_loops` |
| 0111 | — (milestone F review) | A looping `Sfxr` restarts at its envelope's end through `resetSample`, which set `period = (int)fperiod` without `getAudio`'s lower bound of 8: a base frequency past about 10 (damaged files, random settings) made the period 0, and the next supersample's `phase %= period` divided by zero. `resetSample` now bounds it at 8 too, which changes the rest of the restarting sample for periods of 1 to 7. | `sfxr_looping_high_base_freq_survives` |
| 0112 | — (milestone F review) | `TedSidInstance::tick` runs every op up to the next wait, so a dump of many writes between short waits made unbounded work for each sample (a loop of 5000 writes and a wait of 1: 5000 writes a sample). A dump is now refused if any 1024 samples (4.6 ms) of it run more than 8192 ops, counting the loop and waits of 0; a 6502 at the Plus/4's 1.77 MHz makes at most about 2048 register writes in that time. | `tedsid_busy_dumps_fail` |
| 0113 | — (milestone F review) | The AY chip works for every tick of its clock, and `Ay` took any clock: at 4294967295 Hz a voice took about 7 times real time. The clock is now at most 4 MHz: SoLoud's dump tools write 1774400 (ZX Spectrum) and 2000000 (Atari ST), and the fastest of these chips takes a 4 MHz input clock (the YM2149, with its divider). | `ay_chip_clock_is_bounded` |
| 0114 | — (milestone F review) | `ChipPlayer` plays a frame's ops at once, taking ops until its waits reach the frame's ticks: 33.5 million writes ran in one frame (about 135 ms), again at every restart of a looping voice. A dump is now refused if more than 131072 ops come before waits of two frames' ticks (a long wait carries a frame to twice its ticks), counting restarts unless the dump never waits; a Z80 at 3.5 MHz writes at most about 28000 registers in those 0.19 s. | `ay_busy_dumps_fail` |

The fork is public at [kelo221/soloud](https://github.com/kelo221/soloud)
(`master`, pushed from the maintainer's working branch). Its README lists every
fix with credits. Commits after 0017 on that branch add build scripts and that README,
so reference fixes are made on the `solouder-reference` branch (0017 plus code
fixes only, in a worktree next to the fork), exported with
`--ref solouder-reference`, and cherry-picked onto the working branch. Patches 0018 to
0050 were pushed on 2026-10-12 (public `master` at `7b91e3b`), and 0051 after
them (`1784537`). 0052 to 0056 are not pushed yet, nor are 0060 to 0068.

The shipped native libraries are built from the same branch: Windows with
`build_windows.bat` (WASAPI, WinMM, Null, static runtime), Linux with
`build_linux.sh` (MiniAudio, Null; built in Ubuntu 20.04 so it needs at most
glibc 2.29), and macOS with `build_macos.sh` (CoreAudio, Null; universal
arm64 + x86_64). The macOS build needs Apple's SDK and runs on a Mac.

### Still open in the reference

Not fixed by a patch yet. Each needs a Rust requirement and test; reference
fixes are welcome as new patches.

| Upstream | C++ location | Defect | Rust requirement / test |
|---|---|---|---|
| #396 (reported), #156 | `WavStreamInstance::seek` fallback | Non-Ogg streams seek by rewind + decode-and-discard, which is O(seek distance). Upstream reports seeking a stream handle silently kills it. | Done: each codec seeks to the frame the reference's generic seek lands on, bit for bit: WAV by frame, Ogg Vorbis by its frames (patch 0058), FLAC and MPEG audio by decoding up to the target; a seek never ends a voice that has audio left. Seeks while playing and while paused, from a paused play and after playing, match offline and through the `Controller` (`stream_wav`, `stream_ogg`, `seeks_continue_from_the_requested_frame`, `seeks_while_paused_resume_from_the_requested_frame`). The reference's fallback stays O(seek distance) for FLAC and MPEG audio, and so does the port's. PR #336 is a native-seek reference. |
| — (found 2026-10-12) | `Soloud::mix`, `Soloud::mixSigned16` | No bound on the frame count: a request for more frames than the scratch buffer holds (at least 4096, or the buffer size rounded up to 16) writes past `mScratch`. Parity scenarios stay at or below 4096 frames. | Done: `Mixer::mix_interleaved` renders long requests in passes of at most the scratch size (`mix_without_sources_is_silent`, `split_mixes_match_one_mix`). |
| — (found 2026-10-12) | `Queue::play`, `QueueInstance::getAudio` | A queued sound is an instance that reads its `Wav`'s samples; destroying the `Wav` while it is queued stops only its own voices, so the queue later reads freed memory. Queued instances are also never freed: `QueueInstance::getAudio` deletes the ones it finishes, but `Queue` has no destructor, so whatever is still queued leaks. | Done: a queued `Sound` holds its `Pcm`, and dropped queue entries go through retirement like ended voices (scenario `queues`). |
| — (found porting milestone E) | `Soloud::update3dAudio` | Step 2 (`update3dVoices_internal`, colliders and attenuators included) runs without the audio mutex, and step 3 applies its results to whatever voice is in each slot (`mVoice[voices[i]]`). If a voice was replaced in between, by another thread or by a collider that plays or stops sounds, the new voice gets the old one's gains, Doppler speed and kill decision, and step 2 has overwritten the new voice's `m3dData` with the old one's results. A fix would copy each voice's 3D inputs and handle under the lock in step 1 and apply only to the same handle in step 3; not patched, since it changes the locking of a call a shipping game makes every frame: for the maintainer. | Done: the controller computes from its own copy of each voice's 3D state, and the renderer applies a result only to the `VoiceId` (index and generation) it was computed for (`updates_reach_only_their_own_voice`); offline, `update_3d_audio` holds `&mut Mixer`, so nothing can intervene. |
| — (found 2026-10-18) | `WavStream::loadFile`, `WavStreamInstance` | Every instance reads through the caller's one `File` and seeks the same cursor, so two voices of the stream corrupt each other's reads; lengths and seeks are 32-bit. | Done: `WavStream::from_file` takes a `StreamFile` that each voice opens on its own (`voices_read_their_own_cursor`). Single-voice scenarios (`file_loading`) match. |
| — (found 2026-10-18) | `WavStream::load*` under `Queue::play` | Patch 0070 stops a reloaded stream's voices, but an instance waiting in a queue is not a voice: it goes on reading the freed memory file. | Done: a queued `Stream` holds its source; a `WavStream` cannot be reloaded. Scenarios do not reload queued streams. |
| #307, #241 | `Soloud::deinit`, `Soloud::~Soloud` | `SOLOUD_ASSERT(!mInsideAudioThreadMutex)` in `deinit` can fire at shutdown; ordering of `stopAll` versus backend teardown. | `Drop` is safe in every state; test repeated init/teardown. See the shutdown lesson below. |

Also reported upstream and worth a test once the feature is ported: #274
(intelligibility with many voices), #351 (many plays into a bus starting late),
and #163 (valid, non-zero-volume voices that are silent). Done (milestone F
unit 11): the soak test's probe covers #351 and #163 (a burst into a bus up to
the active limit starts within a few renders, fully active and audible; past
the limit exactly the limit is active) and #274 as far as a test can (output
finite, bounded and fully mixed).

### Behavior decisions to record

These are reference behaviors that are surprising rather than memory-unsafe.
For each, `docs/port-status.md` must say whether Rust preserves it, changes it,
or has not decided, with a test either way.

| Upstream | Reference behavior |
|---|---|
| #381 | `Soloud::play` defaults `aVolume` to `-1` (use the source volume); `Soloud::play3d` and every `Bus::play*` default to `1.0`. Rust: `PlayOptions::volume` of `None` uses the source volume for every play call: changed (`buses` passes 1 explicitly). |
| #395 | Nested buses reduce gain compared to direct playback with the same settings. Rust: preserved (each bus voice pans its mix again; `bus_mixes_its_voices_through_its_own_pan`, scenario `buses`). |
| #280 | Stopping a bus handle tears down every voice playing through that bus. Rust: preserved, including SoLoud's exception for a bus that never looked up its handle (`bus_is_silent_until_its_handle_is_looked_up`, scenario `buses`). |
| #240 | Hard-coded inaudibility thresholds: `0.001` in `update3dAudio` and `0.01` in `play3d`/`play3dClocked` (`soloud_core_3d.cpp`). Rust: preserved (`play_and_update_use_different_thresholds`, scenario `spatial_3d`). |
| #175, #203 (reported) | Pops or wrong orientation when 3D voices become audible or between `update3dAudio` calls. Volume/pan ramps are per mix call, so change rate depends on device buffer size. Rust: preserved, including a paused voice snapping to the channel volumes it had before `update3dAudio` set the new ones (`paused_voices_jump_to_their_new_volume`, `spatial_3d`). |
| — (3D update order) | `update3dAudio` computes every 3D voice's gains (running colliders and attenuators) before applying any, so a 3D bus killed for being inaudible stops its voices only after their colliders ran. Rust: preserved, offline and through the `Controller` (`a_killed_bus_stops_its_voices_after_their_colliders_ran`, the `count` collider in `spatial_custom`); units 6 and 7 applied each voice as it was computed, which the milestone E review found. |
| — (3D volume) | The 3D attenuation multiplies both the voice's overall volume and each channel's speaker gain, so a voice at distance gain `g` plays at `g²`. Rust: preserved for parity (a shipping game is tuned to it; `spatial_3d`). |
| #94, #105 (open question for the maintainer) | **What they say.** #94 (closed 2015-05-08): the first `lookatLH` negated the `at` vector, so sounds in front of the listener attenuated like sounds behind it; the reporter proposed flipping the side vector instead, and that is the code today (`2d69070`, two days after the flag was added and before any release had it). #105 (open, 2015-06-26, against the fixed code): the flag "negates one column" of the look-at matrix only and gave wrong 3D perception under Direct3D coordinates; the reporter's workaround is to leave the flag off and negate every z passed to sources and the listener, and asks for a z-negation flag; the #94 reporter suggests instead passing the listener's forward, side and up vectors explicitly. **What the code does.** `lookatLH` is `lookatRH` with its side row (`up × at`) negated. That is exactly the right-handed basis of the scene with every z negated: positions, `at`, `up` and velocities, bit for bit (checked by `left_handed_mirrors_z` and in `spatial_custom`, which renders both). With the default speakers (left at +x in the listener's frame) a source to the right in a left-handed, +z-forward, +y-up world plays on the right speaker; so does the right-handed path in a -z-forward world. **Whether a fix is needed.** #105's workaround and the flag give the same output, so the report could not be reproduced and its proposed z-negation flag would duplicate `LEFT_HANDED_3D`; its "one column" is the side row, which is the correct difference between the two conventions. A world with another axis convention (such as +x left) is not served by either. **What a fix would change.** Making the flag negate z on input, as #105 asks, changes no sample. Negating any other axis instead would swap left and right (or front and back) for every caller of the flag, including the shipping game. The explicit forward/side/up API would be an addition, not a change, and would let other conventions work. Rust: preserved as `MixerConfig::left_handed_3d`, bit-exact, no reference patch; the decision to patch or document is the maintainer's. Bevy is right-handed (-z forward), so unit 8 uses the default. |
| #98 | Relative play speed and Doppler compose: the voice plays at the Doppler factor times its set relative speed, and `setRelativePlaySpeed` keeps the Doppler factor. Rust: preserved, including speed fades and sample rate changes (`doppler_composes_with_play_speed`, `spatial_doppler`). |
| — (Doppler direction, patch 0062) | The reference shifted pitch the wrong way (approaching lower, receding higher), against OpenAL 1.1 and its own documentation. Fixed on both sides as a defect; for the maintainer: a shipping game that sets source or listener velocities hears the corrected direction after patch 0062. Reverting the patch (and `doppler.rs`) restores the old direction. |
| #97 | Doppler at or past the speed of sound: the reference never hung (the mixer skips a zero step), but infinite or NaN speeds poisoned the voice's position. Rust holds the voice silent and in place until the source slows down, as patched reference 0063 does, decided before dividing as patch 0066 does (`the_speed_of_sound_holds_the_voice`, `faster_than_sound_holds_whatever_the_rounding`, `spatial_doppler`). |
| — (distance delay overflow) | `set3dDistanceDelay` converts the delay to an `int` frame count; a delay of `INT_MAX` frames or more (13.5 hours at 44.1 kHz) is undefined in C++ and wraps to `INT_MIN` on x86, which the unsigned delay counter takes as 2,147,483,648 frames, so the voice starts about 13.5 hours late at 44.1 kHz (a clocked play adds its delay to that, wrapping). Rust: preserved (`c_int` conversion as x86 does); no reference patch, since only absurd distances or sound speeds reach it. |
| #76 | A handle can become invalid while audio is still audible on some backends; define validity as "until mixed out". |
| #117 | `getFilterParameter` returns error codes in the value channel; Rust uses `Result`/`Option`. |
| #318 | WAV `smpl` loop points are ignored. |
| #276 | Intro-then-loop scheduling via `setDelaySamples` leaves gaps; Rust scheduling should be frame-accurate. |
| #291, #286 | A second `stop` on a queue is ignored; no API empties a queue. Rust: stopping a queue's voice keeps its queued sounds, which continue when it plays again, as in the reference (scenario `queues`); emptying is not added. |
| #252 | A finished handle cannot be restarted. |
| #177 | Single listener only (matches [ARCHITECTURE.md](ARCHITECTURE.md#spatial-audio)). Rust: preserved; the Bevy plugin uses the `AudioListener` with the lowest entity index when there are several, reports it in `ListenerState`, and keeps the last pose when there is none, or when its transform is degenerate (`several_listeners_use_the_lowest_entity_index`, `despawning_emitters_and_listeners`, `switching_listeners`, `a_degenerate_listener_keeps_its_last_pose`). |
| — (dr_wav) | 8-bit WAV maps 0..255 linearly onto -1..1 (`x * 2/255 - 1`), so silence (128) decodes as +1/255. Rust centers on 128: changed (`wav_decode_u8`). |
| — (patch 0012) | A WAV data chunk that ends early keeps its declared length, padded with silence. Rust keeps only the frames present: changed (`short_data_chunk_keeps_the_frames_present`). |
| — (dr_wav) | Non-finite float samples (NaN, infinity) in WAV files pass through and poison every voice they are mixed with. Rust decodes them as silence: changed (`non_finite_floats_decode_as_silence`). |

Milestone F decisions, taken on 2026-10-18 by the coordinating agent while
the maintainer was away. Each is reversible and awaits the maintainer's
review:

| Area | Decision |
|---|---|
| Openmpt | The four `Openmpt::*` rows are excluded: the source binds an external libopenmpt DLL at run time, cannot be tested for parity, and would add a native dependency. A module-player crate can be wrapped in the `Generator` trait instead. |
| Vizsn `rand()` | Patched in the reference to a per-instance MSVC-style LCG seeded with 1, so voices, scenarios sharing a runner process, and MSVC versus glibc no longer change its output. |
| TedSid SID model | Taken from `mFlags`, the source's 3D flags, instead of the file's flags: a defect, fixed in the reference. The libraries a shipping game uses are not changed by this work. |
| Live generator parameters | Shared as atomics, so a change reaches playing voices at their next block, as the reference behaves. |
| Queued generators | Generators are `Queueable`; the reference queues any source. |
| File readers | `Wav::loadFile`, `WavStream::loadFile` and `loadFileToMem` are ported as proposed under the milestone F units (each stream voice gets its own cursor). |
| Truncated sfxr files | Patched in the reference to fail the load (`FILE_LOAD_FAILED`) without changing the settings; Rust returns an error. |
| Cross-OS output | The Windows output is the golden; Linux and macOS must match it within 1e-6. CI configuration only (SoLouder is not pushed anywhere). |
| Benchmarks | A plain `std` harness, no new dependency. |
| Reference patches | Numbered sequentially; nothing is pushed. |
| Large data files | `Elements.def`, the text-to-speech rules and the SID tables stay data files embedded at compile time (`include_str!`/`include_bytes!` or code generated by `build.rs` into `OUT_DIR`), not hand-written `.rs` files over 400 lines. |

### Fixed upstream: keep as regression tests

These fixes were already upstream; Rust must not reintroduce them.

- **#183, #251, #253:** `clip` and `panAndExpand` SIMD paths dropped or misread
  the tail when the frame count was not a multiple of 4 (e.g. 735 frames at
  44.1 kHz and 60 fps). Test prime and `n % 4 ∈ {1, 2, 3}` lengths.
- **#224, #319:** stream position advances by `buffer time × relative speed`
  and generic seek stores the absolute requested time.
- **#97:** large Doppler velocities made `step_fixed` zero (division by zero).
  The Rust mixer renders a zero step as silence; patch 0063 also keeps the
  Doppler factor finite (`the_speed_of_sound_holds_the_voice`).
- **#228:** `trimVoiceGroup` deadlocked by relocking the audio mutex.
- **#135, #229:** `playClocked` offset handling for the first event and `time == 0`.
- **#160:** reloading a source resets its channel count.
- **#201, #210:** lengths are frames ÷ rate, not interleaved samples ÷ rate.
- **#322:** voice-group allocation size.
- **#95:** `play3d` starts paused, computes the voice's 3D state immediately,
  then unpauses, so the first frame is attenuated.
- **#136, #238:** `play`, `play3d`, and volume changes while paused set the
  current channel volume to its target instead of ramping from silence.

### Lessons from a Unity integration

A Unity game ships the patched libraries through C# bindings. Its crash
analyses gave these findings, which constrain the Rust design:

- **Shutdown crashed in `Soloud_deinit`.** `stopAll` only flags voices; the
  device thread keeps mixing while `deinit` frees filter instances. The game
  works around it with pause → stop → wait about one device buffer → deinit,
  through a single ordered shutdown path. Rust: stop and join the callback
  before releasing mixer state (see
  [callback constraints](ARCHITECTURE.md#callback-constraints)).
- **Filters were shared across engines.** One filter object attached to sources
  on several per-listener engines made teardown order matter. Rust: filter
  definitions are immutable; every voice/bus owns its instance state.
- **Back-pointers outlive engines.** `AudioSource::mSoloud` is not cleared when
  an engine is destroyed, and `~AudioSource` stopped voices only on the last
  engine that played it. The game "abandons" sources rather than destroying
  them at exit; patch 0025 makes destroying them safe. Rust: no source-to-engine pointers; voices hold `Arc` PCM.
- **One engine per listener** (sometimes two at once) exposed process-global backend
  state (patch 0001). Rust: device state is per engine.
- **Filters can produce NaN.** Source filters run at the voice's rate before
  resampling, so a slowed voice can push a biquad cutoff above Nyquist; the
  game clamps cutoffs and play speed and stops all voices when a channel
  exceeds 32× full scale. Rust: validate filter parameters against the actual
  processing rate and keep non-finite output out of the mix.
- **Speed changes zipper.** The game fades out, changes speed, and fades in.
  Test that speed ramps are click-free.
- **3D uses `LEFT_HANDED_3D`** with Unity coordinates passed straight through,
  and is driven by unscaled time because mixing runs in wall-clock time.
  Consistent with "audio time is not game frame delta".
- **Voice cap.** The game capped `setMaxActiveVoiceCount` at 255 because the
  shipped DLL hit a release-build assert at 256 or more; patch 0011 fixes the
  cause. The game's cap can be raised once its DLL is rebuilt.
- **Null backend for capture.** The game renders offline with the null driver
  and explicit pulls, which is the reference harness this guide prescribes. The
  "nosound" backend was not compiled into the macOS build.

### Out of scope for the Rust port

- Backend-specific reports for SDL/SDL2/SDL3, ALSA (#364, #278), OpenSL ES,
  WinMM, XAudio2, miniaudio, vitasdk, and similar. Preserve behavior only through
  the [output lifecycle](ARCHITECTURE.md#callback-constraints) requirements.
- CMake, GENie, and compiler-warning pull requests (for example #403, #397,
  #377, #389, #387).
- Language bindings and the C API generator.
- Codec and feature requests (AAC #394, Opus #354, MIDI #195, HRTF #204).
  Ducking (#69) and module playback (#328, #367) apply only if those sources are
  ported.
- PR #169 (faster WAV loading) tolerates short reads; do not adopt it.

## Milestones and acceptance gates

Implement vertical, compiling slices rather than translating everything before
running it. The proposed workspace/dependencies and integration contracts are
in [ARCHITECTURE.md](ARCHITECTURE.md#proposed-workspace-and-dependency-direction).
The ledger's `milestone` column assigns every C API behavior to one of these.

| Milestone | Required result |
|---|---|
| A — Foundation and reference harness | Patched reference and C++ regression tests (done); parity ledger (done); Cargo workspace with pinned Rust/Bevy versions (done); `scripts/check_all.py` and a CI workflow, verified locally on Windows and in a Linux container but not yet run on a CI host (done); scenario format with a C++ null-driver runner and a Rust runner (done, first scenarios: init validation and silence) |
| B — Offline playback (done) | Prepared PCM and decoded WAV files (done; `solouder-decoders`), planar mixer for 1 to 8 channels (done), handles (done), play/stop/pause/loop (done; loop points in D), volume/pan (done), point/linear/Catmull-Rom resampling (done), clipping and global volume (done); 12 parity scenarios for playback and WAV decoding, bit-identical on Windows except the 8-bit WAV conversion |
| C — Windows and Bevy playable slice (done) | Bounded wait-free control, events and audio retirement between game and audio thread, allocation-free renders (done; `solouder_core::realtime`, `realtime_no_alloc`); CPAL negotiation of rate, channels and sample format, panic-to-silence callback, device error states, safe shutdown order (done; `solouder-cpal`, `play_wav` example, checked on a Windows WASAPI device); Bevy plugin with `.wav` asset loader, playback entities, settings, status and messages, stop on despawn, headless mode (done; `bevy_solouder`, `play_and_despawn` example) |
| D — Routing and DSP parity (done) | Nested buses, queues/groups, virtualization/protection, scheduling, remaining resamplers, voice/bus/global filters (done; see the [review](#milestone-d-review)) |
| E — Spatial and streaming parity (done) | Listener/emitter systems, attenuation/Doppler/delay, workers, seek/loop regressions, WAV/FLAC/Ogg Vorbis/MP3 (done; see the [review](#milestone-e-review)) |
| F — Sources and portability | Generators/speech/music, FFT/visualization, Linux/macOS checks, stress/benchmarks, support matrix |

### Milestone D units

D is delivered in these units, in this order; each ends with passing
scenarios, ledger rows and a commit.

1. **Fades and scheduling** (done): `fade*`, `oscillate*`, `schedulePause`,
   `scheduleStop`, `playClocked`, the global volume ramp into the clipper,
   and the engine getters. Scenario `fades`.
2. **Voice controls and groups** (done): the playback rows left from B (loop
   point, seek, autostop, inaudible behavior and ticking, pause all, absolute
   pan, channel volume, sample rate, single instance, stop/count by sound,
   `playBackground`, raw 8/16-bit loads, length, 16-bit output, version),
   then voice groups, which apply every voice control. Patches 0026 to 0030.
   Scenarios `voice_controls`, `channel_gains`, `sound_instances`,
   `voice_groups`.
3. **Filter framework and time-domain filters** (done): per-voice filters at
   the voice's rate, global filters, live parameters with fades and
   oscillations; biquad, lofi, echo, waveshaper and DC removal. Patches 0031
   to 0038. Scenarios `filters_voice`, `filters_global`, `filter_params`.
4. **FFT and the remaining filters** (done): the FFT, FFT filter, bassboost,
   robotize, flanger and freeverb. Patches 0039 to 0045. Scenarios `fft`,
   `filters_spectral`, `filters_modulation`.
5. **Buses** (done): buses as playable sources, nested routing, bus filters,
   channels and resampler, `annexSound`, active voice counts, visualization
   and bus seeking. Patches 0046 to 0048. Scenarios `buses`, `bus_seek`.
   `Bus::play3d` and `play3dClocked` come with 3D audio in milestone E.
6. **Queues** (done): queues as playable sources, gapless queued sounds,
   the 32-sound limit, parameters, seeking forward and looping queues.
   Patches 0049 and 0050. Scenario `queues`.
7. **Real-time control and review** (done): controller commands for the
   new controls, filters, buses and queues; voices and filter instances
   prepared on the game thread; replaced filters and rejected queue sounds
   retired to it. Tests `routing_tests` and the routing section of
   `realtime_no_alloc`.

Plan work inside a milestone as small parity units: one behavior, its
scenarios, the Rust implementation, and a passing comparison. Order units
along the mix pipeline (source read, resampling, pan/expand with ramps, bus
mix, filters, clipping, output conversion) so each unit builds on verified
stages.

Regression tests for [tracked defects](#reference-patches-and-upstream-tracking)
belong to the milestone that introduces the affected behavior:

- **B:** partial block reads across mix calls (`split_mixes_match_one_mix`;
  the streamed-source stride of patch 0004 belongs to E), frame counts not
  divisible by 4 (#183, #251, patch 0018: `volume_pan_ramps`,
  `leftover_frames_continue_the_volume_ramp`), zero-step and resampler
  progress (patches 0008 and 0022, #343, #97:
  `extreme_speed_renders_silence_instead_of_hanging`), active caps above 255
  (patch 0011: `more_than_255_voices_can_be_active`), voice stealing and
  all-protected tables (patch 0020). Done. Inaudible-tick voices (patch 0006)
  moved to D with inaudible behavior
  (`ticking_voices_over_the_active_cap_still_get_resample_blocks`).
- **C:** init failure, repeated init/teardown, and callback-before-state
  shutdown order (patch 0010, #307, the Unity teardown lessons). Done:
  `device_lifecycle` (`repeated_open_play_close`,
  `dropping_while_playing_stops_cleanly`, `two_outputs_run_independently`,
  `invalid_voice_count_fails_before_touching_the_device`), `plugin_lifecycle`
  (`device_apps_start_play_and_tear_down_repeatedly`,
  `dropping_a_device_app_mid_playback_returns_promptly`,
  `without_an_output_players_fail_and_the_app_keeps_running`,
  `despawning_before_the_clip_loads_never_starts_a_voice`), and the callback
  unit test `dropping_the_callback_returns_the_renderer`. Device tests skip
  without hardware unless `SOLOUDER_REQUIRE_DEVICE=1`.
- **D:** protected-voice ranking (patch 0007: `active_voices`,
  `loudest_and_protected_voices_are_active`), bus default volume and nested
  gain (#381, #395, #280: `buses`, `bus_mixes_its_voices_through_its_own_pan`),
  queue stops (#291: `queues`), filter stability at the voice processing rate
  (patch 0031, `filters_voice`,
  `biquad_above_the_voice_nyquist_frequency_stays_bounded`). Done. The voice-group defects (#228, a deadlock in
  `trimVoiceGroup_internal`; #322, its allocation size) cannot occur in Rust:
  groups are `Vec`s on the mixer's single owner (`voice_groups`).
- **E:** non-Ogg seek/loop and the streamed channel stride (patches 0002
  and 0004: `stream_wav`, `stream_flac` and `stream_mp3` wrap looping
  streams from a loop point several times;
  `a_looping_stream_split_at_its_end_wraps_on_the_worker`), looping streams
  not ending (patch 0005: `stream_wav`, `prebuffered_streams_render_like_the_mixer`),
  stream seek (patch 0009, #396: seeks back, forward and past the end in
  every `stream_*` scenario, and while paused in `stream_wav` and
  `stream_ogg`; `seeks_continue_from_the_requested_frame`,
  `seeks_while_paused_resume_from_the_requested_frame`,
  `rewind_and_skip_land_where_decoding_does`,
  `skipping_past_the_end_of_an_ogg_stream_ends_it`; the skip reads into a
  scratch of a whole block and stops at a short read, so 0009's loop on a
  small scratch cannot occur), decoder metadata validation and fuzzing
  (patch 0012: `rejects_unplayable_layouts`, `bad_headers_are_rejected`,
  `bad_files_are_rejected`, the nine-channel files both sides reject in
  `stream_wav`, and `mutated_streams_never_panic` for WAV, FLAC, Ogg
  Vorbis and MP3), long-duration drift (#314, patch 0052:
  `resampled_playhead_does_not_drift`), 3D initialization (patch 0013:
  `play_3d_in_a_reused_slot_uses_its_own_flags`; #95:
  `play_3d_attenuates_the_first_frame`; both in `spatial_3d`). Done.

### Milestone D review

Checked against the gates below on 2026-10-12:

- **Parity:** all 64 milestone D ledger rows are `ported`. The 30 scenarios
  render the same samples as the Windows reference build, bit for bit
  (subnormals counted as zero; `wav_decode_u8` keeps its documented
  tolerance). On Linux (GCC, Ubuntu 20.04) the reference differs only in
  `filters_spectral`, in the last bits of the FFT filter's math functions.
- **Reference defects:** patches 0026 to 0051 fix the defects milestone D
  reached in the C++ reference, each with a C++ regression test; the D
  regression list above is covered.
- **Real time:** every D control is reachable through `Controller` and
  renders the same samples as the offline mixer; renders neither allocate nor
  free with filters, buses and queues, including mixers split after offline
  use. Limits are listed in
  [port-status.md](port-status.md#real-time-coverage): voice groups and most
  queries are offline-only, seek errors are not reported, and a filter whose
  first use needs more memory than was reserved (a voice sped up before its
  first block, a flanger delay above 0.1 s) allocates during the render.
- **Review:** an independent review of unit 7 found six defects: bus filter
  swaps prepared for the bus's settings rather than its playing voice (a
  flanger could index past its line), delays changed before a filter's first
  use allocating or changing samples, uncounted retirements in mixers split
  after offline use, `keep_retired` missing voices played after it, and
  voices played out by a seek reported as stopped. Each is fixed and has a
  test (`layout_tests`, the `first_use` section of `realtime_no_alloc`). A
  re-check of those fixes found four more, also fixed with tests: a failed
  bus replay hiding the playing bus voice's layout, `split` not preparing
  the filters a mixer already held, surplus voice boxes from offline
  overflow, and SoLoud's oscillation range when `from` is above `to`, which
  is patch 0051.
- **Structure:** `cargo clippy -D warnings`, `cargo fmt --check`, the
  400-line limit, the ledger check and the C++ regression tests pass
  (`check_all.py --reference --parity`).

### Milestone E units

Planned 2026-10-13, in mix-pipeline order (the source read first, panning
last); each ends with passing scenarios, ledger rows and a commit:

1. **Long-playback drift (#314)** (done; patch 0052): measure a voice's position against the
   analytic one over minutes of 44.1 → 48 kHz playback in the reference. A
   real drift is fixed by a reference patch before later fixtures exist.
2. **Streamed WAV, offline** (done; patches 0053 and 0054): `Stream` in the
   core plays any `StreamSource`, and each voice opens its own
   `StreamReader`; `solouder_decoders::WavStream` implements both for WAV
   files, from a file, from bytes and read into memory. Length, seek, rewind,
   looping and loop points match the reference (`stream_wav`, which also
   covers the loop-wrap defects of patches 0002, 0004 and 0005). The offline
   mixer decodes inside the mix, as the reference does, so scenarios stay
   bit-exact; the real-time controller refuses streams until unit 3, and
   queues accepting streams moved there too.
3. **Streaming in real time:** decoding moves to a worker (one per output)
   that fills a bounded PCM ring per streaming voice; prebuffering,
   underflow silence counted in `Status`, seek and loop flushes tagged with
   an epoch, decoders and files dropped on the worker; streamed Bevy assets.
   Prebuffered renders match the offline mixer and do not allocate. In
   three steps, each committed on its own:
   - **3a, feeds** (done): `split` also makes a `StreamWorker` (taken once from the
     controller; the application runs it on a thread, tests call
     `service()`). Playing a `Stream` through the `Controller` makes a
     *feed*: a pool of fixed-size chunks (one block of frames each, allocated
     on the game thread), a ring of full chunks to the audio thread and one
     of empty chunks back. The worker opens the reader, fills chunks in
     order and stamps each with its epoch and first source frame; at the end
     of the audio it marks the short chunk and, when the voice loops,
     carries on from the loop point's frame, so a loop wrap finds its audio
     waiting. A seek on the audio thread (a loop wrap included) whose target
     frame is the next chunk's start continues; any other bumps the epoch,
     returns the queued chunks and asks the worker for the frame. A voice
     whose feed has not filled yet waits silently without advancing
     (prebuffering); one whose feed runs dry plays silence without
     advancing and counts the frames in `Status`. Looping and loop-point
     changes reach the worker through the feed's atomics. An ended voice's
     feed goes back to the controller with the voice, and the worker drops
     the reader (and its file) once it sees the feed closed.
   - **3b, queued streams** (done): `Queue::play` and `Controller::queue_play`
     accept streams, offline and through feeds.
   - **3c, Bevy** (done): streamed assets play through the worker, which the
     plugin runs on its own thread.

   A review of unit 3 led to patches 0055 and 0056 (a looping stream without
   its file never ended, in both, also when the voice could not advance) and
   these changes: `split` moves streams the mixer
   was already reading (voices and queued) onto feeds; a seek publishes its
   request before handing back stale chunks, and both that and each worker
   pass stop after a pool's worth of chunks; a queue skipping audio not
   decoded yet asks for the frame after it; a seek after silent waiting
   counts from the audio, not the voice's position; feeds start with the
   voice's looping, so short loops are decoded past their end at once; a
   dropped worker ends its voices once their audio is played, and the
   controller then refuses streams with `NoStreamWorker`; a looping reader
   moved by `split` at its end wraps at once. A seek after waiting counts
   from the block read ahead, as the reference's generic seek does, so it
   lands where it would have had the audio arrived.
4. **FLAC**, resident and streamed (done): Symphonia's decoder, bit-exact
   against dr_flac in scenarios `flac_decode` and `stream_flac`, from 8 to
   32 bits and 1 to 8 channels; malformed-input fuzzing; more than 8
   channels rejected (patch 0012). `decode` and `WavStream` pick FLAC by its
   `fLaC` tag, as the reference does, and the Bevy loader takes `.flac`.
   Generated fixtures (`scripts/gen_flac_fixtures.py`) cover every subframe
   type, both residual codings, stereo decorrelation and damaged files.
   dr_flac 0.12.31 summed some LPC predictions in 32 bits and wrapped
   around (20-bit and wider side channels, for example); patch 0057
   backports dr_flac 0.12.35's fix. A frame with a bad checksum, data that
   ends early and an unknown length are handled differently on purpose
   (port-status.md). Fuzzing found two Symphonia panics on damaged files,
   both integer overflows that only debug builds check: its seek, which
   SoLouder no longer uses (a seek decodes up to the target, as the
   reference's generic seek does, and a rewind reopens the file), and
   mid/side decorrelation, now caught and treated as a damaged frame
   (fixture `bad_overflow_*`). Both are worth reporting upstream.
   Review fixes: a run of damaged frames no longer shifts the frames after
   it in a stream; a frame overlapping audio already decoded is trimmed
   the same way resident and streamed; variable blocking whose sizes are
   equal decodes (Symphonia rejected it); metadata blocks are bounded by the
   file length; a voice rewinds its own file handle, and a direct stream
   reader that cannot rewind ends the voice, as the worker already did.
   Known limits: Symphonia panics on 33-bit side subframes (32-bit stereo)
   with overflow checks on, so debug builds drop those frames, and 32-bit
   mid/side at high amplitude is wrong in dr_flac and Symphonia alike.
   dr_flac fixes after 0.12.35 (fuzz fixes, 0.12.43's buffer overflow) are
   not backported.
   **4b. Own FLAC decoder** (done): Symphonia is replaced by a port of
   dr_flac 0.12.31's decoding path as SoLoud builds it (`DR_FLAC_NO_CRC`,
   strict open, no metadata callback) with patch 0057, in
   `solouder-decoders/src/features/decoded_audio/flac/`: a buffered bit
   reader, frame headers, constant, verbatim, fixed and LPC subframes, Rice
   residuals, 32 and 64-bit prediction and dr_flac's scalar decorrelation,
   in its wrapping 32-bit arithmetic, so no input panics in any build
   (200,000 mutated files in a debug build). It also reproduces a dr_flac
   quirk on x64: a 33-bit side sample read with one bit left in a 64-bit
   cache line gets that bit in bit 0 (`s32_stereo_44100` pins it). The
   policies above are kept, now with frame and header checksums of its own.
   `bad_overflow_*` stays out of the parity scenarios: dr_flac's SSE2 and
   scalar conversions disagree on overflowed samples, and which one runs
   depends on read sizes. Decoding speed matches Symphonia's (5 minutes of
   16-bit stereo in about 150 ms in release builds) and the output equals
   ffmpeg's on ffmpeg-encoded files. Symphonia leaves the workspace; E5
   decides how Ogg Vorbis and MP3 are decoded. dr_flac's later fixes concern
   its C buffer handling and Ogg/seek-table code, which the port does not
   have.
5. **Ogg Vorbis and MP3**, resident and streamed, with `Wav::load`'s format
   detection (`OggS`, `RIFF`, `fLaC`, anything else tried as MPEG audio)
   and Bevy loaders for `.ogg` and `.mp3`. As for FLAC (4b), the decoders
   are ports, not crates, so the scenarios are bit-exact instead of
   declaring a tolerance: stb_vorbis 1.22's pull-mode decoder in
   `decoded_audio/vorbis/` and dr_mp3 0.6.31 (minimp3) in
   `decoded_audio/mp3/`, with layers 1 to 3. On x64 dr_mp3 always runs its
   SSE path, whose float operations are not ordered like its scalar path;
   the port follows the SSE order. dr_mp3 0.6.31 reads no Xing/LAME or ID3
   headers, so encoder delay and padding stay in the audio, as in SoLoud.
   Fixtures come from `scripts/gen_lossy_fixtures.py` (ffmpeg with
   libvorbis, libmp3lame and libtwolame; the encoded files are committed,
   the damaged ones derived). Scenarios `ogg_decode`, `mp3_decode`,
   `stream_ogg` and `stream_mp3`. Patch 0058 fixes the reference's Ogg
   stream: a rewind kept the Vorbis frame it was reading, so loops replayed
   the rest of it (holding other audio by then) before the start, and
   `stb_vorbis_seek` skipped the rest of the frame holding its target.
   Seeks now land where the generic seek would, found with
   `stb_vorbis_seek_frame` instead of decoding up to them; a packet that
   fails to decode ends the data, as it ends `Wav::loadogg`, and a seek
   past the end ends the stream. Patch 0059: an Ogg file cut inside its
   first page crashed `WavStream` (a division by zero in SoLoud's file
   wrapper).

   Done: both ports are bit-exact in all four scenarios and against wider
   C++ harnesses (MP3: 95 generated files through memory and reader paths;
   Vorbis: 20 more ffmpeg files, a 10-channel file and over 1,000 mutated
   ones). In release builds MP3 decodes about 10% slower than the C++ (5
   minutes of 192 kbps stereo in about 190 ms against 172 ms, 11 ms of it
   zeroing the output) and Vorbis slightly faster (200 seconds of stereo
   in about 123 ms against 135 ms). The SSE order matters in dr_mp3's
   `DCT_II` sums and its mono output stores. stb_vorbis reads SoLoud's
   `DiskFile` and `MemoryFile` differently at the end of the data (a
   `DiskFile` reads one phantom zero byte past it), so streams from a file
   and from memory read through the matching kind. stb_vorbis's inverse
   MDCT is wrong for block sizes 64 and 128; the port keeps that for
   parity. Undefined behaviour on damaged input is not reproduced: dr_mp3
   decodes damaged frames into stack memory it never initialised (26 of
   7,790 mutated files differ; the port zeroes it), and stb_vorbis reads
   and writes out of bounds or asserts on some damaged files, where the
   port fails the packet or the file. Fuzzing found no panics. Vorbis setup
   headers needing more than 256 MiB are refused.
6. **3D core, offline** (done; patches 0060 and 0061): listener and source
   state, attenuation models with
   min/max distance and rolloff, listener-relative sources, speaker positions
   and per-speaker gains for 1 to 8 channels, `update3dAudio` and its 0.001
   inaudibility threshold, and `play3d` (first frame attenuated, #95; its
   0.01 threshold; slot reuse, patch 0013).

   Done: `features/spatial` in the core, bit-exact in `spatial_3d` and
   `spatial_speakers`. The vector math stays in `f32` in SoLoud's order,
   with `sqrt` and `pow` widened to `f64` as MSVC's `math.h` does; the C++
   runner flushes subnormals once it has mixed, so the scenarios keep the
   3D math away from them. `play3d` and `update3dAudio` set a voice's
   volumes in opposite orders, so a paused voice snaps to its old channel
   volumes in an update; both are kept. Voices made inaudible by distance
   now reach the inaudible-tick path, which advances them without
   resampling, filters or panning; it hung on a looping source without
   audio (patch 0060) and left stale samples after a voice's end
   (patch 0061). The 3D attenuation applies twice (above). The `Controller`
   gained the 3D calls in unit 8.
7. **Doppler, distance delay and the rest** (done; patches 0062 to 0065):
   sound speed and Doppler factors,
   Doppler composed with play speed (#98) and at extreme velocities (#97),
   distance delay, `play3dClocked`, `Bus::play3d`, colliders and attenuators
   (Rust traits; test subclasses in the C++ runner), and `LEFT_HANDED_3D`
   (#94, #105; a reference patch only with the maintainer's agreement, since
   a shipping game uses the flag).

   Done: bit-exact in `spatial_doppler`, `spatial_delay` and
   `spatial_custom`, with `spatial_3d` and `spatial_speakers` unchanged.
   The reference's Doppler went the wrong way (patch 0062), gave infinite
   or NaN play speeds at the speed of sound (patch 0063: the voice holds
   still), accepted NaN and infinite sound speeds (patch 0064), and
   `play3dClocked` measured distance delay from the origin (patch 0065).
   Doppler multiplies the voice's set play speed (#98). `play_3d` and its
   clocked and bus forms take a velocity; `play_3d_clocked` honours
   `PlayOptions::paused`. Colliders and attenuators are `Arc` traits; the
   collider sees a `SpatialVoice` instead of the engine and the voice's 3D
   data. The parity runners pick test colliders and attenuators by name
   (`wall`, `shell`, `square`, `cutoff`) with the same `f32` math on both
   sides. `LEFT_HANDED_3D` is ported as it is, as
   `MixerConfig::left_handed_3d`: it equals the right-handed scene with
   every z negated, bit for bit, so #105's workaround is the flag; the open
   question is recorded above. Distance delays past `INT_MAX` frames wrap
   as on x86, to `INT_MIN`, which the unsigned delay takes as 2^31 frames.
   Every 3D ledger row is `ported`, on the offline `Mixer`.
8. **3D in real time and in Bevy, then the review** (done; patch 0066;
   reviewed): the controller runs the 3D math on the game thread, as
   the reference does outside its mixer, and sends each voice's result in
   one batched command, so colliders never run on the audio thread. Bevy
   listener and emitter components from `GlobalTransform`, one listener,
   tested axes and handedness, and velocities that survive zero time steps
   and teleports. An independent review closes the milestone.

   Done: the `Controller` keeps a copy of the mixer's listener, speakers
   and speed of sound from `split`, and of each 3D voice's settings.
   `play_3d`, `play_3d_clocked`, `bus_play_3d` and `bus_play_3d_clocked`
   compute the voice's first gains and distance delay there and send them
   with the play request, so the voice starts placed, before its first
   frame; the 3D setters change only the copy; `update_3d_audio` computes
   every voice, colliders included, and sends the results in one
   preallocated batch (`RealtimeConfig::spatial_batches`, 2 by default, in
   flight), which the renderer applies to the voices that still have those
   IDs and hands back. Voices killed for being inaudible finish with
   `Stopped`; colliders see `Handle::INVALID` and `SpatialVoice::voice`.
   `realtime_no_alloc` covers 3D plays, updates, kills, buses, clocked
   plays and Doppler. Every spatial scenario replays through the controller
   and matches the mixer bit for bit (`solouder-parity`'s `realtime` test),
   in full or up to the first step the controller cannot express. Bevy:
   the `spatial` slice takes one snapshot of the `AudioListener` and every
   `SpatialEmitter` after transform propagation; the playback sync starts
   emitters' sounds with `play_3d` and sends at most one update per frame. Bevy's
   axes are SoLoud's defaults (right-handed, -Z forward, +Y up), so
   positions pass unchanged; units are metres and the speed of sound 343.3
   m/s. Velocities are derived over virtual time unless `AudioVelocity` is
   set, and are 0 for zero or paused time steps, `AudioTeleport` and speeds
   past `SpatialAudioSettings::max_derived_speed`.

   Folded in from the review of units 6 and 7: `update_3d_audio` now
   computes every voice before applying any, as `update3dAudio` does (a
   killed 3D bus stopped its children before their colliders ran); patch
   0066 decides the Doppler hold before dividing (0063's check missed
   rounding-sized divisors); the runners gained the `count`, `meta` and
   `handle` colliders and the `raw` attenuator, with slot-reuse and
   listener-relative cases, so a collider or attenuator given the wrong
   data fails parity; the distance delay overflow is documented as 2^31
   frames late, not "at once", with 511, 512 and 513-frame delays around
   mix and callback boundaries; a 3D voice's filters are reserved at its
   starting speed times its starting Doppler factor (a later rise still
   allocated on the audio thread; the milestone review removed that); and
   `spatial_sources` plays streams, a queue and buses in 3D with colliders
   and attenuators. Found while porting: the reference's `update3dAudio`
   applies results to whatever voice is in a slot (still open, above).

### Milestone E review

Checked against the gates below on 2026-10-18, after an independent review
of units 6 to 8 (`237e77f..8053127`) and the fixes for its findings:

- **Parity:** 31 of the 34 milestone E ledger rows are `ported`. Three
  carry over, as milestone B carried `Soloud::init`: the `WavStream`
  lifecycle stays `in-progress` (dropping a `Stream` does not stop its
  voices, each of which owns its reader), and `WavStream::loadFile` and
  `loadFileToMem`, which read through SoLoud's custom `File` interface,
  are `todo`. The 44 scenarios render the same samples as the Windows
  reference build, bit for bit (subnormals counted as zero;
  `wav_decode_u8` keeps its documented tolerance), and every `spatial_*`
  scenario also replays through the `Controller` bit for bit, in full or
  up to the first step it cannot express (pinned by line). The Linux
  reference build was not rechecked for milestone E.
- **Reference defects:** patches 0052 to 0066 fix the defects milestone E
  reached in the C++ reference, each with a C++ regression test, and the
  E regression list above is covered. Still open, for the maintainer:
  `update3dAudio` applying results to whatever voice holds a slot (the
  port cannot hit it), `LEFT_HANDED_3D` (#94, #105; ported as it is), and
  the O(seek distance) fallback seek of #396.
- **Real time:** streams play through the `StreamWorker` and every 3D call
  through the `Controller`; renders neither allocate nor free with
  streams, 3D plays and updates, including filters whose Doppler factor
  rises after the play. Limits are listed in
  [port-status.md](port-status.md#real-time-coverage): voice groups and
  most queries are offline-only; as in milestone D, a voice sped up by a
  setter before its first block, or a flanger whose delay grows past
  0.1 s, allocates in the render (on a 3D voice, until the next update
  grows its filters); a stream sought while playing continues one block
  of silence later than offline, by design; and the update after
  `spatial_batches` in flight is refused, so what is heard can lag by
  that many updates.
- **Review:** no critical findings. Five warnings, each fixed with a
  test: a zero-scale or NaN listener panicked Bevy debug builds through
  `GlobalTransform::forward` (directions now come from the transform's
  axes; `a_degenerate_listener_keeps_its_last_pose`); a Doppler factor
  rising after a 3D voice started made flanger, echo and DC removal
  filters allocate in the render, measured at 24,620 and 73,636 bytes
  (the controller now sends memory for them with the update and the
  renderer swaps it in with `FilterInstance::grow`; the Doppler sections
  of `realtime_no_alloc`, bit-exact in `spatial_tests`); derived
  velocities fluttered between 0 and 2.25 times the speed when entities
  moved in a fixed physics step (now measured over the time since the
  position last changed and held for `velocity_hold`, with physics users
  told to feed `AudioVelocity`; the `snapshot` unit tests and
  `motion_on_a_fixed_step_keeps_a_steady_pitch`); seeks of paused streams
  (#396, #156) were untested (`stream_wav`, `stream_ogg`,
  `seeks_while_paused_resume_from_the_requested_frame`); and
  `updates_reach_only_their_own_voice` would have passed with the ID
  mapping broken (the reusing voice is now 3D and nearby, with stopped and
  ended variants, checked by breaking the mapping). Seven notes, also
  addressed: an emitter with a non-finite transform started in 2D for good
  (it now waits); the update lag and the controller's collider order are
  documented; Bevy sent an update every frame while a 3D sound played
  (now only when something changed, `still_frames_send_no_update`); the
  parity replay's expected stops are pinned by line and refused requests
  stop it; new Bevy tests cover the first frame of a placed sound,
  distance delay, switching listeners and 300 emitters; and the crate docs
  name both sync systems. `AudioListener` and `SpatialEmitter` keep their
  names, for the maintainer to confirm.
- **Structure:** `cargo clippy -D warnings`, `cargo fmt --check`, the
  400-line limit, the ledger check and the C++ regression tests pass
  (`check_all.py --reference --parity`).

### Milestone F units

Planned 2026-10-18. Units run along the pipeline: the source contract, then
the sources from simplest to largest, then visualization (after clipping),
then portability, stress and review. Each ends with passing scenarios,
ledger rows and a commit. Line counts are of `reference/soloud/`.

Rules for units 1 to 8:
- **Where the code goes:** a new crate `solouder-sources` (slice `sources`), with
  one Cargo feature per source (upstream #268), built on a core
  `Generator`/`GeneratorInstance` contract. Instances are built on the game
  thread; `get_audio` runs on the audio thread, never allocates, and needs no
  `StreamWorker`. Parsers take `&[u8]`; each `loadFile` reads a `Read` to the end.
- **Math:** follow MSVC. A `float` passed to `sin`, `cos`, `pow`, `exp`, `tan`
  or `floor` is widened to `f64`; `double` state stays `f64`. Integer overflow
  that is undefined in C uses wrapping operations. Float-to-int casts go through
  a helper that returns `i32::MIN` when out of range, as `cvttss2si` does
  (Rust `as` saturates instead).
- **Subnormals:** the reference sets FTZ/DAZ (MXCSR `0x8040`), and Rust cannot
  set MXCSR soundly. Scenarios must include the decaying tails of the IIR
  generators (klatt, vizsn, sfxr filters); if DAZ changes later bits, record it.
- **Read sizes:** Noise and Speech reset state on every `getAudio` call, so Rust
  must ask for exactly the reference's sizes: 512-frame blocks, loop refills,
  seek passes of the scratch size, and queue reads. Scenarios cover loop wraps,
  seeks and queued play.
- **Live parameters:** in C++, playing voices re-read Vic's registers and model,
  Monotone's hardware channels and waveform, and Speech's parameters on every
  block. Rust shares them as atomics in an `Arc`, so a change lands at the next
  block, as in the reference.
- **Common settings:** every source carries `SourceSettings` and
  `SpatialSettings`. Each unit tests volume, looping, autostop, filters, play
  through the `Controller` and `realtime_no_alloc`. `getInfo` is answered
  wherever the source defines it.
- **Fixtures:** none ship with the reference. `scripts/gen_source_fixtures.py
  --check` writes Monotone songs, sfxr v100/101/102 files, CHIPTUNE dumps (raw
  and zx7, using a first-party encoder) and damaged variants.
- **THIRD_PARTY.md** already has rows for sfxr, speech, vizsn, VIC, TED/SID, AY
  and OpenMPT. Add rows for WELL512 (Chris Lomont, public domain;
  `src/core/soloud_misc.cpp`) and zx7 (`include/zx7decompress.h`, Jari Komppa,
  public domain/Unlicense, based on Einar Saukas's zx7, who asks to be credited;
  the zx7 compressor has a different licence). `legal.mmd` mentions neither AY
  nor zx7.

1. **Generator contract, file readers and Noise** (done; patches 0069 and
   0070): a generic seek (rewind, then
   decode and discard) and `rewind`; looping voices whose rewind fails end
   instead of playing silence forever. Port `Misc::Prg` (WELL512, 111 lines)
   and `noise` (132 + h 74). Rows: `Noise::*` (3), plus `Wav::loadFile`,
   `WavStream::loadFile` and `loadFileToMem` (see the file readers below).
   Noise is deterministic: the seed is `0xfade` per instance. Defects: an
   octave sum of 0 gives inf, then NaN (patch); the octave state restarts
   every call, so with 512-frame reads octave 9 never updates (preserve).
   Scenarios `noise`, `file_loading`.
   Done: `solouder_core::Generator`/`GeneratorInstance` and `Synth` (a
   `Playable` and `Queueable` source). A voice builds its instance when it
   is prepared, on the game thread; reads ask for exactly the reference's
   sizes (blocks, loop refills, seek passes of the scratch size, queue
   strides), and seeks and loop wraps discard into buffers the mixer
   already holds, so nothing allocates on the audio thread
   (`solouder-sources/tests/realtime_no_alloc.rs` also matches the
   `Controller` against an offline `Mixer`). `GeneratorInstance::info`
   answers `Soloud::getInfo`. The new crate `solouder-sources` has `prg` and
   `noise` features. File readers: `decode_reader` caps the read at
   `DecodeLimits::max_file_bytes` (8 bytes per sample plus 1 MiB) and
   reports `FileTooLarge` past it; `WavStream::from_file` takes a public
   `StreamFile`, and a path or `Arc<[u8]>` handed to it behaves exactly as
   `open` or `from_bytes` (a `DiskFile` or a `MemoryFile` in the
   reference), while other types read as a `MemoryFile`. The parity
   runners reload a `stream` name into the same C++ `WavStream`; a
   stream-replacement case is in `stream_wav`. Patch 0070 covers only the
   voices: a queued instance of a reloaded stream still reads freed memory
   (see "Still open in the reference").
2. **Vic** (done; patch 0071; 167 + h 108; Petri Häkkinen, zlib): integer phases and floats;
   44100 Hz is hard-coded into the phase step. Rows: `Vic::*` (5). Defect:
   `setRegister`/`getRegister` index past 4 registers (bounds patch; Rust returns
   `Option`/`Err`). Scenario `vic` writes registers between mixes.
   Done: `solouder_sources::Vic` shares its registers and model as atomics
   in an `Arc`, which its voices read at the start of every block; the
   noise table is computed at compile time. `VicModel` replaces the
   `int` model. Through the `Controller` with live register changes in
   the sources' `realtime_no_alloc`.
3. **Sfxr** (done; patches 0072 to 0074; 602 + h 158; Tomas Pettersson, zlib by permission): uses `Prg`,
   seeded `0x792352` per instance and `aRandSeed` for presets; the period is
   `double`, the filters `float`. Rows: `Sfxr::*` (6). Defects:
   `loadParamsFile` reads raw `int`/`bool`/`float` (a bool byte other than 0/1 is
   undefined), succeeds on a short file with partly updated settings, and v100
   keeps the old `freq_dramp`/`arp` values; the seed arithmetic overflows `int`;
   looping is handled inside the source (the instance's `LOOPING` flag).
   Scenario `sfxr`: all 7 presets × seeds, files, looping.
   Done: `solouder_sources::Sfxr` (feature `sfxr`) with `SfxrParams`,
   `SfxrPreset` and `SfxrError`; settings sit behind a mutex read when a
   voice is prepared, so voices copy them as SoLoud's do. Bit-exact
   with `pow` of `float`s widened to `double`. The truncated file and
   the `bool` byte are patched (0072, 0073); v100 keeping old values is
   preserved. Found while porting: `getAudio` called after the end (the
   generic seek does) read `env_length[3]` (patch 0074). The seed
   arithmetic wraps in Rust; MSVC and GCC wrap it too (not patched: no
   test can observe it without a sanitizer). Fixtures from
   `scripts/gen_source_fixtures.py`; prefixes and corrupted bytes of a
   file are tested to load or fail with a typed error, and extreme
   settings to render without panicking.
4. **Monotone** (done; patches 0075 to 0082; 434 + h 109; SoLoud proper): uses `generateWaveform` (already
   ported); tempo follows `mSamplerate`, which includes play speed. Rows:
   `Monotone::*` (5). Several memory-safety defects (patch them): a Pascal title
   length over 199 writes past `temp[200]`; the song is uninitialised before
   load (`mTotalTracks`); more than 12 tracks or hardware channels overrun the
   12-entry arrays; order, pattern and note indices are unchecked
   (`mNotesHz[800]`); a rate below 60 Hz gives `% 0`. Scenario `monotone`.
   Done: `solouder_sources::Monotone` (feature `monotone`) with
   `MonotoneSong`, `MonotoneError` and `MONOTONE_CHANNELS`. A song is
   parsed into an immutable `MonotoneSong` shared by its voices, so
   Rust has constructors instead of reloading (`load` stopped the voices
   in SoLoud); hardware channels and the waveform are atomics read every
   block. Patches: an empty song before and after a failed load (0075),
   short files failing (0076), titles cut at 40 bytes (0077), at most 12
   tracks and hardware channels (0078), note indices clamped to the
   table, the order wrapping after 256 entries and missing patterns
   playing empty rows (0079), and at least a sample a tick (0081). Found
   while porting: the note table's entry 1 was never set, and vibrato on
   note 1 reads it (0080); channel frequencies started uninitialised, and
   a slide to a note on a channel's first row starts from them (0082).
   The sample counter and slides wrap on overflow (after 13 hours, or
   days of sliding), as MSVC does. Bit-exact.
5. **TedSid, then Ay**, as 5a (done; patches 0083 to 0089) and 5b (done; patches 0090 to
   0099). TedSid: `soloud_tedsid` 222, sid 623+180,
   ted 236+43, tedplay (Unlicense). AY: `soloud_ay` 179, chipplayer, sndbuffer,
   sndchip and sndrender together 991 lines (SMT, public domain). Both use zx7
   (114). Integer emulation; the SID's filter tables use `tanh`/`exp` in
   `double` (platform libm). Rows: `TedSid::*` (4) and `Ay::lifecycle`. The C
   API has no `Ay_load` (#281); the runner uses C++. Defects:
   - TedSid picks the SID model from `mFlags` (the source's 3D flags) instead
     of the file's flags.
   - A dump without a wait spins forever in `tick` on the audio thread.
   - A loop position at or past the end reads beyond the data; `kchunks == 0`
     makes a negative `new[]`; zx7 reads and writes without bounds.
   - Reloading frees or leaks the register data under playing voices (no
     `stop()`).
   - Ay writes its right channel at stride `aSamplesToRead`, as in patch 0004;
     a CPU speed of 0 divides by zero.
   - `mLooppos` is read but never used; `rewind` keeps the chip state.
   Scenarios `tedsid`, `ay`, including `getInfo`.
   Done (5a): `solouder_sources::TedSid` (feature `tedsid`) with
   `SidModel` and `ChipDumpError`, over a `chipdump` module (the header,
   stored data and a bounded zx7 decompressor) that Ay will share. As
   for Monotone, a dump is parsed into an immutable value shared by its
   voices, so Rust has constructors instead of reloading. `getInfo`
   (`GeneratorInstance::info`) reports register `key & 127`; scenarios
   read it with the new `voice_info` operation. Patches: the SID model
   from the dump's flags (0083), voices stopped before a reload frees
   their ops (0084), header sizes, the data offset and the loop position
   checked, the loop position's halves read in turn and the ops zeroed
   (0085), loops without a wait refused (0086), and zx7 chunks
   decompressed within their bounds by a new `zx7_decompress_bounded`
   (0087; Ay keeps the old one until 5b). Found while porting: an attack
   from level 0xFF counted on to 0x1FF and read past the decay table, so
   the level now wraps at 8 bits, as in reSID (0088); and `reset` set
   each envelope's rate to the first period (0x7F00) instead of its
   index, which read far past the table (a GCC crash; 0089). Kept as in
   SoLoud: the filter's cutoff stays 0 until a cutoff register is
   written, a release from level 0 wraps to 255, and only the TED's
   square and noise are ported (SoLoud never selects its other
   waveforms). The filter tables use `tanh` and `exp` from the platform
   libm, like MSVC. Bit-exact.
   Done (5b): `solouder_sources::Ay` (feature `ay`) over the same
   `chipdump` module; the resampler, the chip and the player are ported
   in `features/ay/` with SoLoud's wrapping 32-bit arithmetic, and a
   voice's ring of 8192 samples is allocated once and reset in place on
   rewind. `getInfo` reports the first chip's registers 0 to 13 and the
   second chip's at bit 4. Patches from the plan: the right channel at
   the buffer's stride (0091), voices stopped and the old ops freed on a
   reload (0092), sizes checked as for TedSid (0093), bounded zx7, the
   unbounded decompressor removed (0094), speeds that divide by zero or
   never advance refused (0095), and a rewind restarting the chips
   (0099). Found while porting: SNDCHIP's counters, noise output, chip
   type and registers started uninitialised, so voices of one dump
   differed (0090, applied first so the other tests are
   deterministic); a chip clock below 8 x 44100 Hz renders more than a
   sample a chip tick and can overrun the ring, so 0095 also refuses
   those (real chips run at 1 to 2 MHz); the last register write of a
   dump was dropped (0096); a wait longer than a frame's room overran
   the ring (0097); and voices ended with up to a frame still buffered
   (0098). Plan change: the dump's loop position stays unused, as in
   SoLoud. Ay voices end, and looping goes through SoLoud's rewind, which
   seeks also use, so a rewind to the loop position would break seeks;
   the loop position is not checked against the length either. Bit-exact.
6. **Speech** (done; patches 0100 to 0104) (`soloud_speech` 148, klatt 1074+152, tts 1425, darray 92,
   resonator 117, `Elements.def` 1659; rsynth public domain, `soloud_speech.*`
   zlib). The noise generator is klatt's own LCG (seed 5); `PI` is a `float`
   literal. `init` runs on every `getAudio` and resets the glottal and
   low-pass state (preserve). Rows: `Speech::*` (3). Defects:
   - `setParams` reaches playing voices; a speed of 0 or below asks `new[]` for a
     negative size, and a faster speed overflows the sample buffer sized at play.
   - The stress lookahead reads past the element array (Rust reads 0, as
     `darray`'s zero-filled spare does).
   - The `ctype` functions get negative `char`s (Rust: non-ASCII is not alpha).
   - #337: `StressDur` ignores stress (preserve).
   Tables over 400 lines live in data files, or Rust generated by `build.rs`.
   Scenario `speech`.
   Done: `solouder_sources::Speech` (feature `speech`) in
   `features/speech/`: `Elements.def` and tts.cpp's rule tables are copied
   as they are into `data/` and read on first use (game thread) by a small
   C-table tokenizer; the phoneme table names its elements. A voice's
   sample buffer is sized for the longest element (16 frames) at its
   speed. F0 flutter (never on in SoLoud) and the filters klatt sets but
   never reads are not kept. Patches from the plan: speeds giving frames
   of no samples or more than 11025 are refused (0100), a voice keeps the
   speed its buffer was sized for, in reads and rewinds (0101; the
   frequency, declination and waveform stay live), and the stress
   lookahead reads 0 past the elements (0102). Found while porting: the
   phoneme lookup read the byte after the last phoneme (0103), and a byte
   that is no phoneme was read from the position after it, so stress
   marks never applied and the next phoneme was dropped (0104; only
   `[phonemes]` reach it, as the rules output phonemes only). Plan
   change: no patch for `ctype` on negative `char`s. The standard leaves
   it undefined, but both C runtimes the reference builds with (UCRT,
   glibc) give bytes from 0x80 no class in the "C" locale, so no test
   can fail before a fix; Rust's ASCII classes match them, and the
   scenario feeds UTF-8 and control bytes. `rewind` keeps klatt's filter
   history, as in SoLoud. The scenario's filter has no feedback: a
   biquad over speech decays into subnormals in the silences between
   words, which the reference flushes. Bit-exact.
7. **Vizsn** (done; patches 0105 to 0110) (540 + h 84; Ville-Matias Heikkilä, WTFPL), 8 kHz mono. It uses the
   CRT's process-wide `rand()`, so voices, scenarios in the same runner process,
   and MSVC versus glibc all change its output. Proposed patch: a per-instance
   MSVC LCG with seed 1. Defects: playing without `setText` dereferences null;
   `hasEnded` turns true while buffered samples remain, and the last block
   copies stale buffer contents; `mA` overflows (undefined); there is no
   `rewind`. Rows: `Vizsn::*` (2). Scenario `vizsn`.
   Done: `solouder_sources::Vizsn` (feature `vizsn`) in `features/vizsn/`.
   Patches from the plan: a per-voice MSVC `rand()` seeded 1 (0105), a
   voice without a text ends at once and plays silence (0106),
   `hasEnded` waits for the buffered samples and the read past the text
   is silent (0107), and `rewind` restarts the voice as a new one
   (0109). Found while porting: `getAudio` buffered whole phones until
   the read was covered, so a read of more than 1248 samples (a seek's
   discard pass, or the 4096-sample read of a loop's seek) wrote past
   its 2048-sample buffer; it now buffers one phone (at most 800
   samples) at a time (0108). And `getAudio` said it filled every
   read, also past the text, so the mixer never saw the end and a
   looping Vizsn played silence instead of looping; the read now ends
   with the text (0110). Plan changes: no patch for `mA`'s overflow:
   it needs some 5.6 minutes of one voice without a rewind, MSVC wraps
   it, and Rust wraps it too, so no test fails before a fix. The
   voice types SoLoud never selects (all but the sawtooth) are not
   kept. `~Vizsn` leaks its text; Rust has nothing to patch, so the
   reference is left as it is. Output stays between 0 and 1, centred
   on 128/255, as in SoLoud. Bit-exact.
8. **Openmpt and the source-common rows** (done): exclude the four `Openmpt::*` rows
   (reason below). Close `AudioSource::setVolume` and `setLooping`, and re-audit
   every source-common row against all the classes in its `c_functions`.
   Generators become `Queueable` if that is decided. Add Bevy loaders for
   Monotone, sfxr and the chip dumps, plus code-built generator assets.
   Done: the four `Openmpt` rows are `excluded`. Every source type gets
   the `AudioSource` settings from one macro, so `setVolume`,
   `setLooping` and the other playback rows hold for Sound, Stream,
   Synth, Bus and Queue alike; scenario `source_settings` plays each
   generator class with every setting, bit-exact, and the rows name
   the generator scenarios that use them. The 3D source-common rows
   are left to milestone E, which another agent is porting at the same
   time. Generators were already `Queueable` (`Synth`), so nothing
   changed there. `bevy_solouder` gains generated clips
   (`AudioClip::generated`, `from_generator`; `duration_seconds` is now
   an `Option`, `None` for them) and, behind the default `sources`
   feature, `SfxrLoader`, `MonotoneLoader` (with `MonotoneSettings`) and
   `ChipDumpLoader`, which plays a dump through `TedSid` unless its chip
   type is an AY one. `realtime_no_alloc` in `solouder-sources` now
   requires every source feature, so single-feature builds pass.
9. **Visualization** (done; patches 0067 and 0068) (`soloud.cpp` lines 760–805 and 2314–2360, setters lines
   235–245). Data is taken from the clipped scratch buffer once per mix call
   (#244); mixes under 256 frames take the `i % aSamples` path. Rows:
   `Soloud::calcFFT`, `getWave`, `getApproximateVolume`,
   `setVisualizationEnable`, plus the `ENABLE_VISUALIZATION` init flag. Defect:
   `getApproximateVolume` checks `>` instead of `>=` (as patch 0047 fixed for
   buses), so channel 8 reads past the array (patch). Keep the last data when
   the flag is off. This needs the real-time `Controller`: the `Renderer`
   publishes 256 samples and 8 volumes per render through a wait-free triple
   buffer; the controller computes the FFT on the calling thread, as `calcFFT`
   does. Buses get the same treatment, which lifts their offline-only limit.
   Bevy gets a snapshot component on the master and bus entities. Scenario
   `visualization`; tests in `realtime_no_alloc`.
   Done: `Mixer::wave`, `fft`, `approximate_volume` and `visualization_data`
   record each mix call's first 256 frames from the clipped mix (#244),
   repeating shorter mixes, and keep the last data when recording is off;
   `MixerConfig::visualization` is `ENABLE_VISUALIZATION`, which makes
   `Soloud::init` `ported` (`NO_FPU_REGISTER_CHANGE` excluded). The
   `Renderer` publishes the mix's data, and up to
   `RealtimeConfig::visualized_buses` watched buses', through a triple buffer
   of atomics; the `Controller` reads it and computes the FFT on its own
   thread, which lifts the offline-only limit on bus wave and FFT data. Bevy
   gets the `AudioVisualization` resource. Patches: `getApproximateVolume`
   checks the channel with `>=` (0067, as 0047 did for buses), and a mix of
   no frames with visualization on keeps the last data instead of computing
   `i % 0` (0068).
10. **Portability and support matrix:** Windows CI job uploads the C++ runner's
    output as an artifact; `run_parity.py --cpp-dir` compares Rust on Linux and
    macOS against it (max/RMS, ≤ 1e-6), Linux also against its own GCC
    reference. macOS-latest in CI runs only the C++ regression tests for the
    reference. Device tests with SOLOUDER_REQUIRE_DEVICE=1 on a PulseAudio null
    sink in a Linux container. Nightly Miri on dsp and realtime; no-default-
    features builds and per-source-feature builds. Support-matrix table in
    port-status.md: OS × arch × host × level; CoreAudio "builds, device
    unverified".
    Done: the Windows (MSVC) runner output stays the golden.
    `run_parity.py --cpp-dir` compares the Rust renders with another
    machine's runner output, and `--profile` applies a platform's tolerances
    from `tests/scenarios/tolerances.txt` (never looser than a scenario's
    own). On Linux (Docker, GCC 14.2, glibc 2.41) Rust matches the Windows
    output bit for bit in every scenario (profile `cross` is empty); the GCC
    reference differs from the Windows one in four scenarios with identical
    logs, `filters_spectral`, `spatial_sources`, `stream_ogg` and
    `ogg_decode` (max 3.9e-7, 6.0e-8, 1.4e-6 and 1.2e-4), declared in profile
    `gcc`. On a PulseAudio null sink 5 of 6 `device_lifecycle` tests pass:
    cpal reports `snd_pcm_avail_delay: I/O error (5)` about once per stream
    and the cpal output treats it as fatal, so repeated open and close fails;
    recorded, not changed. Miri is clean on its set (`dsp::`,
    `realtime::visual`, and since unit 11 `contracts::`,
    `playback::visualization`, `realtime::tests` and
    `realtime::routing_tests`). CI (`.github/workflows/ci.yml`, never run:
    not pushed) covers Windows, Linux with the `gcc` profile, macOS, a
    non-blocking Linux device job, per-crate builds without default
    features and nightly Miri; port-status.md has the support matrix.
11. **Stress and benchmarks:** unpublished `solouder-bench` crate rendering
    through `Renderer` (16/64/256/1023 voices, every resampler, 44.1→48 kHz,
    nested buses, every filter, streams of each codec, generators; buffers
    128/256/480/512/1024): mean, p99, max vs callback budget, next to a C++
    `reference_bench` via the null driver. Seeded soak test through
    Controller/Renderer/StreamWorker on three threads: no panic, no allocation,
    one final event per play, every ID released, no retirement overflow;
    covers #274, #351, #163. Machine and buffer config in port-status.md.
    Done: `solouder-bench` (unpublished, no new dependencies) times
    `Renderer::render` per buffer for its cases, with
    `reference/tests/reference_bench.cpp` building the same cases on the
    reference (null driver, `Soloud::mix`); `scripts/run_bench.py` runs
    both in release. The slowest Rust render is 15.6% of its budget (1023
    Catmull-Rom voices at 512 frames); the mixing path is 1.3 to 2.7 times
    the C++ mean (the C++ build uses SSE for clipping and panning, Rust is
    scalar and untuned), and compressed streams render 2 to 3 times faster
    than C++, which decodes inside the mix. The soak test (`tests/soak`) runs
    the `Controller`, `Renderer` and `StreamWorker` on three threads with
    seeded random requests; it fails on a panic, any allocation in render, a
    non-finite or out-of-range sample, a play without exactly one final
    event, an ID handed out twice or left in use, or retirement overflow.
    3 s in check_all, 600 s nightly; 600 s in release passed (2,874,430
    plays). Generators were added after the merge with milestone F units
    1 to 8 (see the benchmark table in port-status.md).
12. **Review:** an independent review against the gates. Every F patch has a
    C++ test that fails without it.

File readers and the other rows left outside 3D, taken with unit 1:

- `Wav::loadFile`: `decode_reader(impl Read, DecodeLimits)` reading to the end
  capped with `take()` at the decode limit, then decoding (as
  openFileToMem + testAndLoadFile). Scenario `file_loading` passes a DiskFile
  and a MemoryFile from the C++ runner.
- `WavStream::loadFileToMem`: read to the end, then `from_bytes`.
- `WavStream::loadFile`: the reference keeps the caller's borrowed File and
  every instance seeks the same cursor (two voices corrupt each other; 32-bit
  length/seek). Public `StreamFile` trait (`open(&self) ->
  io::Result<Box<dyn Read + Seek + Send>>`) implemented for paths and
  `Arc<[u8]>`, plus a constructor taking it; each voice gets its own cursor.
  Single-voice scenarios match; the shared cursor is a deliberate difference
  recorded in "Still open in the reference". A user reader behaves like a
  MemoryFile at the end of data (DiskFile reads one phantom byte for
  stb_vorbis); test both kinds.
- `Wav::lifecycle`: nothing left; mark ported.
- `WavStream::lifecycle`: add a stream-replacement case to stream_wav and patch
  the defect that `WavStream::load*` never calls `stop()` (reloading frees
  mMemFile under playing instances: use after free). Then ported.
- `AudioSource::setVolume`/`setLooping`: close in unit 8; re-audit rows marked
  ported with "Wav only" notes.
- `Soloud::init`: ENABLE_VISUALIZATION (unit 9), LEFT_HANDED_3D (E7),
  NO_FPU_REGISTER_CHANGE excluded (Rust never touches FPU registers), aBackend
  no numeric enum. Ported once E7 and F9 land.

Each milestone requires a compiling enabled feature set, passing tests, VSA
review, Rust files at or below 400 lines, an updated ledger, and documented
limitations. This plan is not a claim of existing support. Web/mobile need
separate backend, codec, and scheduling validation rather than promises from
the desktop milestone.

## Validation and completion checklist

### Behavioral tests

- Use patched C++ with the **null** backend and explicit `mix` calls as the
  deterministic reference. **Nosound** runs a worker and discards audio; it is
  not the same offline harness.
- Compare identical PCM, rates, layouts, flags, and block sequences. Track
  maximum/RMS numerical tolerances rather than assuming cross-target bit parity.
- Test partial/empty blocks, clipping/ramps, resampling boundaries, loops/seek,
  fades/scheduling, bus/filter order, queues, stale IDs/reuse, stealing, and
  inaudible behavior.
- Test asset/source removal during playback, multiple plays, stop-before-start,
  despawn-before-load, reload, underflow, invalid data/channels, queue overflow,
  and shutdown races.
- Test spatial direction, hierarchy, distance models, units, Doppler, and listener
  changes against the [Bevy contract](ARCHITECTURE.md#bevy-integration-contract).
- Fuzz or malformed-input test every decoder path for channel count, sample
  rate, frame count, allocation size, truncation, and zero/unknown length
  (#401, #317). Decoder errors are typed; they never panic or allocate from
  unchecked metadata.
- Every entry in the [patch ledger](#reference-patch-ledger), the
  [open-defect table](#still-open-in-the-reference), and the
  [fixed-upstream list](#fixed-upstream-keep-as-regression-tests) that covers a
  ported feature needs a passing Rust regression test before its milestone is
  marked complete.

### Build, size, runtime, and performance checks

Run from the repository root:

```sh
python scripts/check_rust_file_lines.py
python scripts/test_check_rust_file_lines.py
python scripts/gen_parity_ledger.py --check
python scripts/test_reference.py   # needs CMake and a C++ compiler
```

The size checker includes untracked Rust source and returns a nonzero status
for violations. No Rust files means only that there are no current size
violations, not that a Rust implementation has been validated. Markdown is not
checked.

`python scripts/check_all.py` runs all of the above except the C++ tests, plus
`cargo fmt --check`, `cargo clippy -D warnings` and `cargo test`. Add
`--reference` for the C++ regression tests and `--parity` to compare every
scenario with the C++ reference; CI (`.github/workflows/ci.yml`, Windows and
Linux) runs it with both.

Run the size check after formatting. As crates gain features, also check
core-only/no-device builds and meaningful codec/platform combinations in CI.
Headless Bevy lifecycle tests use offline output; hardware smoke tests are
separate so CI does not require a device.

Use Miri on applicable pure-Rust paths, callback allocation instrumentation,
and concurrency/stress tests for handoffs/retirement. Benchmark realistic voices,
buses, filters, and streaming at 44.1/48 kHz in release mode. Measure worst-case
callback duration and underruns, not only average throughput. Record machine
and buffer configuration.

### Port status and licensing

Maintain `docs/port-status.md` with source/slice mappings, enabled features,
known differences, and benchmarks; per-function
status lives in the ledger. Confidence labels/trailer comments do not replace
verification.

SoLouder keeps SoLoud's **zlib/libpng license** ([LICENSE](../LICENSE)). Mark
ported files as altered (see [CONTRIBUTING.md](../CONTRIBUTING.md)), credit
the original authors ([CONTRIBUTORS.md](../CONTRIBUTORS.md)), and
record any ported third-party code in [THIRD_PARTY.md](../THIRD_PARTY.md),
checked against `docsrc/legal.mmd` and the individual notices. Audit new
libraries and fixtures independently.

Completion means a tested native mixer and Bevy integration, not translated
files or bindings. Verify each slice's ownership, real-time behavior, audible
output, ECS lifecycle, and Rust file size before marking it complete.
