# SoLouder porting guide: SoLoud C++ → Rust for Bevy

## Goal and current status

SoLouder ports the SoLoud audio engine to **native Rust**, with an idiomatic
**Bevy plugin** for assets, playback, buses, filters, and spatial audio. It
keeps SoLoud's mixing behavior without requiring the C++ library in the normal
Rust build, and adds the fixes and extensions developed for GoreGrind.

**Current status:** this repository contains the patched C++ reference tooling
(pinned upstream commit, patch series, fetch script, C++ regression tests) and
the parity ledger. There is no Cargo workspace or Bevy plugin yet; proposed
paths and APIs are not available or tested until they exist.

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
SoLoud branch (the GoreGrind fork's `goregrind` branch is the current source),
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

Numerical tolerance: compare against a scalar (non-SIMD) reference build. Aim
for bit-exact output on scalar paths; otherwise the maximum absolute
difference must stay within 1e-6, and the scenario records the measured
maximum and RMS difference. Larger tolerances (for example lossy decoders) are
documented per scenario.

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
and regression tests in `docs/port-status.md` once the workspace exists.

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
| 0001 | — (GoreGrind) | CoreAudio `AudioQueueRef` moved from a file-level static into per-engine `mBackendData`; deiniting one engine no longer stops every engine (macOS co-op mute). | — (macOS only) |
| 0002 | #405, PR #379 | `WavStreamInstance::seek` tested the codec union member `mCodec.mOgg`, which is non-null for every live WAV/FLAC/MP3 decoder, so `stb_vorbis_seek` ran on a `drwav*`/`drflac*`/`drmp3*` and corrupted the heap at every loop wrap of a looping non-Ogg stream (GoreGrind crash). Gated on `mFiletype == WAVSTREAM_OGG`; other formats use generic seek. | covered by `wavstream_loop_channel_stride` |
| 0003 | — | Build only: CMake ≥ 3.5 (CMake 4 requires it), static MSVC runtime, `speech/Elements.def` header-only. | — |
| 0004 | PR #379 | `WavStreamInstance::getAudio` wrote FLAC/MP3/WAV channels at stride `aSamplesToRead` instead of `aBufferSize`; the loop-wrap refill reads fewer frames than the stride, so channels ≥ 1 landed at the wrong offset. | `wavstream_loop_channel_stride` |
| 0005 | PR #379 | `WavStreamInstance::hasEnded` ignored `LOOPING`. | `wavstream_looping_not_ended` |
| 0006 | #279, #205, #327 | `calcActiveVoices_internal` returned before `mapResampleBuffers_internal` when inaudible-tick voices filled the active cap; voices were mixed with null resample buffers (segfault). | `inaudible_tick_over_active_cap` |
| 0007 | #298 | Protected voices join the must-live group ahead of the volume sort; previously a quiet protected voice could be made inactive, contradicting "Protected voices are always played". | `protected_voice_stays_active` |
| 0008 | (found auditing #343) | A voice whose fixed-point step is zero (speed near zero, or a step beyond the fixed-point range) skipped resampling but was still panned, mixing the previous voice's scratch data into the output. Unwritten scratch is now zeroed. | `zero_step_voice_no_stale_scratch` |
| 0009 | #392 | `AudioSourceInstance::seek` looped forever with less than one frame of scratch; it now rejects undersized scratch and stops on a non-positive count. | `generic_seek_small_scratch` |
| 0010 | #345, #404, #324 | Stopping a voice before a successful init (or after init failed) walked a null `mResampleDataOwner`. The readers are guarded instead of resetting the cap (PR #345 resets it to 16 on every init, discarding a pre-init `setMaxActiveVoiceCount`). `postinit_internal` no longer leaks the previous arrays. | `play_stop_without_init`, `play_after_failed_init` |
| 0011 | — (GoreGrind) | `mapResampleBuffers_internal` used `char live[256]` guarded by `SOLOUD_ASSERT(mMaxActiveVoices < 256)`, while `setMaxActiveVoiceCount` accepts up to 1023. On MSVC the assert is a `__debugbreak` even in release builds (why GoreGrind capped at 255); without asserts it overran the stack. Sized by `VOICE_COUNT`. | `active_voice_cap_above_255` |
| 0012 | #401, #317 | Decoder metadata validation: `Wav` WAV/FLAC/MP3 overflowed `float tmp[512 * MAX_CHANNELS]` with more than 8 channels and truncated 64-bit frame counts when allocating; `Wav::loadogg` accepted length 0 and could copy past the reported length; `WavStream` FLAC/MP3/WAV clamped `mChannels` but still decoded every channel into the fixed block. Invalid layouts are rejected, allocation is `nothrow`, short reads are zero-filled. | `wav_rejects_too_many_channels`, `wavstream_rejects_too_many_channels` |
| 0013 | #361 | `AudioSourceInstance3dData::init` kept the previous voice's 3D flags; `play3d` in a slot last used by a listener-relative voice started at volume 1.0 instead of 1/99 at 99 units. | `play3d_reused_slot_flags` |
| 0014 | PR #375 | `Wav` and `WavStream` are non-copyable (implicit copies double-freed owned buffers). | compile-time |

GoreGrind's shipped DLL is built from the same branch with
`build_windows.bat` in the fork; the C API and WASAPI/WinMM backends compile
with all patches.

### Still open in the reference

Not fixed by a patch yet. Each needs a Rust requirement and test; reference
fixes are welcome as new patches.

| Upstream | C++ location | Defect | Rust requirement / test |
|---|---|---|---|
| #314 (reported) | `Soloud::mixBus_internal` block read | Fixed 512-frame source reads with a non-integer rate ratio reportedly accumulate drift (about 0.1 s over 3 minutes for 44.1 → 48 kHz). | **High priority.** Exact fractional playhead; long-duration drift test against an analytic position. Reproduce in the reference before changing it. |
| #396 (reported), #156 | `WavStreamInstance::seek` fallback | Non-Ogg streams seek by rewind + decode-and-discard, which is O(seek distance). Upstream reports seeking a stream handle silently kills it. | Per-codec seek with defined accuracy; test seek while playing and while paused. PR #336 is a native-seek reference. |
| #343 (reported) | `Soloud::mixBus_internal` resampling loop | Reported infinite loop when `writesamples` stays 0. Not reproducible from the current code: each iteration either writes frames or loads the next block. The adjacent zero-step defect is fixed by patch 0008. | Every resampling iteration makes progress or exits; test extreme speed and Doppler values. |
| #307, #241 | `Soloud::deinit`, `Soloud::~Soloud` | `SOLOUD_ASSERT(!mInsideAudioThreadMutex)` in `deinit` can fire at shutdown; ordering of `stopAll` versus backend teardown. | `Drop` is safe in every state; test repeated init/teardown. See the GoreGrind shutdown lesson below. |

Also reported upstream and worth a test once the feature is ported: #274
(intelligibility with many voices), #351 (many plays into a bus starting late),
and #163 (valid, non-zero-volume voices that are silent).

### Behavior decisions to record

These are reference behaviors that are surprising rather than memory-unsafe.
For each, `docs/port-status.md` must say whether Rust preserves it, changes it,
or has not decided, with a test either way.

| Upstream | Reference behavior |
|---|---|
| #381 | `Soloud::play` defaults `aVolume` to `-1` (use the source volume); `Soloud::play3d` and every `Bus::play*` default to `1.0`. |
| #395 | Nested buses reduce gain compared to direct playback with the same settings. |
| #280 | Stopping a bus handle tears down every voice playing through that bus. |
| #240 | Hard-coded inaudibility thresholds: `0.001` in `update3dAudio` and `0.01` in `play3d`/`play3dClocked` (`soloud_core_3d.cpp`). |
| #175, #203 (reported) | Pops or wrong orientation when 3D voices become audible or between `update3dAudio` calls. Volume/pan ramps are per mix call, so change rate depends on device buffer size. |
| #94, #105 | `LEFT_HANDED_3D` only flips part of the listener basis; reporters found front/back and Z errors. Derive the Bevy mapping from tests, not the flag. |
| #98 | Relative play speed and Doppler must compose multiplicatively; verify the reference does. |
| #76 | A handle can become invalid while audio is still audible on some backends; define validity as "until mixed out". |
| #117 | `getFilterParameter` returns error codes in the value channel; Rust uses `Result`/`Option`. |
| #318 | WAV `smpl` loop points are ignored. |
| #276 | Intro-then-loop scheduling via `setDelaySamples` leaves gaps; Rust scheduling should be frame-accurate. |
| #291, #286 | A second `stop` on a queue is ignored; no API empties a queue. |
| #252 | A finished handle cannot be restarted. |
| #177 | Single listener only (matches [ARCHITECTURE.md](ARCHITECTURE.md#spatial-audio)). |

### Fixed upstream: keep as regression tests

These fixes were already upstream; Rust must not reintroduce them.

- **#183, #251, #253:** `clip` and `panAndExpand` SIMD paths dropped or misread
  the tail when the frame count was not a multiple of 4 (e.g. 735 frames at
  44.1 kHz and 60 fps). Test prime and `n % 4 ∈ {1, 2, 3}` lengths.
- **#224, #319:** stream position advances by `buffer time × relative speed`
  and generic seek stores the absolute requested time.
- **#97:** large Doppler velocities made `step_fixed` zero (division by zero).
- **#228:** `trimVoiceGroup` deadlocked by relocking the audio mutex.
- **#135, #229:** `playClocked` offset handling for the first event and `time == 0`.
- **#160:** reloading a source resets its channel count.
- **#201, #210:** lengths are frames ÷ rate, not interleaved samples ÷ rate.
- **#322:** voice-group allocation size.
- **#95:** `play3d` starts paused, computes the voice's 3D state immediately,
  then unpauses, so the first frame is attenuated.
- **#136, #238:** `play`, `play3d`, and volume changes while paused set the
  current channel volume to its target instead of ramping from silence.

### Lessons from the GoreGrind Unity integration

The game uses the patched DLL through C# bindings
(`F:\GoreGrind\Assets\Scripts\Audio\SoLoudRuntime`; crash analysis in
`F:\GoreGrind\Assets\Plans\fix-soloud-quit-crash.md` and
`soloud-playmode-exit-crash-fix.md`). Findings that constrain the Rust design:

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
  an engine is destroyed, and `~AudioSource` stops voices only on the last
  engine that played it. The game must "abandon" sources rather than destroy
  them at exit. Rust: no source-to-engine pointers; voices hold `Arc` PCM.
- **One engine per listener** (two for co-op) exposed process-global backend
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
| A — Foundation and reference harness | Patched reference and C++ regression tests (done); parity ledger (done); Cargo workspace, pinned Rust/Bevy versions, CI with file-size and ledger checks; shared scenario format with a C++ null-driver runner and a Rust runner |
| B — Offline playback | Prepared PCM/decoded WAV, planar stereo mixer, IDs, play/stop/pause/loop, volume/pan, linear resampling, clipping/parity |
| C — Windows and Bevy playable slice | CPAL negotiation, bounded control/status, asset loading, playback/despawn example, no-device/allocation tests |
| D — Routing and DSP parity | Nested buses, queues/groups, virtualization/protection, scheduling, remaining resamplers, voice/bus/global filters |
| E — Spatial and streaming parity | Listener/emitter systems, attenuation/Doppler/delay, workers, seek/loop regressions, WAV/FLAC/Ogg Vorbis/MP3 |
| F — Sources and portability | Generators/speech/music, FFT/visualization, Linux/macOS checks, stress/benchmarks, support matrix |

Plan work inside a milestone as small parity units: one behavior, its
scenarios, the Rust implementation, and a passing comparison. Order units
along the mix pipeline (source read, resampling, pan/expand with ramps, bus
mix, filters, clipping, output conversion) so each unit builds on verified
stages.

Regression tests for [tracked defects](#reference-patches-and-upstream-tracking)
belong to the milestone that introduces the affected behavior:

- **B:** declared channel stride with partial reads (patch 0004), frame counts
  not divisible by 4 (#183, #251), zero-step and resampler progress (patch
  0008, #343, #97), mix-buffer assignment when inaudible-tick voices fill the
  active cap (patch 0006), active caps above 255 (patch 0011).
- **C:** init failure, repeated init/teardown, and callback-before-state
  shutdown order (patch 0010, #307, GoreGrind teardown lessons).
- **D:** protected-voice ranking (patch 0007), bus default volume and nested
  gain (#381, #395, #280), queue and voice-group fixes (#291, #228, #322),
  filter stability at the voice processing rate.
- **E:** non-Ogg seek/loop (patch 0002), looping streams not ending (patch
  0005), stream seek (patch 0009, #396), decoder metadata validation and
  fuzzing (patch 0012), long-duration drift (#314), 3D initialization (patch
  0013, #95).

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

Once the workspace exists, also run from the repository root:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Run the size check after formatting, check core-only/no-device builds and
meaningful codec/platform combinations, and include both in future Rust CI.
Headless Bevy lifecycle tests use offline output; hardware smoke tests are
separate so CI does not require a device.

Use Miri on applicable pure-Rust paths, callback allocation instrumentation,
and concurrency/stress tests for handoffs/retirement. Benchmark realistic voices,
buses, filters, and streaming at 44.1/48 kHz in release mode. Measure worst-case
callback duration and underruns, not only average throughput. Record machine
and buffer configuration.

### Port status and licensing

Maintain `docs/port-status.md` with source/slice mappings, enabled features,
known differences, and benchmarks once the workspace exists; per-function
status lives in the ledger. Confidence labels/trailer comments do not replace
verification.

SoLouder keeps SoLoud's **zlib/libpng license** ([LICENSE](../LICENSE)). Mark
ported files as altered (see [CONTRIBUTING.md](../CONTRIBUTING.md)), credit
the original authors ([UPSTREAM_AUTHORS.md](../UPSTREAM_AUTHORS.md)), and
record any ported third-party code in [THIRD_PARTY.md](../THIRD_PARTY.md),
checked against `docsrc/legal.mmd` and the individual notices. Audit new
libraries and fixtures independently.

Completion means a tested native mixer and Bevy integration, not translated
files or bindings. Verify each slice's ownership, real-time behavior, audible
output, ECS lifecycle, and Rust file size before marking it complete.
