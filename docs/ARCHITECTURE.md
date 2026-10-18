# SoLouder architecture

## Purpose and status

This document owns the **target design** for SoLouder, the native Rust port of
SoLoud for Bevy:
workspace boundaries, Vertical Slice Architecture (VSA), ownership, real-time
communication, decoding, and Bevy integration.

[PORTING.md](PORTING.md) owns C++ reference mappings, translation guidance,
behavioral compatibility, patch preservation, migration milestones, and parity
validation. Keep design decisions here rather than maintaining a second copy
in the porting guide.

**Status:** milestone E done (see [port-status.md](port-status.md)). All five crates exist: `solouder-core`
(the offline mixer with routing, filters, streaming and 3D audio, and the
real-time controller/renderer split, which reaches all of them except voice
groups and some queries), `solouder-decoders` (WAV, FLAC, Ogg Vorbis and
MP3, decoded or streamed), `solouder-cpal` (device output), `bevy_solouder`
(assets, playback entities, output lifecycle, and spatial listener and
emitter components, and the mix's visualization data as a resource) and
`solouder-parity`. Buses, filters and the other ECS mappings in the Bevy
sections below are still proposed.

The target is a native Rust mixer, not a binding that requires the C++ SoLoud
library at runtime. Core audio must remain usable without Bevy or a device.
The first hardware target is Windows through CPAL; additional targets require
separate validation rather than inheriting support claims from C++ backends.

## Architectural principles

- **Organize implementation by vertical feature slices.** Playback, streaming,
  spatial audio, routing, and filter automation each own their use cases and
  feature-specific types, behavior, integration, and tests.
- **Retain necessary crate boundaries.** Bevy, device output, decoding, and core
  DSP have different dependencies and execution environments. VSA does not
  require importing Bevy or CPAL into the mixer.
- **Give mutable audio state one owner.** The audio runtime owns the mixer;
  application code submits controls and observes bounded status snapshots.
- **Keep the real-time path bounded.** Loading, allocation, destruction,
  blocking I/O, and general ECS work happen outside the callback.
- **Keep compatibility explicit.** Preserve the behaviors documented in
  [PORTING.md](PORTING.md); record deliberate departures with evidence and tests.
- **Apply a hard 400-physical-line limit to Rust files.** Split along cohesive
  responsibilities before a file exceeds the limit. Markdown has no line cap.

## Vertical Slice Architecture

A vertical slice is a cohesive capability, not a synonym for a crate or a
single file. Keep the slice's request/settings types, validation, operations,
state transitions, adapter integration, and tests near that feature.

Initial slices are:

| Slice | Responsibilities |
|---|---|
| Playback | Play requests, voice lifecycle, pause/stop, loop controls, entity ownership and completion |
| Decoded audio | Asset import, format/metadata validation, immutable PCM preparation |
| Streaming | Per-play decoder workers, buffering, underflow, seek/rewind epochs and cancellation |
| Routing | Bus topology, cycle validation, nested mixing, queues and voice groups |
| Filters | Instance DSP state, filter chains, parameters and audio-time automation |
| Spatial audio | Listener/emitter snapshots, coordinate mapping, attenuation, Doppler and distance delay |
| Visualization | Bounded waveform/FFT snapshots and gameplay-facing diagnostics |
| Output lifecycle | Device negotiation, callback conversion, disconnects and shutdown |

A slice may span crates through narrow contracts. For example, streaming owns
its worker and buffering policy in the decoder crate, callback consumption in
core, and entity/asset integration in the Bevy crate. Name related feature
modules consistently and document the shared contract; do not force these
pieces into one crate merely to make them physically adjacent.

Within a crate, prefer `features/playback/`, `features/streaming/`, and similar
feature folders over application-wide `models/`, `services/`, `repositories/`,
`components/`, or `systems/` layers. A Bevy playback slice can contain its
components and systems together. A filter slice can group one filter's DSP,
configuration, and tests together rather than creating a global file for each
kind of type.

Shared code is justified by a stable contract or actual reuse. Buffer layouts,
sample units, audio-time primitives, typed IDs, and small DSP kernels are
reasonable common foundations. Do not move feature logic into a generic
`utils`, `common`, or `manager` module just to avoid the file-size limit.
Dependencies between slices must be explicit and acyclic; coordinate them
through public operations/contracts rather than reaching into private state.

## Rust file-size rule

Every first-party **`.rs` file must contain at most 400 physical lines**.

- Count all lines, including comments, blank lines, imports, inline tests, and
  macro definitions. A final non-newline-terminated line still counts.
- Apply the same rule to production code, standalone tests, examples,
  benchmarks, build scripts (`build.rs`), and proc-macro implementations.
- Checked-in first-party generated Rust also counts. Avoid treating generation
  as an exemption for oversized authored source.
- Markdown, manifests, C/C++ reference sources, Python validation scripts, and
  other non-Rust files are outside this limit. Documentation may be as long as
  needed, provided responsibilities remain separated.
- Build output under directories named `target` and external vendored trees
  under `vendor/` are excluded, as is the git-ignored C++ reference checkout
  `reference/soloud/`. Do not put first-party source
  there to bypass the rule. Rust generated into Cargo's build-output directory
  is not checked-in first-party source.
- Do not condense multiple statements onto one line or delete useful comments
  to evade the limit. Split by behavior: request validation, state transitions,
  a DSP kernel, worker coordination, or focused tests.
- Keep module entry points small. Large test suites can live in a feature-local
  `tests/` module with separate files; each Rust test file still obeys the cap.

The checker scans the repository, including untracked `.rs` files, and reports
violations with file paths and physical line counts:

```sh
python scripts/check_rust_file_lines.py
```

Run it before completing each porting slice. Add it to CI alongside Cargo checks
when a Rust CI pipeline is introduced. It currently enforces size only; review
must still judge whether module boundaries are cohesive and follow VSA.

## Proposed workspace and dependency direction

The workspace lives at the repository root, beside the reference tooling. The
C++ reference stays under `reference/` and is never part of the Cargo build:

```text
Cargo.toml                      # workspace
crates/
  solouder-core/src/
    lib.rs
    contracts/                  # small shared buffer/time/ID contracts
    dsp/                        # reusable kernels, not feature orchestration
    features/
      playback/
      routing/
      filters/
      spatial/
      visualization/
  solouder-decoders/src/
    lib.rs
    features/
      decoded_audio/
      streaming/
  solouder-sources/src/
    lib.rs
    prg.rs                      # WELL512, shared by noise and sfxr
    features/                   # one slice and Cargo feature per generator
      noise/
  solouder-cpal/src/
    lib.rs
    features/
      output_lifecycle/
  bevy_solouder/src/
    lib.rs                      # SolouderPlugin and system sets
    features/
      output_lifecycle/
      playback/
      decoded_audio/
      streaming/
      routing/
      filters/
      spatial/
      visualization/
  solouder-parity/              # scenario parser, Rust runner, comparison (not published)
  solouder-bench/               # render-time benchmarks and the soak test (not published)
tests/
  scenarios/                    # .scn parity scenarios shared with the C++ runner
  fixtures/                     # small, redistributable audio fixtures
docs/
  parity-ledger.csv             # C API parity target and per-behavior status
  port-status.md                # known differences, enabled features, benchmarks
reference/
  upstream.json, patches/       # pinned upstream SoLoud and reviewed patches
  tests/                        # C++ regression tests and parity_runner.cpp
  soloud/                       # git-ignored checkout (scripts/fetch_reference.py)
```

Existing today: `solouder-core` (`contracts/`, `dsp/`, `features/playback/`
and `features/realtime/`), `solouder-decoders` (`features/decoded_audio/`,
WAV), `solouder-sources` (`prg.rs`, `features/noise/`), `solouder-cpal`
(`features/output_lifecycle/`), `bevy_solouder`
(`features/decoded_audio/`, `features/output_lifecycle/`,
`features/playback/`), `solouder-parity`, `solouder-bench`, `tests/scenarios/`,
`tests/fixtures/wav/` and `docs/port-status.md`. Versions are pinned in `rust-toolchain.toml` (Rust
1.98.1) and the root `Cargo.toml` (Bevy 0.20, CPAL 0.18).

This illustrates feature ownership, not a requirement to create empty modules
up front. Introduce a slice when it has an implemented use case. File names
inside a slice should describe their responsibility; no single feature or
crate-root file is allowed to grow beyond 400 lines.

Dependency direction is `bevy_solouder` → output/decoder/source/core crates,
with output, decoder and source crates depending on `solouder-core`, never
the reverse. Generators (`solouder-sources`) implement core's `Generator`
and `GeneratorInstance` traits; core plays them through `Synth` without
knowing any of them. The
names were free on crates.io on 2026-10-12 (`soloud` and `soloud-sys` belong to
unrelated bindings); they are not reserved until published.
The crates are dependency/execution boundaries, not horizontal business layers.

- Start core with ordinary Rust `std`; `no_std` is not an initial goal.
- CPAL is the initial device-output adapter, not a core DSP dependency. Isolate
  negotiation and target-specific stream requirements in that adapter.
- Audio files are decoded by first-party code: a RIFF reader with dr_wav's
  conversions, and ports of dr_flac, stb_vorbis and dr_mp3 that match the
  reference bit for bit. Symphonia decoded FLAC for a while and was dropped
  because damaged files made it panic; other lossy decoders would not match
  the reference's samples. A port must reproduce the reference's arithmetic
  exactly (dr_mp3's SSE operation order, the C library's double-precision
  math), never panic, and fail safely where the C is undefined.
- Use Bevy's asset/task facilities for Bevy loading. A separate async runtime is
  not required simply to decode an asset.
- Make codecs, output, and extended sources opt-in where useful. Document and
  test meaningful feature combinations; unrestricted `--all-features` need not
  be valid when features represent incompatible backend choices.
- The first playable slice is decoded WAV/PCM, offline stereo mixing, Windows
  output, and a Bevy example. Additional codecs, streaming, buses, filters, and
  spatial audio remain explicit follow-on coverage.

## Ownership and the real-time boundary

The C++ engine serializes much of its mixer/control state with an audio mutex.
The Rust target instead gives the output runtime exclusive ownership of mutable
mixer state. Bevy owns a controller and immutable assets, not a mutable mixer
behind a blocking mutex.

```text
Bevy systems / application controls
              |
     bounded command handoff
              v
output runtime owns mixer -----> device callback output
              |
     bounded status/completion handoff
              v
Bevy notifications / cached playback state

asset loading / decoding workers ---> prepared audio or bounded PCM buffers
```

For the CPAL path, the callback drives mixing with preallocated state.
Offline tests own the same mixer directly without queues or a device.

The topology, implemented in `solouder_core::realtime`: `split(mixer)` returns
a `Controller` (game side) and a `Renderer` (audio side), joined by three
single-producer single-consumer `rtrb` rings and a few atomics.

| Ring | Producer → consumer | Full behavior |
|---|---|---|
| Commands | controller → renderer | The call returns `QueueFull`; nothing is lost silently. The renderer applies at most `commands_per_render` per render and leaves the rest for the next. |
| Events (`Started`, `Failed`, `Finished`) | renderer → controller | Cannot fill: it holds two events per reservable voice ID, and an ID is only released after its final event is read. |
| Retired items | renderer → controller | Ended and stopped voices (with their samples and filter instances), replaced filter instances, and queued sounds that could not be queued are handed back to be dropped on the game thread. Every command that can retire something costs a credit, and credits return only as retired items are collected, so the mixer's preallocated list cannot overflow in normal use; overflow is still counted in `Status`. |

Voice IDs are generation-checked and reserved by the controller when a play
is queued, so commands can target a voice before it starts. The controller
builds each voice on the game thread, in a box that later carries it back:
the voice's filter instances are created, and memory for their delay lines
and FFT buffers reserved for the voice's channels and rate, before the play is
sent; global and bus filter instances are prepared for the output layout or
the bus voice the same way. Filters still size their buffers at first use,
inside the reserved memory, so delays changed before then render as they do
offline. A queue's sound list is allocated by the controller the first time
the queue is played. `Status` (stream time, voice counts, renders, deferred
commands, rejected queue sounds) is published through atomics after every
render. Renders are checked allocation-free by the `realtime_no_alloc` test,
including with every filter that allocates lazily, buses, queues, and delays
and channel settings changed before a filter first runs. A first use that
needs more than was reserved still allocates (see port-status.md,
"Real-time coverage").

### Callback constraints

- No filesystem access, blocking decoder reads, network work, waits, Bevy ECS
  access, logging/formatting, or blocking locks in the callback.
- Preallocate voice storage, scratch buffers, command/status rings, filter
  memory, and streaming buffers. Bound command consumption per callback so
  bursts of game-thread work cannot starve rendering.
- Prepare playback/filter objects off the callback. Avoid hidden allocations
  from `Vec` growth, trait-object construction, or decoder setup.
- Destruction is also non-real-time work: dropping the last `Arc`, large PCM
  buffers, decoders, or device objects can free memory or block. Retire them
  through a bounded handoff to a non-audio owner and define full-queue behavior.
- Define command/status overflow behavior. Do not block for capacity or
  silently lose stop/error/completion notifications. Use rejection, reserved
  capacity, coalescing, or durable status snapshots as appropriate.
- Prevent routine panics with validated inputs and buffer contracts. Failure or
  underflow produces defined silence and publishes status outside the callback.
  Never unwind across a foreign callback ABI.
- Match negotiated sample type/channel order and variable callback lengths.
  Missing or disconnected devices must not crash the Bevy application.
- Stop callbacks before releasing mixer/asset state, then stop workers and
  reclaim retired objects on an appropriate non-audio thread. Device resources
  are per-engine rather than process-global singleton state.

### Control semantics and shared data

A queued play request is not automatically a live voice. Define a request ID or
reserved generation-checked ID, acknowledgement of creation/failure, and ordered
pause/stop behavior for requests that have not started. Reserve notification
capacity before accepting operations whose completion must be observable.

Getters expose documented snapshots, not synchronous queries that block the
audio thread. Distinguish requested state from applied state. Playback-finished
means the mixer finished rendering that voice, not that every buffered sample
has necessarily reached the speaker.

Use `Arc` for immutable PCM shared by assets and voices, not as evidence that
mutable decoder/filter state is thread-safe. Keep mutable instances under one
owner. Respect Bevy's `Send`/`Sync` requirements; use non-send facilities when a
platform stream type requires them, rather than adding unsafe thread guarantees.

Use generation-checked voice/bus/group IDs instead of references to movable
storage. Reject bus cycles, preserve per-play cursor/filter independence, and
publish visualization data as bounded snapshots rather than mutable buffer
pointers: after each render that records it, the renderer copies the mix's
data and that of a bounded set of buses into a triple buffer of atomics,
and the controller reads the latest complete copy (computing the FFT on its
own thread), so neither side waits or allocates. Audio clocks advance from rendered frames, not Bevy frame delta.

## Decoding and streaming design

- Assets hold immutable PCM or encoded data/stream metadata. Mutable decoder
  state and its cursor belong to a playback instance or worker, never an asset
  shared by multiple entities.
- Fully decode resident audio outside the callback. Validate sample rates,
  frames, channel layout, checked allocation sizes, truncation, and memory limits
  before allocating from external metadata.
- Stream through per-instance workers and bounded PCM buffers. The callback
  only consumes prepared frames. Define prebuffering, underflow silence,
  end-of-stream, pause, and cancellation semantics.
- Seek/rewind/loop flush old PCM, interpolation history, and partial decoder
  state. Tag buffered data with an epoch or equivalent so stale worker output
  cannot play after a seek; define accuracy and asynchronous acknowledgement.
- Own codec state in explicit Rust enum variants. Format dispatch must not
  reinterpret one codec's memory as another's.

Reference format support and seek regressions remain documented in
[PORTING.md](PORTING.md#decoding-streaming-and-patch-preservation). DSP parity
uses identical prepared PCM so decoder differences do not obscure mixer errors.

## Bevy integration contract

The `bevy_solouder` crate provides `SolouderPlugin`, the immutable
`AudioClip` asset with a `.wav` loader, the `SolouderController` resource,
and the playback components `SoundPlayer`, `PlaybackSettings` and
`SoundStatus` with `SoundFinished`/`SoundFailed` messages, and the
spatial components `AudioListener`, `SpatialEmitter`, `AudioVelocity` and
`AudioTeleport` with the `SpatialAudioSettings` and `ListenerState`
resources. Buses and filters are still design names. Keep each feature's
components, systems, validation, and tests within its slice; the playback
slice reads the spatial slice's snapshot, never the other way round.

### ECS model (proposed)

The port has two layers with different jobs:

- `solouder-core` is plain Rust with an imperative, C-API-shaped interface
  (create, play, set volume, mix). It knows nothing about Bevy. C API parity is
  proven at this layer, and non-Bevy users can embed it directly.
- `bevy_solouder` is ECS-first: entities, components, assets, relationships and
  events are the primary API. An imperative system parameter is an escape hatch
  for fire-and-forget sounds, not the main path.

| SoLoud concept | ECS form |
|---|---|
| Audio source (`Wav`, `WavStream`, generators) | Asset handle. Generator parameters are an asset or a component. |
| Voice handle | An entity with a sound-player component and playback-setting components. The core voice ID stays internal. |
| `play` / `stop` | Spawn / despawn (or remove the player component). A removal hook stops the voice. |
| Volume, pan, speed, pause, looping, protect, inaudible behavior | Component fields. Change detection turns edits into bounded commands. |
| Fades and oscillators | Explicit fade components or commands carried out by the mixer at audio rate, never tweened once per frame. |
| Bus | An entity with a bus component. Routing is a relationship from voice or bus to its target bus; cycles are rejected. |
| Voice and bus filters (8 slots) | Filter components on the voice or bus entity, in a defined slot order. |
| Global settings and filters | A settings resource and a master-bus entity. |
| Voice groups | Usually unnecessary: queries and relationships select sets of sounds. Group operations remain available as bulk commands. |
| 3D listener and source | `AudioListener` and `SpatialEmitter` read from `GlobalTransform`; velocity explicit (`AudioVelocity`) or derived, with `AudioTeleport` for jumps. |
| Playback status, stream time, finished | A status component updated from the status handoff, a finished event, and an opt-in despawn-when-finished policy. |
| Visualization and FFT | The `AudioVisualization` resource for the master mix (wave, spectrum and channel volumes, refreshed each frame while enabled); a snapshot component on bus entities once buses are entities. |
| `playClocked`, scheduling | Explicit commands in audio-clock time. |

Rules for the mapping:

- Components hold requested state; the mixer owns applied state, which flows
  back through status. Systems never wait for the audio thread.
- One sync system set runs after transform propagation, collects every change
  for the frame, and submits one bounded command batch.
- Detached one-shot sounds (no entity) are an explicit, documented choice.
- Several simultaneous listeners (split-screen) are a deliberate feature with a
  defined mixing policy, not an accident of spawning two listener components.
  Until then the policy is one listener: of several, the lowest entity index
  is heard and `ListenerState::Several` says so; with none, the last pose
  is kept.
- Bevy is pinned at 0.20. `SoundPlayer` is an immutable component, so its
  `on_insert` and `on_discard` hooks (discard runs on replace, removal and
  despawn) see every change. The hooks keep the plugin's private record of
  each player, which owns the voice; `SoundStatus` is only a copy of that
  record, so removing or replacing it cannot orphan a voice. The sync only
  visits new players, players whose `PlaybackSettings` changed, and work a
  full command queue deferred. Starting settings travel inside the play
  request, so a voice never renders a frame with defaults. When the output
  fails or its renderer is gone, every waiting and playing sound fails with
  `OutputUnavailable`.
- Outcomes are `Message`s, and `insert_non_send` holds the CPAL stream,
  which is not `Send` on every platform. Dropping the app drops the stream,
  which stops the callback before the mixer is freed.

### Assets and playback

- Register a custom asset and `AssetLoader` using the selected Bevy version's
  async reader interface. Use `AssetServer` and configured asset sources rather
  than direct disk filenames or I/O in update systems.
- Retain strong asset/data ownership for pending requests and live voices.
  Removal or reload must not invalidate audio being rendered. Initially retain
  existing voice data and apply reloads to new plays; document live replacement
  separately if it is introduced.
- Model load-pending, ready, playing, paused, stopped, finished, and failed
  behavior. Asset-loading and output-device failures are observable status.
- Expose volume, pan, speed, looping, routing, and filters as feature controls;
  do not expose raw mixer pointers.
- Default entity-owned playback to cancellation/stop on entity despawn or
  component removal, including pending loads. Explicitly detached playback may
  outlive an entity, but must be a documented policy.
- Deliver bounded lifecycle notifications through the selected Bevy version's
  message/event facilities. Bevy keeps a message for two updates, so readers
  that run less often than every frame can miss one; the status component
  keeps the final state for them. The mixer's event ring is drained every
  frame, so completions are never lost between the callback and the sync.
- Define startup, device failure, and teardown. Offline/no-device mode permits
  headless Bevy lifecycle tests without hardware.

### Spatial audio

- In the core, `features/spatial` keeps the listener, speakers and per-voice
  3D state on the mixer's owner and computes each voice's gains there, as
  SoLoud's `update3dAudio` does outside its mixer. Through the `Controller`
  the same math runs on the game thread, on the controller's own copy of
  the listener, speakers and each voice's 3D settings, and the per-voice
  results go to the audio thread in one preallocated batched command,
  applied only to the voice IDs they were computed for. Every voice is
  computed before any result applies, as in SoLoud. A Doppler factor that
  would run a voice's filters faster than their memory allows sends
  memory prepared on the game thread in the same batch, which the renderer
  swaps into the filter instances (`FilterInstance::grow`); what they gave
  up rides back in the batch, so Doppler never allocates or frees on the
  audio thread. Colliders and
  attenuators are `Send + Sync` traits (`Collider`, `Attenuator`) held in
  `Arc`s and called with plain values (`SpatialVoice`), never on the audio
  thread. Doppler and distance delay are computed with the gains and reach
  the voice as its speed and start delay. Handedness is a mixer setting
  (`MixerConfig::left_handed_3d`, SoLoud's `LEFT_HANDED_3D`), right-handed
  by default as Bevy is.
- The Bevy `spatial` slice reads `GlobalTransform` after transform
  propagation into one snapshot of the listener and every emitter per
  frame; the playback sync then starts new emitters' sounds in 3D (an
  emitter whose transform is not finite waits), sends the listener and the
  emitters that changed, and asks for one 3D update when anything did.
  Parent hierarchy changes affect world-space audio. A listener whose
  transform is degenerate (zero scale, not finite) keeps its last pose; its
  directions are taken from the transform's axes, never through
  `GlobalTransform::forward`, which panics on them.
- Axes: Bevy is right-handed with -Z forward and +Y up, which is SoLoud's
  default listener (`left_handed_3d` off), so translations, `forward()` and
  `up()` pass unchanged and a sound to the listener's right (+X) plays on
  the right speaker; tested headless (`spatial_audio`). Scale is ignored.
  The default speaker layouts are SoLoud's.
- One listener: the lowest entity index of several, reported in
  `ListenerState`; with none, the last pose is kept at rest (SoLoud's
  default before any). Multiple-listener mixing is not implicit.
- Units are metres and seconds: the speed of sound is 343.3 m/s by default
  (`SpatialAudioSettings`), and attenuation distances are in metres.
  Velocities are `AudioVelocity` or derived from motion over virtual time:
  the distance moved over the time since the position last changed, held
  for `velocity_hold` (0.1 s) while it does not change, so motion stepped
  less often than frames (fixed-step physics) reads a steady speed; a game
  with a physics engine feeds `AudioVelocity` from its bodies. A zero or
  paused time step, an `AudioTeleport` marker, or a derived speed past
  `max_derived_speed` (200 m/s by default) gives 0.
- Retain audio-rate smoothing where needed. Do not render device buffers from
  Bevy `Update` or read the ECS world from the callback.

### Relationship to built-in audio

This plugin owns a SoLoud-style mixer/output path. It is not a drop-in
implementation of Bevy's built-in `AudioPlugin` or Rodio playback API. Use
distinct asset/component types; a `.wav` loader does not imply compatibility
with built-in `AudioSource` or `AudioPlayer`.

Examples using only this mixer disable/omit Bevy's built-in audio plugin or
feature to avoid accidentally opening a second stream. Coexistence is an
explicit integration mode, not implicit double mixing. CPAL does not
automatically plug this implementation into Bevy's built-in mixer.

## Design changes and verification

Keep current facts separate from proposed behavior. When changing a contract,
update this document and the affected slice tests; record audible differences
and support coverage in `docs/port-status.md` and the parity ledger.

A slice is complete only when enabled code compiles, parity/lifecycle tests
pass, callback behavior is bounded, and every authored Rust file satisfies the
400-line limit. The migration milestones and full validation checklist live
in [PORTING.md](PORTING.md#milestones-and-acceptance-gates).

The Windows (MSVC) reference output is the parity golden. Rust follows the
MSVC build's math on every platform, so its output is expected to be the same
everywhere; other platforms compare against the Windows runner's output with
`run_parity.py --cpp-dir`. Where another platform's C++ build differs from the
golden, the difference is declared per scenario and platform in
`tests/scenarios/tolerances.txt` (`--profile`), with what was measured, rather
than loosening the scenario itself. The supported OS, architecture and host
combinations, and what has been verified on each, are in
[port-status.md](port-status.md#platforms).

`solouder-bench` measures what a device callback costs: each case builds a
mixer, splits it, starts its voices through the `Controller`, and times
`Renderer::render`; `reference_bench.cpp` builds the same cases on the C++
reference and times `Soloud::mix` (`scripts/run_bench.py` prints both against
the callback budget). Its soak test runs a `Controller`, a `Renderer` and a
`StreamWorker` on three threads with a counting allocator that sees only the
rendering audio thread; it is the one place in the workspace with unsafe code
(a `GlobalAlloc` that forwards to `System`), confined to the test, so that
crate denies `unsafe_code` instead of forbidding it. New kinds of sources add
benchmark cases in `cases::all` and plays in the soak. Results are in
[port-status.md](port-status.md#performance).
