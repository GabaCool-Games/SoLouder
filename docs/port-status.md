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
streamed, offline or through the `Controller`, whose `StreamWorker` decodes
them ahead of the audio thread; `bevy_solouder` streams `.wav` assets
loaded with `WavSettings::stream`. 3D audio (milestone E unit 6) places
voices with a listener, attenuation models, listener-relative sources and
speaker positions for 1 to 8 channels, on the offline `Mixer` only; Doppler,
distance delay, colliders and attenuators come next. Other codecs do not
exist yet. Voice groups, 3D audio and the voice, bus and queue queries are
available on the offline `Mixer` only (see "Real-time coverage" below).

| Crate | Slices / modules | Contents |
|---|---|---|
| `solouder-core` | `contracts` | `Pcm` (validated planar samples from `f32`, `u8` or `i16`), `SAMPLE_GRANULARITY`, `MAX_CHANNELS` |
| `solouder-core` | `dsp` | Resamplers, pan-and-expand for every channel layout, round-off and hard clipping, faders (linear fades and sine oscillation), `fft` (Ooura's split-radix FFT as SoLoud has it), `waveform` (`generateWaveform`) |
| `solouder-core` | `features/playback` | `Mixer`, `MixerConfig`, `Sound`, `Stream` (with the `StreamSource` and `StreamReader` traits), `Bus`, `Queue`, `Playable`, `SoundId`, `Handle`, `PlayOptions`, `FilterTarget`, voice table, active-voice selection, resample-buffer pool, bus mixing (nested bus voices, bus seeking, visualization data), queued sounds, voice control and queries, fades and timers, voice groups, sound, bus and global filter slots and their parameters, `f32` and `i16` output |
| `solouder-core` | `features/spatial` | `Attenuation`, `Mixer::play_3d` and `update_3d_audio`, the listener, per-voice position, distances and attenuation, speaker positions and per-speaker gains (`lookatRH` basis), the `set_3d_*` source defaults on `Sound`, `Stream`, `Bus` and `Queue`; voices made inaudible by distance are killed, ticked (advanced without resampling, filters or panning) or kept |
| `solouder-core` | `features/filters` | `Filter` and `FilterInstance` traits, `FilterParams` (live values with fades), `BiquadResonantFilter`, `LofiFilter`, `EchoFilter`, `WaveShaperFilter`, `DcRemovalFilter`, `FftFilter`, `BassboostFilter`, `RobotizeFilter`, `FlangerFilter`, `FreeverbFilter` |
| `solouder-core` | `features/realtime` | `split`, `Controller` (plays, voice and global controls, fades and schedules, filters and their parameters, bus settings and `annex_sound`, queued sounds), `Renderer`, `StreamWorker` (decodes streams ahead into per-voice feeds), `VoiceId`, `FilterTarget`, `Event`, `Status`, `RealtimeConfig`; voices and filter instances prepared on the game thread, and ended voices, replaced filters and rejected queue sounds retired to it |
| `solouder-decoders` | `features/decoded_audio` | `decode` (Ogg Vorbis, WAV, FLAC or MPEG audio by the first bytes), `decode_wav` (a bounds-checked RIFF reader with dr_wav's sample conversions), `decode_flac` (a port of dr_flac's decoding path, in `decoded_audio/flac/`), `decode_ogg` (a port of stb_vorbis's pull-mode decoder, in `decoded_audio/vorbis/`), `decode_mp3` (a port of dr_mp3 and minimp3, layers 1 to 3, in `decoded_audio/mp3/`), `DecodeLimits`, `DecodeError` |
| `solouder-decoders` | `features/streaming` | `WavStream` for WAV, FLAC, Ogg Vorbis and MPEG audio (a file, bytes or a file read into memory; per-voice readers: WAV seeks by frame, Ogg finds the target's Vorbis frame, FLAC and MPEG decode up to the target) |
| `solouder-sources` | `prg`, `features/noise`, `features/vic` | SoLoud's generators, one Cargo feature each: `Prg` (WELL512), `Noise` (white, pink, brownish, blueish and custom octave weights) and `Vic` (the VIC-20 sound chip, registers and model live); played through `solouder_core::Synth` |
| `solouder-cpal` | `features/output_lifecycle` | `open`, `Output` (state, info, close), `OutputConfig`, stream negotiation, the panic-guarded callback with sample-format conversion |
| `bevy_solouder` | `decoded_audio`, `output_lifecycle`, `playback` | `SolouderPlugin`, `AudioClip` (decoded or streamed) and `WavLoader` (`WavSettings::stream`), the stream worker (a thread with a device, `HeadlessStreamWorker` headless), `OutputMode` (device or headless), `AudioOutput`, `SoundPlayer`, `PlaybackSettings`, `SoundStatus`, `SoundFinished`/`SoundFailed` |
| `solouder-parity` | — | Scenario parser, test patterns, fixture loading, Rust runner, comparison with C++ output |

Ledger: the Milestone B playback behaviors are `ported`; `Soloud::init`,
`AudioSource::setVolume` and `AudioSource::setLooping` are `in-progress`
(init flags and the rest of the source-level defaults remain), and
`Wav::load` and `Wav::loadMem` are `ported` for all four formats.
Milestone C ports `Soloud::deinit` and the `getBackend*` queries; `init`
stays `in-progress` until its remaining flags are done. Milestone E ports `WavStream::getLength`,
`load`, `loadMem` and `loadToMem`; the `WavStream` lifecycle is
`in-progress`. Unit 6 ports `update3dAudio`, the listener and source
position, distance and attenuation setters, the speaker positions and the
source-level 3D defaults; `play3d`, `set3dListenerParameters` and
`set3dSourceParameters` stay `in-progress` until velocity arrives with
Doppler (unit 7). Milestone F unit 1 ports the `Noise` rows, `Wav::loadFile`,
`WavStream::loadFile` and `loadFileToMem`, and closes the `Wav` and
`WavStream` lifecycles; unit 2 ports the `Vic` rows.

Generators (milestone F): SoLoud's audio sources other than files, buses and
queues implement `solouder_core::Generator`; a `Synth` plays one, alone, in a
queue or through the `Controller`. Each voice's instance is built on the game
thread when the voice is prepared, and reads exactly the frame counts the
reference reads, so sources that restart state on every read (Noise) match
bit for bit. The generic seek rewinds and discards in passes of the scratch
size, as the reference does; a seek back on a generator without `rewind`
fails with `CannotSeekBack`.

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
| `flac_decode` | 8, 12, 16, 20, 24, 28 and 32-bit FLAC (32-bit stereo in every mode, with dr_flac's 33-bit quirks), 1 to 8 channels, every subframe type and stereo decorrelation, both residual codings with escaped partitions, wasted bits, variable block sizes, metadata blocks to skip, variable block sizes that happen to be equal (20-bit side channels need patch 0057); load codes for a bad checksum, short data, an unknown or overstated length, a cut header and a zero rate | 82,210 |
| `stream_queue` | Streams queued with a decoded sound on a stereo queue: a mono stream (its missing channel silent), a 5.1 stream (more channels than the queue), a looping stream played once and dropped; `isCurrentlyPlaying` on streams; seeking the queue forward through a stream; a mono queue of streams at 48 kHz | 35,176 |
| `stream_flac` | Streamed FLAC files loaded from a file, into memory and from bytes: resampled, folded down from 5.1, with variable block sizes, a long file looping from a loop point and wrapping; seeks back, forward across many FLAC frames and past the end; data that ends early, an overstated length, an unknown length ending at once; two files both sides reject | 26,496 |
| `ogg_decode` | Ogg Vorbis with 1 to 8 channels at 8 to 48 kHz, qualities 0 to 10, a file shorter than one Vorbis block, a page with garbled data under a valid checksum (a packet that fails to decode ends the audio, the rest silent) and one with a wrong checksum; load codes for a cut file, headers without audio, a broken setup header and a cut first page | 743,882 |
| `mp3_decode` | MPEG-1, 2 and 2.5 layer 3 at 8 to 48 kHz, mono, joint and plain stereo, constant and variable bit rates, with and without a Xing/LAME frame and an ID3v2 tag; layer 2 mono and stereo; a file cut mid-frame, junk before the first frame and damaged frames; the load code for bytes without audio | 896,832 |
| `stream_ogg` | Streamed Ogg Vorbis from a file, into memory and from bytes: resampled, 5.1 and 7.1 folded down, a long file looping from a loop point, a file shorter than a block, garbled data ending a voice; seeks back, forward within and across Vorbis frames and past the end (patch 0058); load codes for broken files, including one cut inside its first page (patch 0059) | 70,352 |
| `stream_mp3` | The same for MPEG audio: MPEG-1, 2 and 2.5, layer 2, an ID3v2 tag, a long file looping from a loop point, damaged and cut-off frames, junk before the first frame; seeks decode up to the target | 70,352 |
| `spatial_3d` | No attenuation and the inverse, linear and exponential models with min/max distances and rolloff, an unknown model index, the listener's position, `at` and `up` set one by one and together, a listener-relative source, the first frame already attenuated (#95), 3D volume with the voice's volume, fades and pan, a paused voice jumping to its new gains, the 0.01 threshold of `play3d` and the 0.001 threshold of `update3dAudio` (#240), voices killed by either, a voice ticking while inaudible and resuming in place, a ticking voice with a loop point past its end (patch 0060) and with a short last block (patch 0061), a reused slot getting its own flags (patch 0013), setters through groups, stale handles and voices not played in 3D, inaudible voices ranking for the active slots | 20,396 |
| `spatial_speakers` | Default speaker positions for 1, 2, 4, 6 and 8 channels, reading and setting channels past the output's, moved and zero speakers, mono, stereo and 5.1 sources, a 3D voice on a bus, and a bus played in 3D far enough away to tick | 9,300 |
| `stream_wav` | Streamed WAV files loaded from a file, into memory and from bytes, at and away from the output rate, 5.1 and quad float files folded down, A-law; a looping stream with a loop point wrapping several times (the loop-wrap seek and channel stride of patches 0002 and 0004, and patch 0005's looping streams not ending); seeks back, forward and past the end; streams ending, counted, replayed and stopped; two files both sides reject; a stream reloaded under playing voices from a file of another format and from bytes, then failing to reload (patch 0063) | 31,096 |
| `noise` | White, pink, brownish and blueish noise, a type index without a preset, custom octave weights and weights summing to 0 (patch 0062), two voices of one noise (same seed), changed weights reaching only later plays, stopping and counting, speed, rate and seeks forward and back (back fails without `rewind`), a filter, single-instance looping noise, 3D, a queue and a queue seek, and a 48 kHz engine with a 6000-frame buffer (seek passes of another scratch size) | 47,867 |
| `file_loading` | `Wav::loadFile` of a `DiskFile` and of a `MemoryFile` next to `Wav::load` for WAV, FLAC, Ogg Vorbis and MPEG audio (samples compared whole), `WavStream::loadFile` and `loadFileToMem` of each kind, looping Ogg Vorbis streams through each kind of file, seeks, and files that fail to load | 159,084 |
| `vic` | Each oscillator and noise, off below 128 and at 255, registers and the model written between mixes reaching playing voices at their next block, register indices past the last (patch 0064), PAL and NTSC, two voices, speed, rate, seeks forward and back, the suggested lowpass, looping, single instance, 3D, a queue, a 48 kHz engine | 37,345 |

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
| `stream_tests` (core, 8 tests) and `realtime_no_alloc_streams` | Prebuffered streams (looping with a loop point and resampled, played once, and queued with a sound) render the same samples as the offline `Mixer`; a stream waits silently for the worker; a dry feed counts underflow and keeps its place; seeks before and while playing continue from the right frame; ended and missing streams finish and their feeds close; the worker finishes with the controller and renderer; streams the worker has not taken fill `pending_streams`; none of it allocates or frees in the render |
| `routing_tests` (core, 3 tests) | A real-time session with voice and global filters, a filtered bus, a queue, clocked and background plays, fades, filter parameter fades, seeks, a bus filter swap, `annex_sound` and `stop_sound` renders the same samples as the offline `Mixer`, bit for bit; bus and queue plays report their events, and a play through a bus that is not playing fails; queues past `RealtimeConfig::queues` are refused |
| `layout_tests` (core, 5 tests) | Filters whose delays change before their first block, and a bus whose channel setting changed while it plays, render the same samples as the offline `Mixer`; a mixer split after offline use (queue storage, a queued sound, a playing voice) never overflows its retirement list; `Mixer::keep_retired` keeps voices played after it, and `split` trims the surplus voice boxes left by offline overflow; a voice a bus seek plays out reports `Ended` even when a play in the same render takes its slot |
| `realtime_no_alloc` (core) | No allocation or free inside any `Renderer::render`, with a fresh sound per play and the smallest retirement queue: playing, every control, stopping, voices ending, voice stealing past the slot count, all-protected failures, `stop_all`, rendering after the controller is dropped, and voices, a bus and a queue using every filter that allocates lazily (echo, DC removal, FFT, bassboost, flanger), with global and bus filter swaps, seeks, and sounds queued past the queue's limit; and filters first running on a bus at double speed whose channel setting changed, after delays were changed, after a failed replay of the bus and during a speed oscillation from 3 to 1, and with a flanger delay growing to its maximum; and a mixer split while it holds global filters and filtered voices that have not played yet |
| `generator_tests` (core, 7 tests) and `realtime_no_alloc` (sources) | Generator voices read whole blocks and the frames the reference computes, loop wraps rewind and refill the block, a looping voice that cannot rewind ends, seeks discard in passes of the scratch size and seeking back without `rewind` fails, synths queue back to back; noise, looping, seeking and queued synths render through the `Controller` without allocating, bit for bit as an offline `Mixer` |
| `spatial` (core, 16 tests) | The vector math and attenuation models; `play_3d` attenuates the first frame (#95); a reused slot keeps no flags from the old voice (patch 0013); the two inaudible thresholds (#240); killing and ticking inaudible voices; a ticking voice with an empty loop ends its mix (patch 0060) and clears the rest of its last block (patch 0061); `set_3d_*` changes only 3D voices; speaker layouts and bounds; a zero speaker; paused voices jumping to their new volume |
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
| Queued sources | `Queue::play` takes any `AudioSource`, including buses and queues | `Mixer::queue_play` takes a `Sound` or a `Stream` (`Queueable`) | Queuing a bus or a queue mixes nothing useful |
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
| Stream files | `WavStream::loadFile` keeps the caller's `File`: every voice seeks and reads the same cursor, so two voices corrupt each other | `WavStream::from_file` takes a `StreamFile` that each voice opens on its own; paths and bytes behave as `open` and `from_bytes`, other types as a `MemoryFile` | Voices must not share state |
| Reading a file to memory | `Wav::loadFile` and `WavStream::loadFileToMem` read `File::length()` bytes | `decode_reader` reads to the end, up to `DecodeLimits::max_file_bytes` (`FileTooLarge` past it); `WavStream::from_reader` reads to the end | A reader has no length; the cap keeps an endless reader from exhausting memory |
| Generator instances | Created on the audio thread under the engine mutex by `play` | Built when the voice is prepared (`Mixer::play` or `Controller::play`), on the game thread, then moved to the voice | The audio thread never allocates |
| Looping without rewind | A looping voice whose source cannot rewind (`NOT_IMPLEMENTED`) stays alive at its end, rendering silence | The voice stops looping and ends | A voice that plays nothing forever holds a slot; no reference generator reaches it once Vizsn has `rewind` (unit 7) |
| Dropping a generator | `~Noise` (and every source's destructor) stops its voices | Dropping a `Synth` leaves its voices playing; each owns its instance | As for `Sound` |
| Vic model | `setModel` takes an `int`; a value other than PAL or NTSC keeps the clocks, and `getModel` returns it | `VicModel` has the two models | A model that does not exist is not a setting |
| Vic registers | `setRegister` and `getRegister` take any `int` (patch 0064 ignores, and reads 0, past the last) | `set_register` returns `Err(RegisterOutOfRange)` and `register` returns `None` | There are no silent failures |
| Streams in real time | Played like any source; the audio thread reads the file | A `StreamWorker` thread opens and decodes each voice's stream ahead of it into a bounded buffer (`RealtimeConfig::stream_chunks`). A voice waits silently, without advancing, until its audio arrives at the start and after a seek the worker had not anticipated (loop wraps are anticipated); if the worker falls behind, the voice plays silence without advancing, counted in `Status::stream_underflow_frames`. A source that can no longer rewind ends the voice, and so does losing the worker once the audio it decoded has played (new streams then fail with `ControllerError::NoStreamWorker`). Fades, scheduled events and the stream position keep running while a voice waits; a seek counts from the audio actually played | File reads on the audio thread can stall it |
| Unknown files | `Wav::load` and `WavStream` detect Ogg, WAV and FLAC by their first four bytes and try MP3 for anything else | The same; bytes that hold no MPEG audio report `NotRecognized` | — |
| FLAC frame with a bad checksum | dr_flac, built with `DR_FLAC_NO_CRC`, checks neither frame nor header checksums and plays the frame; it stops at a frame it cannot decode | Both checksums are checked. A frame that fails either, or fails to decode, is dropped and its frames are silent (resident and streamed), so the frames after it keep their place, even after a run of damaged frames; the search for the next frame resumes just after the damaged frame's sync code, so a frame starting inside a rejected header is still found (dr_flac searches on from the end of what it read) | A damaged frame is more likely noise than audio |
| FLAC data that ends early | `Wav` keeps the length STREAMINFO declares; the missing frames are silent | Keeps the frames decoded, as for WAV; streams end where the data does in both (patch 0054) | Padding would add a gap to looping sounds |
| FLAC of unknown length | `Wav` rejects it; `WavStream` loads it with a length of 0, so a voice ends at once, silent | The same: `decode_flac` reports `Unsupported`, and a stream voice ends at once | Kept for parity |
| FLAC arithmetic overflow | dr_flac's integer arithmetic wraps on damaged files. For 8 to 24-bit stereo its SSE2 conversion keeps 8 more bits of a wrapped sample than its scalar code, and x64 runs SSE2 on most of each read, so its output there depends on how SoLoud splits reads | The port wraps where dr_flac's scalar code does, in every build (no panic with overflow checks on). Files whose samples overflow are damaged; `bad_overflow_*` is checked by unit tests, not against the C++ | The scalar result does not depend on read sizes |
| FLAC seeking and rewinding | dr_flac seeks with the seek table or a search; `WavStream` uses the generic seek, decoding up to the target | Seeks decode up to the target, as `WavStream` does; a rewind goes back to the first frame on the voice's own file handle (or the bytes), as dr_flac does, so a looping voice plays on when the file is deleted. A rewind that fails ends the voice, offline as through the worker | The same as the reference's generic seek |
| FLAC frames numbered out of order | dr_flac plays frames in file order, whatever their numbers | A frame numbered past the last one leaves silence for the gap, up to the declared length; a frame overlapping audio already decoded gives only its later frames, and nothing if it lies wholly inside | Keeps resident and streamed decoding on the same timeline |
| FLAC metadata | dr_flac skips metadata blocks as declared, rejects the file if an application, comment, cue sheet or picture block is too short to parse, and stops at a block that runs past the end of the file | The same checks, except that a block running past the end of the file rejects the file | A file with no frames to find is reported, not decoded to nothing |
| Damaged MPEG frames | dr_mp3 decodes a damaged frame into stack memory it never initialised and can read what an earlier frame left there | The scratch memory is zeroed when the decoder resynchronises; valid files decode bit for bit, and 26 of 7,790 mutated files differ | Undefined behaviour is not reproduced |
| Damaged Ogg Vorbis | stb_vorbis reads and writes out of bounds, asserts or reads uninitialised arrays on some damaged files | The packet or the file fails instead; valid files decode bit for bit | Undefined behaviour is not reproduced |
| Huge Vorbis setup headers | stb_vorbis allocates what the setup header asks for, up to about 2 GiB per table | Setups needing more than 256 MiB are refused (`Unsupported`) | Unchecked allocation from file data |
| Ogg Vorbis seeking | `stb_vorbis_seek` landed after its target, and a rewind kept the Vorbis frame being read | As patched (0058): seeks land where the generic seek would, found with `stb_vorbis_seek_frame` | Kept for parity |
| MP3 in memory over 2 GiB | dr_mp3 truncates the length to an `int` | Clamped to `i32::MAX` | A truncated length could be negative |
| 32-bit FLAC stereo | dr_flac keeps 33-bit side samples in 32 bits: left/side and right/side come out right, mid/side at high amplitude does not. On x64 a 33-bit read with one bit left in its 64-bit cache line also sets bit 0 to the dropped high bit | The same, quirks included (`s32_stereo_44100` in `flac_decode`) | Kept for parity; 32-bit FLAC is rare. A fix would be a reference patch first |
| Decode limits | Only the `u32` size checks of patch 0012 | `DecodeLimits::max_samples` (default 256 Mi samples); WAV allocation follows the data present, not header claims; FLAC allocation follows the length STREAMINFO declares, within the limit, since FLAC can legitimately compress silence a thousandfold | Untrusted files cannot request huge allocations |
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
| 3D attenuation model | `set3dAttenuation` and `set3dSourceAttenuation` take an `unsigned int`; unknown models attenuate nothing | `Attenuation` enum with the four models; the parity runner maps unknown indices to `Attenuation::None`, as C++ behaves | A model that does not exist is not a setting |
| 3D play options | `play3d` takes velocity, volume, paused and bus; the pan of a 3D voice starts at 0 | `Mixer::play_3d(source, position, PlayOptions)`: no velocity until Doppler (unit 7); `PlayOptions::pan` is ignored and `volume: None` uses the source's volume (#381) | One options type for every play call |
| Speaker positions | `getSpeakerPosition` and `setSpeakerPosition` return `INVALID_PARAMETER` for a channel the output does not have | `speaker_position` returns `Option`, `set_speaker_position` returns `Err(ControlError::NoSuchSpeaker)` | There are no result codes |
| 3D setters | `set3dSource*` change only voices played with `play3d`, through handles and groups | The same: a voice without the `PROCESS_3D` flag is left alone | Kept for parity |
| 3D in real time | `update3dAudio` runs under the engine mutex on whichever thread calls it | Not reachable through the `Controller` yet; voices played in 3D before `split` keep the gains they had | Unit 8 computes the gains on the game thread and sends each voice's results to the audio thread in one batched command |
| Shutdown | `deinit` can free state while the device thread still runs (patch 0010) | The stream is dropped first, which stops the callback; the renderer is handed back (waiting up to half a second for backends that release the callback late) and freed afterwards | Teardown order is enforced by ownership |

Known reference defects fixed on both sides by patches 0018 to 0024 and 0026
to 0057, and 0060 to 0064 (pan ramp leftovers, surround fold-down volumes, no-free-voice
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
whose file is gone crashing, a stream cut off in its data never ending, and a
looping stream whose file is gone never ending, a ticking voice hanging on a
loop without audio, a ticking voice leaving stale samples after its
end, noise weights summing to 0, a stream reloaded under its voices, and Vic
register indices past the last) are
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
  `annex_sound`; `queue_play` (sounds and streams).
- Streams, played and queued: the `StreamWorker` decodes them ahead of the
  audio thread (`stream_tests`).
- Generators (`Synth`), played, seeked, looping and queued
  (`realtime_no_alloc` in `solouder-sources`).

Limits:

| Area | Behavior |
|---|---|
| Mixer only | 3D audio (`play_3d`, `update_3d_audio`, the listener, source and speaker setters), voice groups, voice getters other than those in `Status`, bus wave, FFT and voice counts, queue counts and `is_current`, and filter parameter reads |
| Seek errors | A seek backwards on a bus or queue voice, which they cannot do, is ignored; `CannotSeekBack` is not reported |
| Queued sounds | A sound the renderer cannot queue (queue not playing, or full) is retired and counted in `Status::rejected_queue_sounds`, not reported on its own. Each queued sound holds a retirement credit until it has played and been collected. A controller plays at most `RealtimeConfig::queues` distinct queues (default 64) besides those the mixer already had at `split`; each new queue's list is allocated on its first play |
| Filter sizing | Memory is reserved for a voice's filters at its channels and starting rate, for global filters at the output layout, and for bus filters at the largest layout among the bus's voices this controller requested and has not seen finish (channels, and the fastest speed each was set, faded or oscillated to); a bus played before `split` uses the `Bus`'s current settings. `split` reserves for the filters the mixer already holds, at each voice's current rate or the top of its speed fade. Echo reserves for the delay it was created with, its `DELAY` parameter's maximum (larger values are accepted); the flanger for 0.1 s, its `DELAY` maximum, or its own delay if longer. The render allocates when a filter needs more than that: echo or DC removal first running at a higher rate than reserved (a voice sped up or given a higher sample rate before its first mixed block), an echo delay set above its maximum before its first block, or a flanger whose rate or delay grows past its reserved line at any time |
| Finish reasons | `FinishReason::Ended` covers playing to the end, including during a bus seek, and a scheduled stop coming due; `Stopped` covers stop commands, stopping a bus or a sound's voices, and a play that replaced the voice |

## Not yet covered

- Init flags other than the clipper (`LEFT_HANDED_3D`, ...), device
  reconnection, and device output outside Windows WASAPI (checked only to
  build and skip on Linux).
- 3D audio: Doppler, sound speed and velocities, distance delay, colliders,
  attenuators, `play3dClocked`, `Bus::play3d` and `LEFT_HANDED_3D`
  (milestone E unit 7), and 3D through the `Controller` (unit 8).
- Voice groups, voice getters, bus wave, FFT and voice counts, queue counts,
  and filter parameter reads through the `Controller` (see "Real-time
  coverage").
- WAV ADPCM; chained Ogg files (several logical streams one after another) are untested.
