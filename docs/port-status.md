# Port status

What the Rust port does today, where it deliberately differs from the C++
reference, and how it is verified. Per-behavior status lives in
[parity-ledger.csv](parity-ledger.csv); the plan is in
[PORTING.md](PORTING.md#milestones-and-acceptance-gates).

## Summary

Milestone D (routing and DSP) is done: fades, oscillations,
scheduled pauses and stops, clocked play, the remaining voice controls (loop
points, seeking, autostop, inaudible ticking, absolute pan and channel gains,
sample rates, pause all, background play, single-instance sounds, stopping
and counting a sound's voices), 8- and 16-bit raw loads, 16-bit output,
voice groups, SoLoud's FFT, and per-voice and global filters (biquad,
lofi, echo, waveshaper, DC removal, the FFT pitch filter, bassboost,
robotize, flanger and freeverb) with live parameters, fades and oscillations,
buses (nested, with their own channels, resampler, filters and
visualization data) and queues, offline and through the real-time
`Controller` (see [PORTING.md](PORTING.md#milestone-d-review)).
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
`bevy_solouder` plays `.wav` assets from entities. WAV files can also be
streamed, on the offline `Mixer` only for now. Other codecs and 3D audio do
not exist yet. Voice groups and the voice, bus and queue queries are
available on the offline `Mixer` only (see "Real-time coverage" below).

| Crate | Slices / modules | Contents |
|---|---|---|
| `solouder-core` | `contracts` | `Pcm` (validated planar samples from `f32`, `u8` or `i16`), `SAMPLE_GRANULARITY`, `MAX_CHANNELS` |
| `solouder-core` | `dsp` | Resamplers, pan-and-expand for every channel layout, round-off and hard clipping, faders (linear fades and sine oscillation), `fft` (Ooura's split-radix FFT as SoLoud has it), `waveform` (`generateWaveform`) |
| `solouder-core` | `features/playback` | `Mixer`, `MixerConfig`, `Sound`, `Stream` (with the `StreamSource` and `StreamReader` traits), `Bus`, `Queue`, `Playable`, `SoundId`, `Handle`, `PlayOptions`, `FilterTarget`, voice table, active-voice selection, resample-buffer pool, bus mixing (nested bus voices, bus seeking, visualization data), queued sounds, voice control and queries, fades and timers, voice groups, sound, bus and global filter slots and their parameters, `f32` and `i16` output |
| `solouder-core` | `features/filters` | `Filter` and `FilterInstance` traits, `FilterParams` (live values with fades), `BiquadResonantFilter`, `LofiFilter`, `EchoFilter`, `WaveShaperFilter`, `DcRemovalFilter`, `FftFilter`, `BassboostFilter`, `RobotizeFilter`, `FlangerFilter`, `FreeverbFilter` |
| `solouder-core` | `features/realtime` | `split`, `Controller` (plays, voice and global controls, fades and schedules, filters and their parameters, bus settings and `annex_sound`, queued sounds), `Renderer`, `VoiceId`, `FilterTarget`, `Event`, `Status`, `RealtimeConfig`; voices and filter instances prepared on the game thread, and ended voices, replaced filters and rejected queue sounds retired to it |
| `solouder-decoders` | `features/decoded_audio` | `decode_wav` (a bounds-checked RIFF reader with dr_wav's sample conversions), `DecodeLimits`, `DecodeError` |
| `solouder-decoders` | `features/streaming` | `WavStream` (a file, bytes or a file read into memory; per-voice readers that seek by frame) |
| `solouder-cpal` | `features/output_lifecycle` | `open`, `Output` (state, info, close), `OutputConfig`, stream negotiation, the panic-guarded callback with sample-format conversion |
| `bevy_solouder` | `decoded_audio`, `output_lifecycle`, `playback` | `SolouderPlugin`, `AudioClip` and `WavLoader`, `OutputMode` (device or headless), `AudioOutput`, `SoundPlayer`, `PlaybackSettings`, `SoundStatus`, `SoundFinished`/`SoundFailed` |
| `solouder-parity` | — | Scenario parser, test patterns, WAV fixture loading, Rust runner, comparison with C++ output |

Ledger: the Milestone B playback behaviors are `ported`; `Soloud::init`,
`AudioSource::setVolume` and `AudioSource::setLooping` are `in-progress`
(init flags and the rest of the source-level defaults remain), as are
`Wav::load` and `Wav::loadMem` (WAV only; the other codecs come in E).
Milestone C ports `Soloud::deinit` and the `getBackend*` queries; `init`
stays `in-progress` until its remaining flags are done. Milestone E ports `WavStream::getLength`;
the `WavStream` lifecycle, `load`, `loadMem` and `loadToMem` are
`in-progress` (WAV only, offline only).

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
| `fades` | Volume, pan and speed fades and oscillations on voice stream time (waiting while paused), which setters cancel them (not `setPan`), fading to the current value, speeds the oscillation passes that are not above 0, oscillations from a higher value to a lower one (patch 0051), scheduled pause and stop and their cancelling, the global volume fade and oscillation ramping across each mix, `playClocked` delays and their limits | 36,298 |
| `voice_controls` | Loop points (one past the end of the sound, one at double speed: patches 0028 and 0029), seeking forward, back and past the end, autostop off and on, sample rate changes including a negative rate (patch 0027), pause all, background play, version and the remaining voice queries | 42,920 |
| `channel_gains` | Absolute pan on every source layout, absolute pan cancelling a pan fade, per-channel gains (ignored past the source's channels), 16-bit output through both clippers including values past full scale | 15,400 |
| `sound_instances` | Counting and stopping a sound's voices through the engine and through the sound, single-instance sounds, length, 8- and 16-bit raw loads and rejected layouts, sound defaults reaching voices, ticking voices taking active slots even when paused (patch 0026), hard pan (patch 0030) | 11,599 |
| `voice_groups` | Group handles and index reuse, adding stopped voices, duplicates and groups, group handles in setters, fades, schedules and stop, reads through the first member (also once it has stopped), emptiness and trimming, destroying | 7,200 |
| `filters_voice` | Every time-domain filter on sounds, chained in slot order, at the voice's rate as speed and sample rate change (recalculating the biquad), a cutoff above a slow voice's Nyquist frequency (patch 0031), lofi on eight channels (patch 0032), slots changed after play | 48,244 |
| `filters_global` | Global filters in slot order on mono, stereo and 5.1 mixes, odd mix sizes and one-frame mixes (patch 0038), replacing and clearing slots | 26,371 |
| `filter_params` | Settings validation and defaults, parameter descriptions, set, fade, oscillate and query on voices, groups and global filters, indices past the last slot or parameter, an echo delay longer than its line and a delay of 0 (patch 0033), the waveshaper at amount -1 (patch 0034) | 16,800 |
| `fft` | `FFT::fft` and `FFT::ifft` on noise and saw signals for every power-of-two size from 4 to 4096 floats, which covers every kernel path | 17,656 |
| `filters_spectral` | The FFT pitch filter and bassboost on mono, stereo and 6-channel voices and as global filters, across mix calls and blocks of any size, at a changing voice rate; wet and boost set, faded and oscillated; bassboost validation | 32,577 |
| `filters_modulation` | Robotize with every waveform and an index without one, periods that are not at least 1 (patch 0042) and parameter fades (patch 0041); the flanger with its channels sharing the LFO (patch 0043), delays that grow its line and a delay of 0 (patch 0044); freeverb on stereo voices, a mono voice and a mono engine (left unchanged, patch 0039), with its settings applied (patch 0040), faded and frozen; settings validation (patch 0045) and parameter descriptions | 36,818 |
| `buses` | Sounds and a bus played through buses, mono, stereo and quad buses on a 48 kHz output (so the bus voices resample), `Bus::play`'s default volume of 1 (#381), clocked plays through a bus, speed, volume and pan on a bus voice, changing the resampler (including an index without one), visualization and filters of a playing bus and filters set before it plays, invalid channel counts, approximate volumes for channels past the bus's (patch 0047), wave and FFT data, annexing a voice and a group, a voice given a bus handle the bus never looked up (not mixed, not stopped with it), stopping, replaying and `stopAudioSource` on a bus stopping its voices (#280) | 23,946 |
| `bus_seek` | Seeking mono, stereo and 5.1 buses with voices of 1, 2 and 6 channels in passes whose strides are not multiples of 16 (patch 0046), seeking back (`NOT_IMPLEMENTED`), seeking a sped-up bus, and a seek after a voice was replaced in its slot before the next mix (patch 0048), and a quad bus whose seeks end in blocks of 285 and 193 frames (visualization data from the last) | 28,104 |
| `queues` | Queued sounds playing back to back on mono, stereo and 22050 Hz queues, `Queue::play` before the queue plays and past 32 sounds, a looping sound dropped once read (patch 0049), sounds with fewer channels than the queue (patch 0050) and with more, a queue on a bus, its volume and filters, an empty queue playing silence, sounds waiting while the queue is stopped, the voice stopping once the queue runs dry, `isCurrentlyPlaying` and `getQueueCount`, seeking forward at speed 1.25 and back (`NOT_IMPLEMENTED`), playing a queue again or replacing it stopping its voice, and a looping queue whose loop wrap can only seek forward | 36,142 |
| `sound_tails` | Sounds play out when mixes of 256, 441 or 480 frames end partway through their last block, with every resampler, slow and fast speeds, a resampled stereo sound and a sound of whole blocks (patch 0024) | 25,758 |
| `sound_replace` | Reloading a sound name stops the old sound's voices and keeps other sounds playing; a failed reload keeps the old sound | 2,600 |
| `wav_decode` | 16/24/32-bit integer, 32/64-bit float, A-law and mu-law WAV (every code); extensible 5.1, 7.1, 24-in-32-bit, A-law and mu-law headers; channel masks with a high bit or none; extra and odd-sized chunks; six files both sides reject (9, 33 and 65535 channels, no frames, sample rate 0, a header cut short) | 15,562 |
| `wav_decode_u8` | 8-bit WAV (tolerance 1/128, see below) | 1,000 |
| `wav_playback` | Decoded files played at and away from the output rate, a 5.1 fold-down, a looping file | 8,246 |
| `stream_wav` | Streamed WAV files loaded from a file, into memory and from bytes, at and away from the output rate, 5.1 and quad float files folded down, A-law; a looping stream with a loop point wrapping several times (the loop-wrap seek and channel stride of patches 0002 and 0004, and patch 0005's looping streams not ending); seeks back, forward and past the end; streams ending, counted, replayed and stopped; two files both sides reject | 26,496 |

Samples are compared bit for bit, except that zero and subnormal values all
count as zero, because the C++ build flushes subnormals. Every scenario has
an identical log and identical samples on Windows x86-64; `wav_decode` has
one subnormal float sample that the C++ build writes as zero, and
`wav_decode_u8` declares a tolerance of 1/128 for the 8-bit conversion. A
scenario that renders no samples fails. The output of a Linux GCC build of
the reference (Ubuntu 20.04) is identical to the Windows output for every
scenario except `filters_spectral` (see "Math functions" below). Run with
`python scripts/run_parity.py` or
`python scripts/check_all.py --reference --parity`.

## Verified by tests (milestones C and D)

The real-time and device paths have no C++ counterpart in the null-driver
harness, so they are checked by Rust tests instead of scenarios:

| Test | Checks |
|---|---|
| `realtime_tests` (core, 14 tests) | Renders match an offline `Mixer` sample for sample, a stop queued before the first render still reports start and finish, commands for finished voices do nothing, full queues and ID pools reject requests, each render applies a bounded number of commands, ended audio is released by the controller, all-protected tables fail the play, a stolen voice finishes with `Stolen` before its replacement starts, uncollected audio blocks plays instead of being freed in the render, a dropped renderer disconnects the controller, play options set speed and protection from the first frame |
| `routing_tests` (core, 3 tests) | A real-time session with voice and global filters, a filtered bus, a queue, clocked and background plays, fades, filter parameter fades, seeks, a bus filter swap, `annex_sound` and `stop_sound` renders the same samples as the offline `Mixer`, bit for bit; bus and queue plays report their events, and a play through a bus that is not playing fails; queues past `RealtimeConfig::queues` are refused |
| `layout_tests` (core, 5 tests) | Filters whose delays change before their first block, and a bus whose channel setting changed while it plays, render the same samples as the offline `Mixer`; a mixer split after offline use (queue storage, a queued sound, a playing voice) never overflows its retirement list; `Mixer::keep_retired` keeps voices played after it, and `split` trims the surplus voice boxes left by offline overflow; a voice a bus seek plays out reports `Ended` even when a play in the same render takes its slot |
| `realtime_no_alloc` (core) | No allocation or free inside any `Renderer::render`, with a fresh sound per play and the smallest retirement queue: playing, every control, stopping, voices ending, voice stealing past the slot count, all-protected failures, `stop_all`, rendering after the controller is dropped, and voices, a bus and a queue using every filter that allocates lazily (echo, DC removal, FFT, bassboost, flanger), with global and bus filter swaps, seeks, and sounds queued past the queue's limit; and filters first running on a bus at double speed whose channel setting changed, after delays were changed, after a failed replay of the bus and during a speed oscillation from 3 to 1, and with a flanger delay growing to its maximum; and a mixer split while it holds global filters and filtered voices that have not played yet |
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
| Filter parameter reads | `getFilterParameter` returns `INVALID_PARAMETER` (1.0) without a filter and 0 past the last parameter; handle 0 means the global filters | `Mixer::filter_parameter` returns `None` for both; `FilterTarget::Global` replaces handle 0 | 1.0 and 0 are also real parameter values |
| Filter objects | `setFilter` keeps a pointer: changed settings reach later plays, and deleting the filter first leaves the sound with a dangling pointer | Sounds hold an `Arc<dyn Filter>` whose settings are fixed; set a new filter to change them | No dangling filters, and no locking between the game and audio threads |
| Biquad type | `setParams` takes an `int` and accepts 0 to 3 (3 acts as lowpass) | `BiquadType` has the three real types; the live `TYPE` parameter still takes any value | A type that does not exist is not a setting |
| Filters in real time | Filter instances are created, and their delay lines and FFT buffers allocated, on the audio thread, under the engine mutex | `Controller` creates a voice's filter instances, and the global and bus instances it sets, on the game thread, and reserves their buffers' memory for the channels and rate they will run at (`FilterInstance::prepare`); the buffers are still sized at their first use, inside that memory, so the output matches the reference. Replaced instances are retired to the game thread | The audio thread neither allocates nor frees in normal use (see "Real-time coverage" for what needs more than was reserved) |
| Math functions | SoLoud's sources include `math.h`, not `cmath`, so on MSVC `sin`, `cos`, `atan2` and `sqrt` of a `float` run in double precision; with GCC's libstdc++ they resolve to the `float` versions | Follows the MSVC build: the value is widened to `f64`, the function runs, and the result is rounded to `f32` | The Windows reference is the one the scenarios compare against. A Linux GCC build of the reference renders every scenario identically except `filters_spectral` (the FFT filter's `atan2`, `cos` and `sin`), which differs in the last bits |
| Robotize waveform | `setParams` takes the waveform as an `int` | Also an `i32`, with the waveforms as `dsp::waveform` constants; indices without a waveform act as square, as in C++ | Keeps the live `WAVE` parameter and the setting in step |
| FFT filter buffers | Sized for the channel count of the first call; a later call with more channels writes past them | Grow to the larger count | Memory safety; unreachable from the mixer, which keeps a voice's channel count |
| Flanger line allocation | A failed `new` leaves the audio unfiltered; a line of 2^32 samples or more wraps its size | `try_reserve_exact`; on failure the audio is left unfiltered | Same outcome without the wrap |
| Sound identity | A copied `AudioSource` keeps its ID, so stopping either copy stops both | A cloned `Sound` keeps its `SoundId` the same way; `Sound::new` gives a new one | Same behavior; cloning is how Rust shares a sound |
| Seek result | `Soloud::seek` returns an error code: `NOT_IMPLEMENTED` when a bus would seek back, otherwise `SO_NO_ERROR` | `Mixer::seek` returns `Err(ControlError::CannotSeekBack)` or `Ok(())` | There are no result codes |
| Bus play volume | `Bus::play` and `Bus::playClocked` default the volume to 1, while `Soloud::play` defaults to the source's (#381) | `Mixer::bus_play` takes `PlayOptions`, whose `volume: None` uses the source's volume everywhere; the parity runner passes 1 to match | One default for every play call |
| Bus settings | `Bus::setFilter`, `setResampler` and `setVisualizationEnable` change the bus and its playing instance | `Bus` setters affect later plays; `Mixer::set_bus_filter`, `set_bus_resampler` and `set_bus_visualization` change both | A `Bus`, like a `Sound`, holds no pointer to its voices |
| Bus instances | A bus has one instance: playing it again, on any engine, stops the previous one | Playing a bus again stops its voice on that mixer only | A bus does not know the mixers playing it (as for `Sound`) |
| Bus wave data | `getWave` and `calcFFT` return the bus's last copy once its voice has stopped | `Mixer::bus_wave` and `bus_fft` return `None` | A stale copy looks like live data |
| Queued sources | `Queue::play` takes any `AudioSource`, including buses, queues and streams | `Mixer::queue_play` takes a `Sound` | Streams come in milestone E; queuing a bus or a queue mixes nothing useful |
| Queued sounds | One list per `Queue`, shared by every engine it plays on; destroying a queued `Wav` leaves the queue reading freed samples | One list per queue on each mixer; a queued sound holds its samples | A `Queue`, like a `Sound`, holds no pointer to mixers |
| Queue instances | A queue has one instance: playing it again, on any engine, stops the previous one | Playing a queue again stops its voice on that mixer only | As for buses |
| Error strings | `getErrorString` names result codes | Error enums implement `Display` | There are no result codes |
| Engine before init | `Soloud` exists before `init`; calls then do nothing | `Mixer::new` validates and initializes in one step | No half-initialized state |
| Play speed | `setRelativePlaySpeed` rejects `<= 0` but accepts NaN | Rejects anything not `> 0`, including NaN | NaN would poison the fixed-point step |
| PCM sample rate | `Wav::loadRawWave*` accepts an infinite rate (patch 0021 rejects only `!(rate > 0)`) | `Pcm::from_planar` requires a finite rate | An infinite rate gives an undefined step |
| 8-bit WAV | dr_wav maps 0..255 onto -1..1 (`x * 2/255 - 1`), so silence decodes as +1/255 | `(x - 128) / 128` | Silence must decode as 0 |
| Truncated WAV data | Keeps the declared frame count; the missing frames are silent (patch 0012) | Keeps the frames present | Padding would add a gap to looping sounds |
| Non-finite WAV floats | NaN and infinity pass through | Decode as 0 | One NaN sample silences every voice in the mix |
| WAV variants | dr_wav also reads MS/IMA ADPCM, 12-bit and 64-bit integer PCM | `DecodeError::Unsupported` | Not implemented yet |
| Stream length | `WavStream` takes its frame count from the header | `WavStream` counts the whole frames present in the file | The header can claim more than the file holds; with patch 0054 both end where the data does |
| Changed stream files | Each instance reopens the file and trusts the header read at load | A file reopened with a different encoding, channel count or rate ends the voice, like a missing file (patch 0053); one that got shorter plays what is left | A reader must produce the layout the voice was set up for |
| Dropping a stream | `~WavStream` stops its voices | Dropping a `Stream` leaves its voices playing; each owns its reader | As for `Sound` |
| Streams in real time | Played like any source; the audio thread reads the file | `Controller::play` refuses a `Stream` with `StreamsNotSupported` until decoding moves to a worker (milestone E unit 3) | File reads on the audio thread can stall it |
| Unknown files | `Wav::load` detects WAV, Ogg, FLAC and MP3, and tries MP3 for anything else | `decode_wav` reads WAV only and reports `NotRecognized` | Other codecs come in milestone E; the caller picks the decoder |
| Decode limits | Only the `u32` size checks of patch 0012 | `DecodeLimits::max_samples` (default 256 Mi samples); allocation follows decoded data, not header claims | Untrusted files cannot request huge allocations |
| Deleting a sound | `~Wav` stops every voice playing it | Dropping a `Sound` leaves its voices playing; they share its samples and release them when they end | No dangling source; stop voices explicitly (the Bevy plugin stops a player's voice when its component goes away) |
| Voice end | The audio thread releases the source instance under the engine mutex | Through `Renderer`, an ended or stopped voice's PCM goes back to the controller and is freed on the game thread; an offline `Mixer` drops it in the mix call unless `keep_retired` is set | Freeing memory on the audio thread can block |
| Control | Every call locks the audio mutex and applies immediately | Controller calls queue a command, applied at the start of the next render; getters read the last published `Status` | The audio thread never waits for the game |
| Voice IDs | `play` returns a handle once the voice exists | The controller reserves a `VoiceId` when the play is queued; with all 4096 reserved, `play` fails with `NoFreeVoiceId` (the Bevy plugin reports `NoFreeVoice`) instead of starting late | Commands can target a voice before it starts; a late sound is worse than none |
| Voice stealing | A new voice silently takes the oldest unprotected voice's slot | The stolen voice reports `Finished(id, FinishReason::Stolen)` before the new voice's `Started` | The owner learns why its sound ended |
| Finished-audio backpressure | Not applicable (freed in place) | Each queued play reserves room for its audio to come back; filter swaps and queued sounds reserve room the same way; while the controller has not collected finished items with `poll_event` or `release_retired`, these calls fail with `QueueFull` | The render never frees memory, however long the game goes without polling |
| Play settings | `play` takes volume, pan and paused; speed and protection are later calls | `PlayOptions` also carries speed and protection, applied before the first frame | A queued follow-up command could miss the first render |
| Output conversion | The backend converts whatever the mix holds | Samples are clamped to `[-1, 1)` and NaN plays as silence before conversion | Integer conversions of 1.0 overflow (24-bit gives an invalid value) |
| Callback panic | Undefined (an exception unwinds into the backend) | The output plays silence from then on, `Output::state` reports `Panicked`, and `Output::panic_message` gives the message; best-effort, because the panic hook still runs on the audio thread | Unwinding into a foreign callback is undefined behavior |
| Lost device | Backend-specific | `Output::state` reports `Disconnected` or `Failed` and plays nothing; there is no automatic reconnection yet | Reopening is an application decision for now |
| Shutdown | `deinit` can free state while the device thread still runs (patch 0010) | The stream is dropped first, which stops the callback; the renderer is handed back (waiting up to half a second for backends that release the callback late) and freed afterwards | Teardown order is enforced by ownership |

Known reference defects fixed on both sides by patches 0018 to 0024 and 0026
to 0054 (pan ramp leftovers, surround fold-down volumes, no-free-voice
handling, raw layout validation, the #343 hang, protection and ticking not
recomputing the active voices, sound endings dropped when a mix ends
mid-block, the hang on a negative sample rate, seeks scaled by play speed,
loop wraps overwriting rendered frames, the rounded `M_PI` of MSVC builds,
unstable biquads above the voice's Nyquist frequency, lofi state for two
channels only, zero-length echo and DC removal lines, the waveshaper's 0 / 0,
DC removal reading its freed filter, NaN filter settings, the biquad reading
before a one-sample buffer, freeverb reading past mono audio and ignoring its
settings, freeverb and robotize ignoring parameter fades, robotize dividing by
a period of 0, the flanger's channels each advancing the LFO, flanger
delays of 0 or past its line, bus seeks with unaligned strides or more voice
channels than the bus has, a bus volume read past its channels, bus seeks
mixing voices that have no resample buffers yet, a queue spinning forever on
a looping sound, queue channels left over from the previous sound, and
oscillations from a higher value to a lower one running above both, and
resampled voices skipping part of a step at every block (#314), a stream
whose file is gone crashing, and a stream cut off in its data never ending) are
listed in
[PORTING.md](PORTING.md#reference-patch-ledger). Patch 0025 (sounds, buses
and queues that outlive their engine or play on several engines) has no Rust
counterpart: sounds hold no engine pointer, and voices own their samples.

## Real-time coverage

Through `Controller`, every milestone D control applies on the audio thread
at the start of the next render, and the result matches the offline `Mixer`
bit for bit (`routing_tests`):

- Plays: `play`, `play_with`, `play_clocked`, `play_background`, `bus_play`
  and `bus_play_clocked`, for sounds, buses and queues.
- Voice controls: stop, stop all, stop a sound's voices, volume, pan,
  absolute pan, channel volume, pause, pause all, looping, loop point,
  autostop, inaudible behavior, protection, delay, seek, speed, sample rate,
  their fades and oscillations, and scheduled pauses and stops.
- Global volume with its fade and oscillation, the post-clip scaler and the
  resampler.
- Filters: global filter slots, and setting, fading and oscillating the
  parameters of global and voice filters (`FilterTarget`); bus filters,
  resampler and visualization (on the `Bus` and its playing voice);
  `annex_sound`; `queue_play`.

Limits:

| Area | Behavior |
|---|---|
| Mixer only | Voice groups, voice getters other than those in `Status`, bus wave, FFT and voice counts, queue counts and `is_current`, and filter parameter reads |
| Seek errors | A seek backwards on a bus or queue voice, which they cannot do, is ignored; `CannotSeekBack` is not reported |
| Queued sounds | A sound the renderer cannot queue (queue not playing, or full) is retired and counted in `Status::rejected_queue_sounds`, not reported on its own. Each queued sound holds a retirement credit until it has played and been collected. A controller plays at most `RealtimeConfig::queues` distinct queues (default 64) besides those the mixer already had at `split`; each new queue's list is allocated on its first play |
| Filter sizing | Memory is reserved for a voice's filters at its channels and starting rate, for global filters at the output layout, and for bus filters at the largest layout among the bus's voices this controller requested and has not seen finish (channels, and the fastest speed each was set, faded or oscillated to); a bus played before `split` uses the `Bus`'s current settings. `split` reserves for the filters the mixer already holds, at each voice's current rate or the top of its speed fade. Echo reserves for the delay it was created with, its `DELAY` parameter's maximum (larger values are accepted); the flanger for 0.1 s, its `DELAY` maximum, or its own delay if longer. The render allocates when a filter needs more than that: echo or DC removal first running at a higher rate than reserved (a voice sped up or given a higher sample rate before its first mixed block), an echo delay set above its maximum before its first block, or a flanger whose rate or delay grows past its reserved line at any time |
| Finish reasons | `FinishReason::Ended` covers playing to the end, including during a bus seek, and a scheduled stop coming due; `Stopped` covers stop commands, stopping a bus or a sound's voices, and a play that replaced the voice |

## Not yet covered

- Init flags other than the clipper (`LEFT_HANDED_3D`, ...), device
  reconnection, and device output outside Windows WASAPI (checked only to
  build and skip on Linux).
- 3D audio with the `INAUDIBLE` flag it sets, including `Bus::play3d` and
  `play3dClocked` (Milestone E).
- Voice groups, voice getters, bus wave, FFT and voice counts, queue counts,
  and filter parameter reads through the `Controller` (see "Real-time
  coverage").
- Streaming through the `Controller` and queued streams (Milestone E unit
  3); Ogg Vorbis, FLAC, MP3 and WAV ADPCM (Milestone E).
