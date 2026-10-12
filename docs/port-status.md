# Port status

What the Rust port does today, where it deliberately differs from the C++
reference, and how it is verified. Per-behavior status lives in
[parity-ledger.csv](parity-ledger.csv); the plan is in
[PORTING.md](PORTING.md#milestones-and-acceptance-gates).

## Summary

Milestone C (Windows and Bevy playable slice) is done; milestone B (offline
playback) before it. The mixer plays prepared PCM with 1
to 8 channels on outputs of 1 to 8 channels: play, stop, pause, loop, volume,
pan, relative speed, point/linear/Catmull-Rom resampling, delayed starts,
protected voices, the active-voice limit, global volume and both clippers.
WAV files decode into that PCM. Every scenario renders the same samples as
the C++ reference, bit for bit, except 8-bit WAV (see below).

For real-time use the mixer is split into a `Controller` for the game thread
and a `Renderer` for the audio thread, joined by bounded wait-free queues
(see [ARCHITECTURE.md](ARCHITECTURE.md#ownership-and-the-real-time-boundary)).
`solouder-cpal` runs the renderer on the default output device and
`bevy_solouder` plays `.wav` assets from entities. Other codecs, buses,
filters and 3D audio do not exist yet.

| Crate | Slices / modules | Contents |
|---|---|---|
| `solouder-core` | `contracts` | `Pcm` (validated planar samples), `SAMPLE_GRANULARITY`, `MAX_CHANNELS` |
| `solouder-core` | `dsp` | Resamplers, pan-and-expand for every channel layout, round-off and hard clipping |
| `solouder-core` | `features/playback` | `Mixer`, `MixerConfig`, `Sound`, `Handle`, `PlayOptions`, voice table, active-voice selection, resample-buffer pool, bus mixing, voice control and queries |
| `solouder-core` | `features/realtime` | `split`, `Controller`, `Renderer`, `VoiceId`, `Event`, `Status`, `RealtimeConfig`; retirement of ended voices' PCM to the game thread |
| `solouder-decoders` | `features/decoded_audio` | `decode_wav` (a bounds-checked RIFF reader with dr_wav's sample conversions), `DecodeLimits`, `DecodeError` |
| `solouder-cpal` | `features/output_lifecycle` | `open`, `Output` (state, info, close), `OutputConfig`, stream negotiation, the panic-guarded callback with sample-format conversion |
| `bevy_solouder` | `decoded_audio`, `output_lifecycle`, `playback` | `SolouderPlugin`, `AudioClip` and `WavLoader`, `OutputMode` (device or headless), `AudioOutput`, `SoundPlayer`, `PlaybackSettings`, `SoundStatus`, `SoundFinished`/`SoundFailed` |
| `solouder-parity` | — | Scenario parser, test patterns, WAV fixture loading, Rust runner, comparison with C++ output |

Ledger: the Milestone B playback behaviors are `ported`; `Soloud::init`,
`AudioSource::setVolume` and `AudioSource::setLooping` are `in-progress`
(init flags and the rest of the source-level defaults remain), as are
`Wav::load` and `Wav::loadMem` (WAV only; the other codecs come in E).
Milestone C ports `Soloud::deinit` and the `getBackend*` queries; `init`
stays `in-progress` until its remaining flags are done.

## Verified by scenario

| Scenario | Checks | Samples |
|---|---|---|
| `init_validation` | Accepted and rejected channel counts and buffer sizes; 0 selects the default rate and buffer; a failed init leaves nothing to mix | 32 |
| `silence_block_sizes` | No voices, 1 to 8 channels, blocks of 0 to 4096 frames | 47,097 |
| `play_basic` | Mono and stereo sounds mixed together, a sound ending inside a mix call, handles, voice state and counts | 5,200 |
| `volume_pan_ramps` | Volume and pan ramps over the next mix call, lengths not divisible by 4 (patch 0018), default and zero volume | 1,146 |
| `looping` | Loop wraps inside a block and several times per block, loop counts, turning looping off and on | 7,050 |
| `pause_stop` | Paused voices keep their place; stop, stop_all, stale handles, slot reuse with new play indices | 2,400 |
| `speed_and_resamplers` | Slow, fast and fractional steps with each resampler, speed 0 rejected, extreme steps render silence (patch 0022) | 7,962 |
| `channel_layouts` | Every source layout on every output layout with pan and volume changes (patch 0019) | 10,647 |
| `clipping_and_global` | Sums past full scale through both clippers, global volume, post-clip scale | 4,012 |
| `active_voices` | Protected and loudest voices win the active slots; protecting a voice takes effect at once (patch 0023); changing and invalid limits | 3,212 |
| `delay_samples` | Delayed starts spanning mix calls | 1,666 |
| `sound_tails` | Sounds play out when mixes of 256, 441 or 480 frames end partway through their last block, with every resampler, slow and fast speeds, a resampled stereo sound and a sound of whole blocks (patch 0024) | 25,758 |
| `sound_replace` | Reloading a sound name stops the old sound's voices and keeps other sounds playing; a failed reload keeps the old sound | 2,600 |
| `wav_decode` | 16/24/32-bit integer, 32/64-bit float, A-law and mu-law WAV (every code); extensible 5.1, 7.1, 24-in-32-bit, A-law and mu-law headers; channel masks with a high bit or none; extra and odd-sized chunks; six files both sides reject (9, 33 and 65535 channels, no frames, sample rate 0, a header cut short) | 15,562 |
| `wav_decode_u8` | 8-bit WAV (tolerance 1/128, see below) | 1,000 |
| `wav_playback` | Decoded files played at and away from the output rate, a 5.1 fold-down, a looping file | 8,246 |

Samples are compared bit for bit, except that zero and subnormal values all
count as zero, because the C++ build flushes subnormals. Every scenario has
an identical log and identical samples on Windows x86-64; `wav_decode` has
one subnormal float sample that the C++ build writes as zero, and
`wav_decode_u8` declares a tolerance of 1/128 for the 8-bit conversion. A
scenario that renders no samples fails. Run with
`python scripts/run_parity.py` or
`python scripts/check_all.py --reference --parity`.

## Verified by tests (milestone C)

The real-time and device paths have no C++ counterpart in the null-driver
harness, so they are checked by Rust tests instead of scenarios:

| Test | Checks |
|---|---|
| `realtime_tests` (core, 14 tests) | Renders match an offline `Mixer` sample for sample, a stop queued before the first render still reports start and finish, commands for finished voices do nothing, full queues and ID pools reject requests, each render applies a bounded number of commands, ended audio is released by the controller, all-protected tables fail the play, a stolen voice finishes with `Stolen` before its replacement starts, uncollected audio blocks plays instead of being freed in the render, a dropped renderer disconnects the controller, play options set speed and protection from the first frame |
| `realtime_no_alloc` (core) | No allocation or free inside any `Renderer::render`, with a fresh sound per play and the smallest retirement queue: playing, every control, stopping, voices ending, voice stealing past the slot count, all-protected failures, `stop_all`, and rendering after the controller is dropped |
| `callback` unit tests (cpal) | Long callbacks render in parts identical to manual renders, conversion to `i16`, a panic injected inside the render silences the output and hands its payload off the audio thread, conversions clamp to `[-1, 1)` (24-bit 1.0 would otherwise be out of range) and play NaN as silence, dropping the callback returns the renderer |
| `device_lifecycle` (cpal) | Repeated open/play/close, drop while playing, two outputs at once, invalid voice counts and buffer sizes rejected before the device is opened |
| `headless_playback` (Bevy, 8 tests) | Play to the end, stop on despawn, remove and replace, pause and resume, despawn on finish, `.wav` loading through the asset server, a bad file fails the player |
| `playback_ownership` (Bevy, 8 tests) | Removing `SoundStatus` cannot orphan a voice, a replaced status shows the current state, a stolen sound reports `Stolen` and despawns its entity, starting speed and protection ride in the play request (sample-exact against the mixer with a nearly full queue), unusable settings are ignored and never resent, quiet frames send no commands, a failed output or dropped renderer fails waiting and playing sounds |
| `plugin_lifecycle` (Bevy) | No output keeps the app running and fails plays, despawn before load never starts a voice, a 5000-sound burst gets one outcome per sound with every ID released, repeated device app teardown, prompt drop mid-playback |

Device tests skip on machines without audio output (as in CI) unless
`SOLOUDER_REQUIRE_DEVICE=1`; only "no device", "device not available",
"host unavailable" and "device busy" count as no output; they pass on a Windows WASAPI device. The
workspace also builds, passes clippy and passes its tests on Linux (Docker,
Rust 1.98.1, no device).

## Deliberate differences from the reference

| Area | C++ reference | Rust | Why |
|---|---|---|---|
| Mix length | `Soloud::mix` writes past its scratch buffer when asked for more frames than it holds | `Mixer::mix_interleaved` renders long requests in passes of at most the scratch size; volume and pan ramps then complete per pass | Memory safety; see the open-defect table in PORTING.md |
| Partial frames | Not representable (the C API takes a frame count) | A trailing partial frame in the output slice is set to silence | Rust takes an interleaved slice |
| Sample rate 0 | Means "use the default" (44100) in `Soloud::init` | `MixerConfig` rejects 0; the parity runner maps 0 to 44100 | No hidden defaults in the core type |
| Invalid handles | Getters return 0 or a default value; setters do nothing | Getters return `Option`; `is_valid_voice_handle` is unchanged | A stale handle is distinguishable from a real 0 |
| Play speed | `setRelativePlaySpeed` rejects `<= 0` but accepts NaN | Rejects anything not `> 0`, including NaN | NaN would poison the fixed-point step |
| PCM sample rate | `Wav::loadRawWave*` accepts an infinite rate (patch 0021 rejects only `!(rate > 0)`) | `Pcm::from_planar` requires a finite rate | An infinite rate gives an undefined step |
| Pan constant | `M_PI`, which the MSVC build defines as `3.14159265359` | `std::f64::consts::PI` | The rounded constant is a build artifact; no scenario output differs |
| Loop point | Generic seek to a nonzero loop point writes into the mixer scratch and discards a speed-scaled count of frames (open defect in PORTING.md) | Only loop point 0 exists | The reference behavior corrupts frames already rendered; nonzero loop points wait for a reference fix |
| 8-bit WAV | dr_wav maps 0..255 onto -1..1 (`x * 2/255 - 1`), so silence decodes as +1/255 | `(x - 128) / 128` | Silence must decode as 0 |
| Truncated WAV data | Keeps the declared frame count; the missing frames are silent (patch 0012) | Keeps the frames present | Padding would add a gap to looping sounds |
| Non-finite WAV floats | NaN and infinity pass through | Decode as 0 | One NaN sample silences every voice in the mix |
| WAV variants | dr_wav also reads MS/IMA ADPCM, 12-bit and 64-bit integer PCM | `DecodeError::Unsupported` | Not implemented yet |
| Unknown files | `Wav::load` detects WAV, Ogg, FLAC and MP3, and tries MP3 for anything else | `decode_wav` reads WAV only and reports `NotRecognized` | Other codecs come in milestone E; the caller picks the decoder |
| Decode limits | Only the `u32` size checks of patch 0012 | `DecodeLimits::max_samples` (default 256 Mi samples); allocation follows decoded data, not header claims | Untrusted files cannot request huge allocations |
| Deleting a sound | `~Wav` stops every voice playing it | Dropping a `Sound` leaves its voices playing; they share its samples and release them when they end | No dangling source; stop voices explicitly (the Bevy plugin stops a player's voice when its component goes away) |
| Voice end | The audio thread releases the source instance under the engine mutex | Through `Renderer`, an ended or stopped voice's PCM goes back to the controller and is freed on the game thread; an offline `Mixer` drops it in the mix call unless `keep_retired_audio` is set | Freeing memory on the audio thread can block |
| Control | Every call locks the audio mutex and applies immediately | Controller calls queue a command, applied at the start of the next render; getters read the last published `Status` | The audio thread never waits for the game |
| Voice IDs | `play` returns a handle once the voice exists | The controller reserves a `VoiceId` when the play is queued; with all 4096 reserved, `play` fails with `NoFreeVoiceId` (the Bevy plugin reports `NoFreeVoice`) instead of starting late | Commands can target a voice before it starts; a late sound is worse than none |
| Voice stealing | A new voice silently takes the oldest unprotected voice's slot | The stolen voice reports `Finished(id, FinishReason::Stolen)` before the new voice's `Started` | The owner learns why its sound ended |
| Finished-audio backpressure | Not applicable (freed in place) | Each queued play reserves room for its audio to come back; while the controller has not collected finished audio with `poll_event` or `release_retired_audio`, `play` fails with `QueueFull` | The render never frees memory, however long the game goes without polling |
| Play settings | `play` takes volume, pan and paused; speed and protection are later calls | `PlayOptions` also carries speed and protection, applied before the first frame | A queued follow-up command could miss the first render |
| Output conversion | The backend converts whatever the mix holds | Samples are clamped to `[-1, 1)` and NaN plays as silence before conversion | Integer conversions of 1.0 overflow (24-bit gives an invalid value) |
| Callback panic | Undefined (an exception unwinds into the backend) | The output plays silence from then on, `Output::state` reports `Panicked`, and `Output::panic_message` gives the message; best-effort, because the panic hook still runs on the audio thread | Unwinding into a foreign callback is undefined behavior |
| Lost device | Backend-specific | `Output::state` reports `Disconnected` or `Failed` and plays nothing; there is no automatic reconnection yet | Reopening is an application decision for now |
| Shutdown | `deinit` can free state while the device thread still runs (patch 0010) | The stream is dropped first, which stops the callback; the renderer is handed back (waiting up to half a second for backends that release the callback late) and freed afterwards | Teardown order is enforced by ownership |

Known reference defects fixed on both sides by patches 0018 to 0024 (pan ramp
leftovers, surround fold-down volumes, no-free-voice handling, raw layout
validation, the #343 hang, protection not recomputing the active voices,
sound endings dropped when a mix ends mid-block) are listed in
[PORTING.md](PORTING.md#reference-patch-ledger). Patch 0025 (sounds, buses
and queues that outlive their engine or play on several engines) has no Rust
counterpart: sounds hold no engine pointer, and voices own their samples.

## Not yet covered

- Init flags other than the clipper (`LEFT_HANDED_3D`, ...), `mixSigned16`,
  device reconnection, and device output outside Windows WASAPI (checked
  only to build and skip on Linux).
- Inaudible behavior, 3D audio, filters, buses, faders, voice groups and
  queues (Milestone D).
- Streaming, Ogg Vorbis, FLAC, MP3 and WAV ADPCM (Milestone E).
