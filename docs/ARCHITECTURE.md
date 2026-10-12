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

**Status:** no Rust workspace or Bevy plugin is implemented in this repository
yet; it currently holds the patched C++ reference tooling and the parity
ledger. The paths, crate names, features, and Rust-facing types below are
proposed. The Rust file-size checker exists independently of that future
workspace; its presence does not imply that a Rust implementation exists.

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
  solouder-cpal/src/
    lib.rs
    features/
      output_lifecycle/
  bevy_solouder/src/
    lib.rs
    plugin.rs
    features/
      playback/
      decoded_audio/
      streaming/
      routing/
      filters/
      spatial/
      visualization/
tests/fixtures/                 # small, redistributable audio fixtures
docs/
  parity-ledger.csv             # C API parity target and per-behavior status
  port-status.md                # known differences, enabled features, benchmarks
reference/
  upstream.json, patches/       # pinned upstream SoLoud and reviewed patches
  tests/                        # C++ regression tests for the patches
  soloud/                       # git-ignored checkout (scripts/fetch_reference.py)
```

This illustrates feature ownership, not a requirement to create empty modules
up front. Introduce a slice when it has an implemented use case. File names
inside a slice should describe their responsibility; no single feature or
crate-root file is allowed to grow beyond 400 lines.

Dependency direction is `bevy_solouder` → output/decoder/core crates, with
output and decoder crates depending on `solouder-core`, never the reverse. The
names were free on crates.io on 2026-10-12 (`soloud` and `soloud-sys` belong to
unrelated bindings); they are not reserved until published.
The crates are dependency/execution boundaries, not horizontal business layers.

- Start core with ordinary Rust `std`; `no_std` is not an initial goal.
- CPAL is the initial device-output adapter, not a core DSP dependency. Isolate
  negotiation and target-specific stream requirements in that adapter.
- Prefer maintained Rust decoders, with Symphonia as an initial candidate for
  WAV, Ogg Vorbis, FLAC, and MP3. Check enabled codecs, channel layouts, seek
  behavior, numerical differences, and licenses before committing to a library.
  Ogg container support does not automatically include every Ogg codec.
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

For the initial CPAL path, the callback drives mixing with preallocated state.
Offline tests own the same mixer directly without queues or a device. Define
queue ownership and producer/consumer topology before choosing a channel or
ring buffer; a supposedly lock-free dependency is not enough by itself.

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
pointers. Audio clocks advance from rendered frames, not Bevy frame delta.

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

The proposed `bevy_solouder` crate provides a `SolouderPlugin`, an immutable audio
asset, a thread-safe controller resource, playback components, and listener/
emitter components. These are design names, not existing APIs. Keep each
feature's components, systems, validation, and tests within its slice.

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
| 3D listener and source | Listener and spatial-emitter components read from `GlobalTransform`; velocity explicit or derived. |
| Playback status, stream time, finished | A status component updated from the status handoff, a finished event, and an opt-in despawn-when-finished policy. |
| Visualization and FFT | A snapshot component on the bus or master entity. |
| `playClocked`, scheduling | Explicit commands in audio-clock time. |

Rules for the mapping:

- Components hold requested state; the mixer owns applied state, which flows
  back through status. Systems never wait for the audio thread.
- One sync system set runs after transform propagation, collects every change
  for the frame, and submits one bounded command batch.
- Detached one-shot sounds (no entity) are an explicit, documented choice.
- Several simultaneous listeners (split-screen) are a deliberate feature with a
  defined mixing policy, not an accident of spawning two listener components.
- Bevy is pinned (0.20 at the time of writing). Check the exact names for
  observers, relationships, hooks and messages against that version when the
  crate is written in milestone C.

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
  message/event facilities. Preserve completions for consumers that run less
  frequently than the callback.
- Define startup, device failure, and teardown. Offline/no-device mode permits
  headless Bevy lifecycle tests without hardware.

### Spatial audio

- Read `GlobalTransform` after transform propagation and submit one coherent
  listener/emitter snapshot. Parent hierarchy changes affect world-space audio.
- Document/test forward and up axes, handedness, units, speaker layout, and
  scale in the Bevy-to-mixer mapping. Do not assume C++ defaults match Bevy.
- Select one active listener initially. Reject ambiguity or specify a
  deterministic selection policy; multiple-listener mixing is not implicit.
- Convert units consistently for attenuation, Doppler, and speed of sound.
  Explicit or derived velocities must handle zero time steps, teleports, and
  paused simulation.
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
and support coverage in `docs/port-status.md` (and the parity ledger) once that file exists.

A slice is complete only when enabled code compiles, parity/lifecycle tests
pass, callback behavior is bounded, and every authored Rust file satisfies the
400-line limit. The migration milestones and full validation checklist live
in [PORTING.md](PORTING.md#milestones-and-acceptance-gates).
