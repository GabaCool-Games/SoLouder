# Port status

What the Rust port does today, where it deliberately differs from the C++
reference, and how it is verified. Per-behavior status lives in
[parity-ledger.csv](parity-ledger.csv); the plan is in
[PORTING.md](PORTING.md#milestones-and-acceptance-gates).

## Summary

Milestone F (sources and portability), the last of the plan, is done
(2026-10-19, after two independent reviews; see
[PORTING.md](PORTING.md#milestone-f-review), which also lists what stays
open): the generated sources in `solouder-sources` (Sfxr, Speech, Vizsn,
Noise, Monotone, TedSid, Vic, AY and chip dumps, played like sounds and
queued), file readers, visualization, CI jobs for Linux and macOS with the
support matrix below ("Platforms"), and benchmarks and a soak test in
`solouder-bench`. All 56 scenarios render the same samples as the C++
reference on Windows, bit for bit except the 8-bit WAV conversion, and the
same on Linux.

Milestone E (spatial and streaming) is done: WAV, FLAC, Ogg Vorbis and MP3
files decode into PCM or stream, offline and through the real-time
`Controller`, whose `StreamWorker` decodes them ahead of the audio thread;
3D audio places voices offline, through the `Controller` and from Bevy
entities (see [PORTING.md](PORTING.md#milestone-e-review)). Milestone D
(routing and DSP) before it: fades, oscillations,
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

Visualization (milestone F unit 9) is done: the mix's wave, spectrum and
channel volumes (`Mixer::wave`, `fft`, `approximate_volume`,
`set_visualization` and `MixerConfig::visualization` for
`ENABLE_VISUALIZATION`, with reference patches 0067 and 0068), offline,
through the `Controller` (with buses' data, published by the `Renderer`
after each render) and as `bevy_solouder`'s `AudioVisualization` resource.
Every `Soloud::init` flag is now covered.

For real-time use the mixer is split into a `Controller` for the game thread
and a `Renderer` for the audio thread, joined by bounded wait-free queues
(see [ARCHITECTURE.md](ARCHITECTURE.md#ownership-and-the-real-time-boundary)).
`solouder-cpal` runs the renderer on the default output device and
`bevy_solouder` plays `.wav`, `.flac`, `.ogg` and `.mp3` assets from
entities. Those files can also be streamed, offline or through the
`Controller`, whose `StreamWorker` decodes them ahead of the audio thread;
`bevy_solouder` streams assets loaded with `WavSettings::stream`. 3D audio (milestone E units 6 to 8)
places voices with a listener, attenuation models, listener-relative sources
and speaker positions for 1 to 8 channels, with Doppler, the speed of sound,
distance delay, colliders, attenuators and `LEFT_HANDED_3D`, offline and
through the `Controller`, which runs the 3D math on the game thread;
`bevy_solouder` places sounds from `AudioListener` and `SpatialEmitter`
components. Voice groups and the voice, bus voice-count and queue queries
are available on the offline `Mixer` only (see "Real-time coverage" below).

| Crate | Slices / modules | Contents |
|---|---|---|
| `solouder-core` | `contracts` | `Pcm` (validated planar samples from `f32`, `u8` or `i16`), `SAMPLE_GRANULARITY`, `MAX_CHANNELS` |
| `solouder-core` | `dsp` | Resamplers, pan-and-expand for every channel layout, round-off and hard clipping, faders (linear fades and sine oscillation), `fft` (Ooura's split-radix FFT as SoLoud has it), `waveform` (`generateWaveform`) |
| `solouder-core` | `features/playback` | `Mixer`, `MixerConfig`, `Sound`, `Stream` (with the `StreamSource` and `StreamReader` traits), `Bus`, `Queue`, `Playable`, `SoundId`, `Handle`, `PlayOptions`, `FilterTarget`, voice table, active-voice selection, resample-buffer pool, bus mixing (nested bus voices, bus seeking, visualization data), the mix's visualization data (`VisualizationData`, `WAVE_LEN`), queued sounds, voice control and queries, fades and timers, voice groups, sound, bus and global filter slots and their parameters, `f32` and `i16` output |
| `solouder-core` | `features/spatial` | `Attenuation`, the `Collider` and `Attenuator` traits with `SpatialVoice`, `Mixer::play_3d`, `play_3d_clocked`, `bus_play_3d`, `bus_play_3d_clocked` and `update_3d_audio`, the listener (position, orientation, velocity), the speed of sound, per-voice position, velocity, distances, attenuation and Doppler factor, Doppler composed with play speed, distance delay, speaker positions and per-speaker gains (`lookatRH` basis, `lookatLH` with `MixerConfig::left_handed_3d`), the `set_3d_*` source defaults on `Sound`, `Stream`, `Bus` and `Queue`; voices made inaudible by distance are killed, ticked (advanced without resampling, filters or panning) or kept |
| `solouder-core` | `features/filters` | `Filter` and `FilterInstance` traits, `FilterParams` (live values with fades), `BiquadResonantFilter`, `LofiFilter`, `EchoFilter`, `WaveShaperFilter`, `DcRemovalFilter`, `FftFilter`, `BassboostFilter`, `RobotizeFilter`, `FlangerFilter`, `FreeverbFilter` |
| `solouder-core` | `features/realtime` | `split`, `Controller` (plays, voice and global controls, fades and schedules, filters and their parameters, bus settings and `annex_sound`, queued sounds, 3D plays and setters with `update_3d_audio` computed on the game thread and sent in one batch, the mix's and buses' visualization data published through a triple buffer), `Renderer`, `StreamWorker` (decodes streams ahead into per-voice feeds), `VoiceId`, `FilterTarget`, `Event`, `Status`, `RealtimeConfig`; voices and filter instances prepared on the game thread, and ended voices, replaced filters and rejected queue sounds retired to it |
| `solouder-decoders` | `features/decoded_audio` | `decode` (Ogg Vorbis, WAV, FLAC or MPEG audio by the first bytes), `decode_wav` (a bounds-checked RIFF reader with dr_wav's sample conversions), `decode_flac` (a port of dr_flac's decoding path, in `decoded_audio/flac/`), `decode_ogg` (a port of stb_vorbis's pull-mode decoder, in `decoded_audio/vorbis/`), `decode_mp3` (a port of dr_mp3 and minimp3, layers 1 to 3, in `decoded_audio/mp3/`), `DecodeLimits`, `DecodeError` |
| `solouder-decoders` | `features/streaming` | `WavStream` for WAV, FLAC, Ogg Vorbis and MPEG audio (a file, bytes or a file read into memory; per-voice readers: WAV seeks by frame, Ogg finds the target's Vorbis frame, FLAC and MPEG decode up to the target) |
| `solouder-sources` | `prg`, `chipdump`, `features/ay`, `features/monotone`, `features/noise`, `features/sfxr`, `features/speech`, `features/tedsid`, `features/vic`, `features/vizsn` | SoLoud's generators, one Cargo feature each: `Prg` (WELL512), `Noise` (white, pink, brownish, blueish and custom octave weights) `Sfxr` (sfxr sound effects from seeded presets and parameter files), `Monotone` (PC-speaker tracker songs), `TedSid` (Commodore 64 SID and Plus/4 TED register dumps), `Ay` (AY-3-8910 and YM2149F register dumps, one chip or two), `Speech` (English text to speech: rsynth's letter-to-sound rules and Klatt formant synthesiser, voice parameters live), `Vic` (the VIC-20 sound chip, registers and model live) and `Vizsn` (Finnish text to speech, letter by letter, through a small formant synthesiser); played through `solouder_core::Synth` |
| `solouder-cpal` | `features/output_lifecycle` | `open`, `Output` (state, info, close), `OutputConfig`, stream negotiation, the panic-guarded callback with sample-format conversion |
| `bevy_solouder` | `decoded_audio`, `generated_audio` (feature `sources`, on by default), `output_lifecycle`, `playback`, `spatial`, `visualization` | `SolouderPlugin`, `AudioClip` (decoded, streamed, or generated: `AudioClip::generated(Synth)` and `from_generator`) and `WavLoader` (`WavSettings::stream`), `SfxrLoader` (`.sfx`, `.sfs`), `MonotoneLoader` (`.mon`, `MonotoneSettings`) and `ChipDumpLoader` (`.dump`, `TedSid` or `Ay` by chip type), the stream worker (a thread with a device, `HeadlessStreamWorker` headless), `OutputMode` (device or headless), `AudioOutput`, `SoundPlayer`, `PlaybackSettings`, `SoundStatus`, `SoundFinished`/`SoundFailed`; `AudioListener`, `SpatialEmitter`, `AudioVelocity`, `AudioTeleport`, `SpatialAudioSettings`, `ListenerState` (one snapshot from `GlobalTransform` per frame, one 3D update); `AudioVisualization` (the mix's wave, spectrum and volumes each frame) |
| `solouder-parity` | — | Scenario parser, test patterns, fixture loading, Rust runner, comparison with C++ output |

Ledger: the Milestone B playback behaviors are `ported`; `Soloud::init`
was `in-progress` until milestone F unit 9, and
`Wav::load` and `Wav::loadMem` are `ported` for all four formats.
Milestone C ports `Soloud::deinit` and the `getBackend*` queries; `init`
stays `in-progress` until its remaining flags are done. Milestone E ports `WavStream::getLength`,
`load`, `loadMem` and `loadToMem`; the `WavStream` lifecycle is
`in-progress`, and `loadFile` and `loadFileToMem` (SoLoud's custom `File`
interface) are `todo`, carried past the milestone. Unit 6 ports `update3dAudio`, the listener and source
position, distance and attenuation setters, the speaker positions and the
source-level 3D defaults. Unit 7 ports the velocities, the speed of sound,
the Doppler factors, distance delay, colliders, attenuators, `play3dClocked`
and `Bus::play3d`/`play3dClocked`, which completes `play3d`,
`set3dListenerParameters` and `set3dSourceParameters`; every 3D row is
`ported`. Unit 8 makes every 3D call reachable through the `Controller`.
Milestone F unit 1 ports the `Noise` rows, `Wav::loadFile`,
`WavStream::loadFile` and `loadFileToMem`, and closes the `Wav` and
`WavStream` lifecycles; unit 2 ports the `Vic` rows, unit 3 the `Sfxr`
rows, unit 4 the `Monotone` rows, unit 5a the `TedSid` rows,
unit 5b `Ay::lifecycle`, unit 6 the `Speech` rows and unit 7 the
`Vizsn` rows. Unit 8 excludes the four `Openmpt` rows (an external
libopenmpt binding), closes `AudioSource::setVolume` and `setLooping`,
and checks every source-common row against Sound, Stream, Synth (each
generator), Bus and Queue (scenario `source_settings`).
Milestone F unit 9 ports `getWave`, `calcFFT`, `getApproximateVolume` and
`setVisualizationEnable`, and with `ENABLE_VISUALIZATION` makes
`Soloud::init` `ported` (`NO_FPU_REGISTER_CHANGE` excluded: Rust never
changes FPU registers).

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
| `stream_ogg` | Streamed Ogg Vorbis from a file, into memory and from bytes: resampled, 5.1 and 7.1 folded down, a long file looping from a loop point, a file shorter than a block, garbled data ending a voice; seeks back, forward within and across Vorbis frames and past the end (patch 0058); load codes for broken files, including one cut inside its first page (patch 0059); seeks while paused, back and forward across Vorbis frames, resuming where the last seek put the stream (#396, #156) | 78,752 |
| `stream_mp3` | The same for MPEG audio: MPEG-1, 2 and 2.5, layer 2, an ID3v2 tag, a long file looping from a loop point, damaged and cut-off frames, junk before the first frame; seeks decode up to the target | 70,352 |
| `spatial_3d` | No attenuation and the inverse, linear and exponential models with min/max distances and rolloff, an unknown model index, the listener's position, `at` and `up` set one by one and together, a listener-relative source, the first frame already attenuated (#95), 3D volume with the voice's volume, fades and pan, a paused voice jumping to its new gains, the 0.01 threshold of `play3d` and the 0.001 threshold of `update3dAudio` (#240), voices killed by either, a voice ticking while inaudible and resuming in place, a ticking voice with a loop point past its end (patch 0060) and with a short last block (patch 0061), a reused slot getting its own flags (patch 0013), setters through groups, stale handles and voices not played in 3D, inaudible voices ranking for the active slots | 20,396 |
| `spatial_speakers` | Default speaker positions for 1, 2, 4, 6 and 8 channels, reading and setting channels past the output's, moved and zero speakers, mono, stereo and 5.1 sources, a 3D voice on a bus, and a bus played in 3D far enough away to tick | 9,300 |
| `spatial_doppler` | Doppler with velocities from `play3d`, `set3dSourceVelocity`, `set3dSourceParameters`, `set3dListenerVelocity` and `set3dListenerParameters`, projected so that approaching sources play higher (patch 0062); source and per-voice Doppler factors, 0 turning it off; the speed of sound changed, and 0, negative and infinite speeds rejected (patch 0064); Doppler times the play speed, during a speed fade and after a sample rate change (#98); sources and listeners at or past the speed of sound holding the voice until they slow down, including 0 / 0 (#97, patch 0063), and a source past it by more than rounding could hide before dividing (patch 0066); a source at the listener; listener-relative sources; a 6-channel output | 15,800 |
| `spatial_delay` | Distance delay through `play3d` and `play3dClocked` (counted from the listener, patch 0065), listener-relative and paused plays, a changed speed of sound and a 22,050 Hz output; clocked spacing plus distance delay; `Bus::play3d` and `Bus::play3dClocked` with velocities, and through a bus that is not playing (handle 0); delays of 511, 512 and 513 frames around a 512-frame mix and across 256-frame callbacks | 12,092 |
| `visualization` | The mix's wave, FFT and channel volumes: none before recording is enabled, then from the clipped mix (#244) of mixes of 300, 100 and 255 frames (shorter mixes repeat their frames), one frame, and no frames (keeping the last data, patch 0068); turning recording off keeps the last data; global volume past full scale; the `ENABLE_VISUALIZATION` init flag on 8 channels with hard clipping (channel 8 of 8 reads 0, patch 0067) and on mono; buses recording from their first play and enabled before they play, a stopped bus (no data) and one turned off | 31,100 |
| `spatial_custom` | The runner's colliders and attenuators by name: a collider times the attenuation, its user data, set after a voice started (later plays only); an attenuator replacing the model with the voice's distances and rolloff; an attenuator silencing voices at both thresholds, killed or ticking; removing them; `LEFT_HANDED_3D` on stereo and 5.1, rendering like the right-handed scene with every z negated; every voice's gains computed before any applies, so a killed 3D bus's child still runs its collider (`count`); colliders that depend on the voice's velocity, listener-relative flag, distances, rolloff, Doppler factor (`meta`) and handle (`handle`), also in reused slots; an attenuator given the unclamped distance (`raw`); generators (a Vic with distance delay and Doppler, a listener-relative noise with `meta` and `raw`) in 3D | 20,600 |
| `spatial_sources` | Streamed WAV and Ogg files in 3D, clocked and with distance delay, moving with Doppler, ticking while inaudible and sped up, played to their end across stream blocks; a queue in 3D with a collider, an attenuator and Doppler, fed while it moves until it runs dry; a 3D bus with a collider and an attenuator, its 3D children moving, killed once inaudible | 49,814 |
| `stream_wav` | Streamed WAV files loaded from a file, into memory and from bytes, at and away from the output rate, 5.1 and quad float files folded down, A-law; a looping stream with a loop point wrapping several times (the loop-wrap seek and channel stride of patches 0002 and 0004, and patch 0005's looping streams not ending); seeks back, forward and past the end; streams ending, counted, replayed and stopped; two files both sides reject; seeks while paused, from a paused play and after playing, resuming where the last seek put the stream (#396, #156); a stream reloaded under playing voices from a file of another format and from bytes, then failing to reload (patch 0070) | 35,296 |
| `noise` | White, pink, brownish and blueish noise, a type index without a preset, custom octave weights and weights summing to 0 (patch 0069), two voices of one noise (same seed), changed weights reaching only later plays, stopping and counting, speed, rate and seeks forward and back (back fails without `rewind`), a filter, single-instance looping noise, 3D, a queue and a queue seek, and a 48 kHz engine with a 6000-frame buffer (seek passes of another scratch size) | 47,867 |
| `file_loading` | `Wav::loadFile` of a `DiskFile` and of a `MemoryFile` next to `Wav::load` for WAV, FLAC, Ogg Vorbis and MPEG audio (samples compared whole), `WavStream::loadFile` and `loadFileToMem` of each kind, looping Ogg Vorbis streams through each kind of file, seeks, and files that fail to load | 159,084 |
| `vic` | Each oscillator and noise, off below 128 and at 255, registers and the model written between mixes reaching playing voices at their next block, register indices past the last (patch 0071), PAL and NTSC, two voices, speed, rate, seeks forward and back, the suggested lowpass, looping, single instance, 3D, a queue, a 48 kHz engine | 37,345 |
| `sfxr` | All seven presets with four seeds each (two wrapping the seed arithmetic), each played through its decaying tail; an unknown preset; parameter files of versions 100 to 102 from a path, memory and a `DiskFile`, an unknown wave type, a filter byte of 2 (patch 0073), trailing bytes, damaged files that change nothing (patch 0072), version 100 keeping earlier values; settings changing for later plays only; looping inside the source and switched off; speed, rate, seeks forward and back (patch 0074); an echo, a queue and a queue seek; a 48 kHz engine; a looping file with a base frequency of 20 restarting past a period of 0 (patch 0111) | 1,182,256 |
| `monotone` | A song using every effect, including a slide from 0 Hz (patch 0082), vibrato on note 1 (patch 0080) and notes past 99 (patch 0079), on one hardware channel then four, changed while playing; every waveform and one past the last; order and row jumps into a missing pattern; twelve tracks on three hardware channels, thirteen refused (patch 0078); an order without an end marker passing its 256th entry; long titles (patch 0077); damaged songs from a path, memory and a `DiskFile` failing and playing silence (patches 0075, 0076); a play speed below 60 Hz (patch 0081); seeks forward and back; a filter; a 48 kHz engine | 1,212,416 |
| `tedsid` | SID dumps for each model, from the dump's flags (patch 0083), stored and zx7-compressed, with every SID waveform, ring modulation, sync, the test bit, every filter type over a cutoff sweep and an attack from full level (patch 0088); a TED dump with both voices and noise; both chips over two zx7 chunks looping from an odd byte; chip type 6 with a later data offset; an odd length; registers through `getInfo`; storing under a name again stopping its voices; damaged dumps from a path, memory and a `DiskFile` failing (patches 0085 to 0087) and playing the idle chips; seeks forward and back; play speeds; a filter; a 48 kHz engine | 778,240 |
| `ay` | AY and YM dumps, stored and zx7-compressed, with every tone, noise and mixer setting, fixed and enveloped volumes, every envelope shape, register numbers past 15 and a pause longer than a frame (patch 0097); TurboSound and TurboSound Next driving a second chip; the slowest speeds (patch 0095) and a fast tick rate; a dump of several chunks; registers of both chips through `getInfo`; last writes (patch 0096) and tails (patch 0098) played; looping voices restarting their chips (patch 0099) with reads shorter than the buffer (patch 0091) and a loop point; storing under a name again; damaged dumps from a path, memory and a `DiskFile` failing (patches 0092 to 0095); seeks forward and back; play speeds; a filter; a 48 kHz engine | 1,204,224 |
| `speech` | Text through the letter-to-sound rules: numbers (a point, negative, past the int range), spelled words, punctuation, `[phonemes]` with stress marks and bytes that are no phonemes (patches 0103 and 0104), bytes from 0x80 and control bytes, text after a 0, more than 128 elements (patch 0102) and no text; every waveform at several frequencies, speeds and declinations, each played to its end; refused speeds (patch 0100); parameters changed under a playing voice, which keeps its speed (patch 0101); reads of odd sizes; looping with a loop point, seeks, play speeds, auto stop off; storing under a name again; a 44.1 kHz stereo engine resampling, panning and filtering | 784,866 |
| `vizsn` | Finnish text letter by letter: ä and ö as 0x84 and 0x94 or `{` and `|`, capitals, digits, punctuation and bytes from 0x80 as pauses, text after a 0, an empty text and no text (patch 0106), each played to its end and into silence (patch 0107); noise of its own per voice (patch 0105); reads of odd and large sizes (patch 0108); looping at the end of the text (patch 0110) with a loop point, seeks forward and back and rewinds to a new voice's state (patches 0108 and 0109); play speeds; storing under a name again; auto stop off; a 44.1 kHz stereo engine resampling, panning and filtering | 381,130 |
| `source_settings` | Source settings on every generator class: the source's volume taken by plays that give none, looping and loop points, auto stop off, single instance (a new play stopping the other voice), and inaudible ticking and killing among two active voices (kill acts on 3D voices only, as in SoLoud) | 102,400 |
| `generator_queues` | Generators queued (`Queue::play` of a `Synth`): Speech read in the queue's blocks, a text queued twice, and a seek through the queue; Vizsn texts ending and the next starting mid-block; AY dumps of one and two chips one after another, resampled to an 11025 Hz output, the last set to loop and played once (patch 0049); Monotone after a sound, and TedSid twice, never ending, so later items wait | 208,896 |

Samples are compared bit for bit, except that zero and subnormal values all
count as zero, because the C++ build flushes subnormals. Every scenario has
an identical log and identical samples on Windows x86-64; `wav_decode` has
one subnormal float sample that the C++ build writes as zero, and
`wav_decode_u8` declares a tolerance of 1/128 for the 8-bit conversion. A
scenario that renders no samples fails. The Windows output is the golden;
other platforms are in "Platforms" below. Run with
`python scripts/run_parity.py` or
`python scripts/check_all.py --reference --parity`.

## Verified by tests (milestones C to E)

The real-time and device paths have no C++ counterpart in the null-driver
harness, so they are checked by Rust tests instead of scenarios:

| Test | Checks |
|---|---|
| `realtime_tests` (core, 14 tests) | Renders match an offline `Mixer` sample for sample, a stop queued before the first render still reports start and finish, commands for finished voices do nothing, full queues and ID pools reject requests, each render applies a bounded number of commands, ended audio is released by the controller, all-protected tables fail the play, a stolen voice finishes with `Stolen` before its replacement starts, uncollected audio blocks plays instead of being freed in the render, a dropped renderer disconnects the controller, play options set speed and protection from the first frame |
| `stream_tests` (core, 9 tests) and `realtime_no_alloc_streams` | Prebuffered streams (looping with a loop point and resampled, played once, and queued with a sound) render the same samples as the offline `Mixer`; a stream waits silently for the worker; a dry feed counts underflow and keeps its place; seeks before and while playing continue from the right frame; seeks while paused, from a paused play and after playing, resume from the new place sample for sample as on the `Mixer` (#396, #156); ended and missing streams finish and their feeds close; the worker finishes with the controller and renderer; streams the worker has not taken fill `pending_streams`; none of it allocates or frees in the render |
| `routing_tests` (core, 3 tests) | A real-time session with voice and global filters, a filtered bus, a queue, clocked and background plays, fades, filter parameter fades, seeks, a bus filter swap, `annex_sound` and `stop_sound` renders the same samples as the offline `Mixer`, bit for bit; bus and queue plays report their events, and a play through a bus that is not playing fails; queues past `RealtimeConfig::queues` are refused |
| `layout_tests` (core, 5 tests) | Filters whose delays change before their first block, and a bus whose channel setting changed while it plays, render the same samples as the offline `Mixer`; a mixer split after offline use (queue storage, a queued sound, a playing voice) never overflows its retirement list; `Mixer::keep_retired` keeps voices played after it, and `split` trims the surplus voice boxes left by offline overflow; a voice a bus seek plays out reports `Ended` even when a play in the same render takes its slot |
| `realtime_no_alloc` (core) | No allocation or free inside any `Renderer::render`, with a fresh sound per play and the smallest retirement queue: playing, every control, stopping, voices ending, voice stealing past the slot count, all-protected failures, `stop_all`, rendering after the controller is dropped, and voices, a bus and a queue using every filter that allocates lazily (echo, DC removal, FFT, bassboost, flanger), with global and bus filter swaps, seeks, and sounds queued past the queue's limit; and filters first running on a bus at double speed whose channel setting changed, after delays were changed, after a failed replay of the bus and during a speed oscillation from 3 to 1, and with a flanger delay growing to its maximum; and a mixer split while it holds global filters and filtered voices that have not played yet; and 3D: echo, DC removal and flanger voices at 22,050 Hz, 1.25 times speed and approaching (a starting Doppler factor of 1.78), a receding voice, a filtered 3D bus with a 3D voice, clocked 3D plays with distance delay, a play killed as it is placed, batched updates moving the listener and voices and an update that kills, with a collider that allocates (it runs on the game thread); and Doppler rising after the start: a still flanger at its largest delay approached at 200 m/s (a factor of 2.4), paused echo and DC removal voices given that update and then unpaused, a 3D bus whose flanger rises with it and an echo swapped onto it afterwards, a speed set on a 3D voice before an update, and a factor rising a little at every update; and visualization: the mix and two buses recorded over renders of 1 to 4096 frames and an empty one, a recording bus stopping, and recording turned off |
| `generator_tests` (core, 7 tests) and `realtime_no_alloc` (sources) | Generator voices read whole blocks and the frames the reference computes, loop wraps rewind and refill the block, a looping voice that cannot rewind ends, seeks discard in passes of the scratch size and seeking back without `rewind` fails, synths queue back to back; every source's synths, looping, seeking and queued (noise, sfxr, Ay dumps one after another, Speech, Vizsn and TedSid), render through the `Controller` without allocating, bit for bit as an offline `Mixer` |
| `spatial` (core, 29 tests) | The vector math and attenuation models; `play_3d` attenuates the first frame (#95); Doppler follows OpenAL (patch 0062), composes with play speed (#98) and holds the voice at the speed of sound (#97, patches 0063 and 0066, including a factor whose clamp left a rounding-sized divisor); a killed 3D bus stops its voices after their colliders ran; a distance delay past `i32::MAX` frames waits 2^31 frames; the speed of sound rejects NaN and infinity (patch 0064); distance delay counts from the listener (patch 0065); bus plays need a playing bus; colliders and attenuators; left-handed equals right-handed with z negated; a reused slot keeps no flags from the old voice (patch 0013); the two inaudible thresholds (#240); killing and ticking inaudible voices; a ticking voice with an empty loop ends its mix (patch 0060) and clears the rest of its last block (patch 0061); `set_3d_*` changes only 3D voices; speaker layouts and bounds; a zero speaker; paused voices jumping to their new volume |
| `spatial_tests` and `spatial_thread_tests` (core realtime, 8 tests) | 3D plays (plain, clocked, through buses), moving listeners and voices, Doppler, distance delay, speakers, colliders and attenuators render the same samples through the `Controller` as on the `Mixer`, bit for bit, on 1, 2, 4, 6 and 8 channels, at 22,050 to 48,000 Hz, right- and left-handed, including flanger, echo and DC removal voices whose Doppler factor rises after they start (their filters grown on the way); colliders run only on the game thread and see the `VoiceId`; an update computed for a voice that was stopped or ended reaches neither the 3D voice that took its slot nor anything else; voices killed for being inaudible finish with `Stopped` and are forgotten; at most `spatial_batches` updates are in flight; the setters behave as the mixer's; `split` keeps the mixer's listener and speakers; a killed 3D bus's child still runs its collider |
| `visual_tests` (core realtime, 2 tests) and the `visual` and `visualization` unit tests | The `Controller` reads the mix's wave, spectrum and volumes, and two buses' (one recording from its play, one enabled while it plays), exactly as the `Mixer` records them over long, short and empty renders, nothing for a stopped bus, and the last data once recording stops; buses past `RealtimeConfig::visualized_buses` are refused when enabled or played recording; the triple buffer hands the reader the latest copy and keeps it; the init flag and setter; short mixes repeat and empty mixes keep the data |
| `realtime` (parity, 1 test) | Every `spatial_*` scenario, and `visualization` up to the bus it turned off, replayed through the `Controller` and `Renderer` (sources defined apart, streams through the `StreamWorker`) renders the same samples as the mixer's run, bit for bit: in full, except `spatial_3d` up to `global max_active` and `spatial_custom` up to its handle collider (stops pinned by line); a request refused for want of room stops the replay instead of being ignored |
| `callback` unit tests (cpal) | Long callbacks render in parts identical to manual renders, conversion to `i16`, a panic injected inside the render silences the output and hands its payload off the audio thread, conversions clamp to `[-1, 1)` (24-bit 1.0 would otherwise be out of range) and play NaN as silence, dropping the callback returns the renderer |
| `device_lifecycle` (cpal) | Repeated open/play/close, drop while playing, two outputs at once, invalid voice counts and buffer sizes rejected before the device is opened |
| `headless_playback` (Bevy, 8 tests) | Play to the end, stop on despawn, remove and replace, pause and resume, despawn on finish, `.wav` loading through the asset server, a bad file fails the player |
| `generated_clips` (Bevy, 4 tests) | sfxr, Monotone, TedSid (plain and zx7) and Ay files load through the asset server as generated clips and are heard; bad files fail their players; `MonotoneSettings` reach the song (13 channels refused); a code-built `Vic` clip keeps its live registers; a `Synth`'s volume comes with its clip and the player's looping wins |
| `playback_ownership` (Bevy, 8 tests) | Removing `SoundStatus` cannot orphan a voice, a replaced status shows the current state, a stolen sound reports `Stolen` and despawns its entity, starting speed and protection ride in the play request (sample-exact against the mixer with a nearly full queue), unusable settings are ignored and never resent, quiet frames send no commands, a failed output or dropped renderer fails waiting and playing sounds |
| `spatial_audio` (Bevy, 6 tests) | With no listener the default (origin, -Z ahead, +Y up) is Bevy's own: a sound at +X is on the right speaker; turned to face +X, the listener hears +Z on its right (right-handed), and its position counts; emitters follow their parents, a reparenting and a parent's rotation; despawning an emitter stops its sound with `Stopped`; despawning the listener keeps its place; several listeners use the lowest entity index and say so; derived and explicit velocities and a moving listener shift the pitch the right way; a jump past `max_derived_speed` or marked `AudioTeleport`, and motion while time is paused or not advancing, have no Doppler, and the same jumps allowed as motion pass the speed of sound and hold the voice |
| `spatial_edges` (Bevy, 8 tests) and the `snapshot` unit tests (4) | A listener scaled to zero or given a NaN transform keeps its last pose without panicking; an emitter whose transform is not finite waits to start, then starts in 3D; motion stepped every third frame, or at a fixed 64 Hz seen at 144 frames per second, keeps a steady derived speed; a stopped entity reads 0 after `velocity_hold`; a placed sound is placed from its first rendered frame; distance delay starts a sound 34.33 m away 0.1 s late; moving `AudioListener` to another entity switches what is heard; 300 emitters spawned in one frame all start in 3D and follow their entities; still frames send no 3D update, and a moved emitter or listener sends one |
| `visualization` (Bevy, 2 tests) | `AudioVisualization` is off by default, follows the mix's wave, spectrum and volumes once enabled (0 past the output's channels), keeps the last data once disabled, and starts enabled from a headless `MixerConfig::visualization` |
| `plugin_lifecycle` (Bevy) | No output keeps the app running and fails plays, despawn before load never starts a voice, a 5000-sound burst gets one outcome per sound with every ID released, repeated device app teardown, prompt drop mid-playback |

Device tests skip on machines without audio output (as in CI) unless
`SOLOUDER_REQUIRE_DEVICE=1`; only "no device", "device not available",
"host unavailable" and "device busy" count as no output; they pass on a Windows WASAPI device.

## Platforms

Verified 2026-10-19 (Linux parity and device re-run on the merged tree). Windows ran on the development machine; Linux in Docker
(`scripts/docker/linux.Dockerfile`: `rust:1.98`, Debian 13, GCC 14.2,
glibc 2.41, Rust 1.98.1). CI (`.github/workflows/ci.yml`) is configured for
every row but has not run: SoLouder is not pushed anywhere.

| OS | Arch | Host | Builds | Tests | Parity | Device |
|---|---|---|---|---|---|---|
| Windows 10 (MSVC, Rust 1.98) | x86-64 | WASAPI | Verified | Verified: workspace, clippy, 117 C++ regression tests | Verified: the golden; every scenario exact | Verified: `device_lifecycle` and `plugin_lifecycle` with `SOLOUDER_REQUIRE_DEVICE=1` |
| Linux (Debian 13, container) | x86-64 | ALSA, through its PulseAudio plugin | Verified | Verified: workspace, clippy, 117 C++ regression tests with GCC 14.2 | Verified: Rust exact against the Windows golden; the GCC reference within the `gcc` profile (below) | Partly: on a PulseAudio null sink, `device_lifecycle` passes; `plugin_lifecycle` fails in some runs on a stream that never starts (see below); no real hardware |
| Linux | arm64 | ALSA | Not run | Not run | Not run | Not run |
| macOS (CI `macos-latest`) | arm64 | CoreAudio | CI configured | CI configured: Rust tests; C++ regression tests non-blocking until they have passed once | CI configured against the Windows golden, non-blocking until it has run | Unverified |
| macOS | x86-64 | CoreAudio | Not run | Not run | Not run | Unverified |
| Miri (nightly 2026-10-12, Linux container) | x86-64 | none | Verified | Verified, no undefined behavior: `dsp::` (16 tests), `contracts::` (3), `playback::visualization` (3), `realtime::visual` (3), `realtime::tests` (14, 13 minutes) and `realtime::routing_tests` (3); in `solouder-sources` (fixtures read without isolation), 41 of 59 unit tests (chip dumps, zx7, the WELL512 generator, AY, Monotone, noise, Sfxr, TedSid, Vic, Vizsn), not Speech or the tests that render or fuzz whole files | n/a | n/a |

Cross-platform parity, measured with `run_parity.py --cpp-dir` (compare
against another machine's runner output) and `--profile` (per-scenario
tolerances from `tests/scenarios/tolerances.txt`):

- **Rust on Linux against the Windows C++ output:** all 56 scenarios match bit
  for bit (the `cross` profile declares nothing). Rust's math follows the MSVC
  build on both platforms, and nothing it calls through libm differed on
  glibc in these scenarios.
- **The GCC reference against the Windows one** (and so against Rust): seven
  scenarios differ, logs identical, from three lines of the reference. Each
  cause was confirmed by changing that line in a GCC build: the scenarios it
  explains then match the Windows output exactly.

  | Scenario | Samples differing | Max abs | RMS | Declared (`gcc`) | Cause |
  |---|---|---|---|---|---|
  | `filters_spectral` | 2,321 of 32,577 | 3.9e-7 | 1.6e-8 | 1e-6 | FFT filter |
  | `spatial_sources` | 25,624 of 49,814 | 6.0e-8 | 7.8e-10 | 1e-6 | Ogg |
  | `file_loading` | 67,530 of 159,084 | 3.3e-7 | 3.8e-8 | 1e-6 | Ogg |
  | `stream_ogg` | 65,639 of 78,752 | 1.4e-6 | 3.4e-8 | 1e-5 | Ogg |
  | `ogg_decode` | 623,804 of 743,882 | 1.2e-4 | 7.3e-7 | 5e-4 | Ogg |
  | `speech` | 10 of 784,866 | 1.8e-5 | 6.3e-8 | 3.1e-5 | Speech |
  | `generator_queues` | 2 of 208,896 | 1.8e-5 | 5.5e-8 | 3.1e-5 | Speech |

  - **Ogg:** stb_vorbis defines its own `M_PI`, a `float` literal, only
    `#ifndef M_PI`. MSVC's `math.h` defines none without `_USE_MATH_DEFINES`;
    glibc's defines it as a `double`. So the GCC build computes the IMDCT
    twiddle factors and the window from a different pi, and every Ogg decode
    differs in the last bits (Rust uses stb's `float` pi, as MSVC does).
    `ogg_decode` exceeds 1e-6 because its fixtures are long and loud.
  - **Speech:** `resonator.cpp` computes the Klatt resonators' coefficients
    with `exp(arg)` and `cos(arg)` of a `float`, which resolve to libstdc++'s
    `float` overloads (`expf`, `cosf`) where MSVC widens to `double`. The
    coefficients differ in the last bit; Klatt then truncates each sample to
    16 bits, so now and then a sample lands one 16-bit step away (1/32768
    times the voice's gain, here 1.8e-5) while its neighbours match. The
    declared 3.1e-5 is one step at full gain. `generator_queues` queues the
    same speech.
  - **FFT filter:** `cos(pha)` and `sin(pha)` of a `float` resolve to the
    `float` overloads (GCC fuses them into `sincosf`). Its `atan2` of floats
    also becomes `atan2f`, but changes no sample here.

  These differences belong to the GCC build of the reference; they are not
  defects, and Rust follows the Windows build that a shipping game uses. A
  reference patch could make the GCC build match MSVC (stb's pi
  unconditionally, `double` arguments in the resonator and the FFT filter),
  at the cost of changing the Linux library's output; not done.
- The WAV fixture generator now rounds its float sine to `f32` before writing
  64-bit files: glibc's `sin` differs from the Windows CRT's in the last bit
  of one sample, which made `gen_wav_fixtures.py --check` fail on Linux.

Device output on Linux (2026-10-19, Docker, PulseAudio's null sink through
ALSA's `pulse` plugin, as the CI `linux-device` job sets it up): cpal reports
`snd_pcm_avail_delay failed: I/O error (5)` about once per stream, as an
unclassified backend error (cpal turns only `EPIPE` into an underrun), while
the stream keeps rendering. The output now sorts the error callback's
kinds (`features/output_lifecycle/health.rs`): underruns, device changes and
a denied real-time priority are ignored; unclassified backend errors and a
busy device are counted (`Output::watched_error_count`, and
`AudioOutput::watched_errors` in Bevy) and only watched; a vanished device
or invalidated stream is `Disconnected`; anything else is `Failed`. A
watched stream turns `Failed` only when its callbacks stop: none for 500 ms,
or four callbacks' length if longer, since the last one seen. Before this
the first such error failed the output and Bevy shut playback down although
the device kept playing.

Results: `device_lifecycle` passes (6 of 6, in 8 runs of 8).
`plugin_lifecycle`, its tests run one at a time, passed in 3 of 8 runs in
the final measurement (12 of 13 in an earlier session the same day); in
the other runs, and in every run with its tests in parallel, one of the
device apps opens a stream that reports the error repeatedly (19 times in
one captured failure) and never calls back, which the output reports as
`Failed` after 500 ms, as it should, so that app's sounds fail with
`OutputUnavailable`. That stream is the pulse plugin's (cpal recovers only from
`EPIPE`); whether real ALSA hardware does either is unknown. The CI device
job therefore blocks on `device_lifecycle` and runs `plugin_lifecycle` one
test at a time without failing the workflow.

## Performance

Measured 2026-10-18 with `python scripts/run_bench.py` (release builds,
2 seconds of output per row after a 0.25 s warmup, one run): AMD Ryzen 9
9950X3D (16 cores, 32 threads), 32 GB, Windows 10 IoT Enterprise LTSC 21H2
(build 19045), Rust 1.98.1, MSVC 19.44 (`/O2`, the reference's
`SOLOUDER_SSE_INTRINSICS` on). Nothing else was running.

Each case builds a stereo 48 kHz mixer (buffer size at least 2048, as
`solouder-cpal` sets it), starts its voices through the `Controller`, and
times `Renderer::render` of one buffer; `reference_bench` builds the same
case on the C++ reference with the null driver and times `Soloud::mix`. The
tones are 2-second looping 44.1 kHz sines, so every voice is resampled;
voices have volume 0.05 and spread pans, and all are active. `buses` is three
levels (1, 4 and 16 buses) with 4 tones on each leaf; `filters` is 30 tones,
three through each of the ten filters at their default settings, plus a
global biquad; `filter_tails` (added 2026-10-19, measured that day) is 8 buses,
each through a biquad and a freeverb, each fed one 0.1 s tone, timed after
30 s of decay, when every filter's state is subnormal; `streams_*` are 16 looping voices of one stereo 44.1 kHz file
in memory; `spatial` is 256 3D tones that all move, with a turning listener
and `update_3d_audio` before every render (untimed in both). The `sources_*`
rows (`--features sources`, which `run_bench.py` builds; measured the same
day, a separate run of 128 and 512 frames) are 16 looping voices of one
generated source each, as in `crates/solouder-bench/src/cases_sources.rs`:
pink noise, a Vic with three squares, the seven Sfxr presets, Monotone and
TedSid playing the test fixtures (`monotone/demo.mon`, a square wave, 4
channels; `chipdump/sidted.zx7.dump`), an AY dump (`chipdump/ay_ay.dump`),
Speech and Vizsn reading a sentence. The budget is the buffer's length;
times are microseconds.

| Case | 128 frames (2,667 µs): Rust mean / max | C++ mean / max | 512 frames (10,667 µs): Rust mean / max | C++ mean / max | Rust / C++ (mean, 512) |
|---|---|---|---|---|---|
| `voices16_point` | 2.5 / 3.8 | 1.5 / 16.5 | 8.6 / 10.1 | 4.2 / 5.3 | 2.05 |
| `voices16_linear` | 3.4 / 4.7 | 1.7 / 2.8 | 11.9 / 12.2 | 6.3 / 6.9 | 1.89 |
| `voices16_catmullrom` | 4.9 / 5.6 | 3.6 / 4.7 | 17.3 / 17.6 | 13.4 / 16.2 | 1.29 |
| `voices64_point` | 10.8 / 14.8 | 4.3 / 6.2 | 31.5 / 34.7 | 13.6 / 14.3 | 2.32 |
| `voices64_linear` | 12.9 / 16.6 | 6.2 / 8.6 | 45.7 / 46.5 | 21.4 / 24.7 | 2.14 |
| `voices64_catmullrom` | 18.9 / 21.5 | 13.6 / 16.8 | 67.0 / 69.4 | 50.2 / 55.2 | 1.33 |
| `voices256_point` | 43.3 / 54.4 | 17.3 / 25.8 | 141 / 149 | 53.2 / 55.7 | 2.65 |
| `voices256_linear` | 52.6 / 67.8 | 24.9 / 42.1 | 181 / 189 | 84.2 / 89.9 | 2.15 |
| `voices256_catmullrom` | 75.3 / 87.3 | 54.5 / 62.7 | 268 / 277 | 200 / 207 | 1.34 |
| `voices1023_point` | 175 / 199 | 75.8 / 112 | 573 / 611 | 218 / 237 | 2.63 |
| `voices1023_linear` | 208 / 230 | 106 / 138 | 732 / 750 | 341 / 366 | 2.15 |
| `voices1023_catmullrom` | 303 / 326 | 225 / 277 | 1,140 / 1,661 | 812 / 836 | 1.40 |
| `buses` | 24.6 / 134 | 9.8 / 45.5 | 72.5 / 113 | 38.2 / 44.2 | 1.90 |
| `filters` | 25.9 / 230 | 23.2 / 102 | 97.8 / 139 | 91.5 / 102 | 1.07 |
| `filter_tails` | 20.7 / 86.4 | 21.8 / 94.1 | 79.7 / 89.0 | 87.5 / 100 | 0.91 |
| `streams_wav` | 8.9 / 46.6 | 5.0 / 19.4 | 29.4 / 81.2 | 19.4 / 27.0 | 1.52 |
| `streams_flac` | 9.0 / 34.0 | 17.5 / 171 | 29.4 / 70.4 | 69.1 / 182 | 0.43 |
| `streams_ogg` | 9.1 / 24.3 | 23.5 / 321 | 31.5 / 116 | 91.9 / 312 | 0.34 |
| `streams_mp3` | 8.5 / 34.8 | 21.2 / 195 | 29.0 / 59.9 | 83.2 / 201 | 0.35 |
| `spatial` | 91.6 / 129 | 54.7 / 66.0 | 216 / 244 | 117 / 132 | 1.85 |
| `sources_noise` | 16.7 / 73.7 | 26.4 / 218 | 64.1 / 80.8 | 104 / 124 | 0.62 |
| `sources_vic` | 4.9 / 14.2 | 3.7 / 13.6 | 17.5 / 19.7 | 13.9 / 16.0 | 1.26 |
| `sources_sfxr` | 85.4 / 367 | 57.1 / 264 | 339 / 387 | 227 / 269 | 1.50 |
| `sources_monotone` | 78.4 / 389 | 17.2 / 73.9 | 307 / 388 | 67.6 / 77.7 | 4.54 |
| `sources_tedsid` | 238 / 445 | 246 / 577 | 1,005 / 1,151 | 1,013 / 1,233 | 0.99 |
| `sources_ay` | 57.0 / 3,263 | 61.0 / 3,554 | 225 / 3,333 | 243 / 3,562 | 0.93 |
| `sources_speech` | 15.9 / 788 | 8.7 / 411 | 61.6 / 797 | 34.1 / 416 | 1.81 |
| `sources_vizsn` | 17.0 / 548 | 4.9 / 125 | 65.6 / 555 | 18.7 / 130 | 3.51 |

`run_bench.py` also writes 256, 480 and 1024 frames (`build/bench/bench.md`,
with the 99th percentiles). What they show:

- **Against the budget:** the slowest render in any mixing row is 15.6% of its
  budget (`voices1023_catmullrom` at 512 frames, 1,661 µs; 12.2% at 128
  frames). Every other Rust mixing row stays under 9%; a 128-frame callback with
  1,023 linear voices takes at most 230 µs of its 2,667 µs. Of the
  generated sources, `sources_ay` overruns the 128-frame budget in both
  (Rust 3,263 µs, C++ 3,554 µs at most, 31% of 512 frames' budget): the
  chip player renders a frame of about 4,000 samples at once, and the 16
  voices, started together, all render theirs in the same callback. The
  other generator rows peak at 30% in Rust (Speech at 128 frames, 788 µs;
  C++ 411 µs) and 11% or less at 512 frames (TedSid, 1,151 µs).
- **Against C++:** the mixing path is 1.3 to 2.7 times slower in Rust:
  about 2.1 to 2.7 times for point and linear resampling, 1.3 to 1.4 for
  Catmull-Rom (where the interpolation itself dominates), about 1.1 for the
  filters and 1.7 to 2 for buses and 3D. The C++ build uses SSE intrinsics
  for clipping and for panning mono and stereo voices onto stereo output;
  the Rust port computes the same values in scalar code, bit exact, and has
  not been profiled or tuned. Not a budget problem at these sizes, but the
  largest gap left.
- **Generators:** noise is faster in Rust (0.6), the chip emulations
  (TedSid, AY) take the same time, Vic and Sfxr are 1.3 to 1.5 times slower,
  Speech 1.8, Vizsn 3.5 and Monotone 4.5 times. None has been profiled;
  Speech and Vizsn, like AY, synthesize in blocks, hence their maxima. (The
  Vizsn row was re-measured on 2026-10-19 after the milestone F review: the
  C++ case had passed its text as UTF-8, `C2 84` for each ä, where Rust
  passes CP437 `0x84`; with the same bytes the numbers barely moved.)
- **Streams:** Rust renders FLAC, Ogg and MP3 streams 2 to 3 times faster
  (WAV, cheap to decode, 1.4 to 1.8 times slower, like other voices), with far smaller
  worst cases (FLAC, Ogg and MP3 maxima of 170 to 320 µs in C++ against 24
  to 116 µs), because the stream worker decodes on its own thread and the
  render only copies decoded audio; the C++ reference decodes inside `mix`.
  The Rust stream rows render at the output's pace (as behind a device) and
  had no underflow; they do not compare the same work, and are cold-cache
  numbers where the others run back to back.
- **Subnormals:** the C++ build flushes subnormals to zero (FTZ and DAZ)
  and Rust cannot, so a filter's decaying tail (feedback state shrinking
  towards zero after its input stops) goes through subnormal arithmetic
  in Rust only. `filter_tails` times exactly that (its unit test checks
  that the timed output is mostly subnormal): on this AMD Zen 5 CPU it
  costs nothing (0.91 of the C++ mean). Some CPUs, older Intel ones in
  particular, take a microcode assist of about a hundred cycles per
  subnormal operation; there this row would show the cost, and the remedy
  would be flushing tiny filter state to zero in Rust. Not measured on
  such a CPU.
- **Block spikes:** with buffers under 512 frames, buses and the FFT filter
  produce their output in 512-frame blocks, so every few renders costs much
  more than the mean (`buses` at 128 frames: mean 24.6, max 134). C++ shows
  the same pattern.

Both benchmarks also build and run in the Linux container (GCC 14.2 for the
reference); a short run there (0.2 s per row, 256 frames, under Docker's
virtualization, not recorded as results) showed Rust at 1.1 to 1.85 times
the C++ mean, a smaller gap than with MSVC.

## Soak test

`crates/solouder-bench/tests/soak.rs` runs a `Controller` on the test thread,
a `Renderer` on an audio thread (random buffers of 128 to 1024 frames at four
times real time) and the `StreamWorker` on its own thread, with a seeded
random mix of requests: sounds, filtered sounds, streams of every codec,
the generated sources (with `--features sources`, as the nightly CI soak
runs; streams in their place otherwise), bursts of 4 to 32 plays into a bus, 3D plays with moving sources and listener
updates, floods of 600 plays (past the 1024 voice slots, so voices are
stolen, and past the 1024-command queue, so requests are refused), stops,
volume, pan, fades, pauses, seeks, speed changes (not on filtered voices,
see "Filter sizing"), scheduled stops, global volume and `stop_all`, at
`max_active` 48. A global allocator counts allocations made on the audio
thread inside `render` only.

It fails on any panic, any allocation or free in a render, any sample that is
not finite or outside [-1, 1], a play without exactly one final event (and a
`Started` before any `Finished`), an ID handed out twice while live, a voice
ID still in use at the end, or `retired_overflow` above 0. Every half second
a probe stops everything and plays 47 looping tones into a bus at once: they
must all start within a few renders, `Status` must show all 48 voices (bus
included) active and the mix audible (#351: plays into a bus starting late;
#163: valid, non-zero-volume voices that are silent); 16 more must leave
exactly 48 active (the active-voice limit, the cause found upstream for #351).
#274 (intelligibility with many voices) has no reproduction upstream; the
soak checks only that many-voice mixes stay finite, bounded and fully mixed
up to the limit.

`check_all` runs it for 3 seconds (debug, 12,000 to 15,000 plays and 6
probes);
`SOLOUDER_SOAK_SECONDS` and `SOLOUDER_SOAK_SEED` set the length and seed, and
CI's nightly `soak` job runs 600 seconds in release. On the machine above,
600 seconds (release, seed 20261018) passed: 208,731 renders (2,084 s of
audio), 2,874,430 plays (2,812,012 started, 62,418 failed for a bus that was
not playing; finished: 170,373 ended, 1,853,592 stopped, 788,047 stolen),
95,910 refused requests, 1,119 probes, the slowest probe burst started within
2 renders, no allocation on the audio thread, every ID released, no
retirement overflow. Streams underflowed 18,944 frames in total, which is
expected when the audio thread renders at four times real time.

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
| Math functions | SoLoud's sources include `math.h`, not `cmath`, so on MSVC `sin`, `cos`, `atan2` and `sqrt` of a `float` run in double precision; with GCC's libstdc++ they resolve to the `float` versions | Follows the MSVC build: the value is widened to `f64`, the function runs, and the result is rounded to `f32` | The Windows reference is the one the scenarios compare against. A Linux GCC build of the reference differs in seven scenarios, by at most 1.2e-4, from stb_vorbis's `M_PI`, the speech resonator's `expf`/`cosf` and the FFT filter's `sincosf` (see "Platforms"); Rust renders the Windows output on Linux too |
| Robotize waveform | `setParams` takes the waveform as an `int` | Also an `i32`, with the waveforms as `dsp::waveform` constants; indices without a waveform act as square, as in C++ | Keeps the live `WAVE` parameter and the setting in step |
| FFT filter buffers | Sized for the channel count of the first call; a later call with more channels writes past them | Grow to the larger count | Memory safety; unreachable from the mixer, which keeps a voice's channel count |
| Flanger line allocation | A failed `new` leaves the audio unfiltered; a line of 2^32 samples or more wraps its size | `try_reserve_exact`; on failure the audio is left unfiltered | Same outcome without the wrap |
| Sound identity | A copied `AudioSource` keeps its ID, so stopping either copy stops both | A cloned `Sound` keeps its `SoundId` the same way; `Sound::new` gives a new one | Same behavior; cloning is how Rust shares a sound |
| Seek result | `Soloud::seek` returns an error code: `NOT_IMPLEMENTED` when a bus would seek back, otherwise `SO_NO_ERROR` | `Mixer::seek` returns `Err(ControlError::CannotSeekBack)` or `Ok(())` | There are no result codes |
| Bus play volume | `Bus::play` and `Bus::playClocked` default the volume to 1, while `Soloud::play` defaults to the source's (#381) | `Mixer::bus_play` takes `PlayOptions`, whose `volume: None` uses the source's volume everywhere; the parity runner passes 1 to match | One default for every play call |
| Bus settings | `Bus::setFilter`, `setResampler` and `setVisualizationEnable` change the bus and its playing instance | `Bus` setters affect later plays; `Mixer::set_bus_filter`, `set_bus_resampler` and `set_bus_visualization` change both | A `Bus`, like a `Sound`, holds no pointer to its voices |
| Bus instances | A bus has one instance: playing it again, on any engine, stops the previous one | Playing a bus again stops its voice on that mixer only | A bus does not know the mixers playing it (as for `Sound`) |
| Bus wave data | `getWave` and `calcFFT` return the bus's last copy once its voice has stopped | `Mixer::bus_wave` and `bus_fft` return `None` | A stale copy looks like live data |
| Visualization in real time | `getWave`, `calcFFT` and `getApproximateVolume` copy the data under the engine mutex | The `Renderer` copies the mix's data, and each watched bus's, into a triple buffer of atomics after every render that records; the `Controller` reads the latest copy and computes the FFT on its own thread | Neither thread waits; a bus's data is published only once this controller enabled it (`RealtimeConfig::visualized_buses` slots) |
| FPU control word | `Soloud::init` sets the x87/SSE control word to flush denormals unless given `NO_FPU_REGISTER_CHANGE` | Never changes FPU registers; the flag has no counterpart | Changing the caller's floating-point environment is a side effect; the parity comparison counts subnormals as zero |
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
| Looping without rewind | A looping voice whose source cannot rewind (`NOT_IMPLEMENTED`) stays alive at its end, rendering silence | The voice stops looping and ends | A voice that plays nothing forever holds a slot; no reference generator reaches it since Vizsn has `rewind` (patch 0109) |
| Dropping a generator | `~Noise` (and every source's destructor) stops its voices | Dropping a `Synth` leaves its voices playing; each owns its instance | As for `Sound` |
| Vic model | `setModel` takes an `int`; a value other than PAL or NTSC keeps the clocks, and `getModel` returns it | `VicModel` has the two models | A model that does not exist is not a setting |
| Vic registers | `setRegister` and `getRegister` take any `int` (patch 0071 ignores, and reads 0, past the last) | `set_register` returns `Err(RegisterOutOfRange)` and `register` returns `None` | There are no silent failures |
| Sfxr settings | `SfxrParams` is a public member; `loadPreset` takes an `int` and `loadParamsMem` rejects empty memory with `INVALID_PARAMETER` | `Sfxr` guards its settings with a mutex (`params`, `set_params`, loaders taking `&self`), read when a voice is prepared on the game thread; `SfxrPreset` names the presets; loaders return `SfxrError` (an empty slice is an unknown version 0); `load_params_reader` reads at most 128 bytes | Shared sounds change safely; the audio thread never locks |
| Monotone songs | `load*` replace the song of an existing `Monotone`, stopping its voices; a failed load leaves it empty; `setParams` takes an `int` waveform | `Monotone::open`, `from_bytes` and `from_reader` build a new one (`Monotone::new` is the empty song); `set_params` takes `&self` and a `MonotoneWaveform` (`set_params_raw` an `int`), and voices read the channels and waveform every block, each on its own as in SoLoud; `MonotoneError` says why a song was refused | Songs are immutable and shared by their voices |
| TedSid dumps | `load*` replace the dump of an existing `TedSid` (stopping its voices since patch 0084); a failed load leaves no dump | `TedSid::open`, `from_bytes` and `from_reader` build a new one (`TedSid::new` has no dump); `ChipDumpError` says why a dump was refused; dumps are read up to 80 MiB | Dumps are immutable and shared by their voices |
| Ay dumps | `load*` replace the dump of an existing `Ay` (stopping its voices since patch 0092); a failed load leaves no dump | `Ay::open`, `from_bytes` and `from_reader` build a new one (`Ay::new` has no dump); speeds the player cannot render fail with `ChipDumpError::UnsupportedSpeed` | Dumps are immutable and shared by their voices |
| Speech text and voice | `setText` replaces the text of an existing `Speech`, stopping its voices; `setParams` takes any speed and an `int` waveform, and its waveform defaults to a triangle | `Speech::new(text)` builds a new one (bytes up to the first 0; `Speech::default` has no text); `set_params(SpeechParams)` takes `&self`, refuses bad speeds with `InvalidSpeechSpeed` (patch 0100) and `KlattWaveform` names the seven waveforms; `SpeechParams::default` is the constructor's voice (square) | Texts are immutable and shared by their voices; frequency, declination and waveform reach playing voices at their next block, each on its own as SoLoud's fields do (a block can see some new and some old), the speed only new voices (patch 0101) |
| Vizsn text | `setText` replaces the text of an existing `Vizsn`, stopping its voices; a `Vizsn` without a text cannot play (patch 0106 ends its voices at once) | `Vizsn::new(text)` builds a new one (bytes up to the first 0); `Vizsn::default` has no text; `Vizsn::samples` is the text's length | Texts are immutable and shared by their voices |
| Generator files in Bevy | Each source has its own `load`/`loadMem` | One loader per file kind; `.dump` files play through `TedSid` or, for AY chip types, `Ay`; Monotone's hardware channels and waveform are load settings | Bevy picks loaders by extension, and both chip players read the same `CHIPTUNE` format |
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
| 3D play options | `play3d` takes velocity, volume, paused and bus; `play3dClocked` always starts the voice; the pan of a 3D voice starts at 0 | `Mixer::play_3d(source, position, velocity, PlayOptions)` and the clocked and bus forms; `PlayOptions::paused` is honored by `play_3d_clocked`, `PlayOptions::pan` is ignored and `volume: None` uses the source's volume (#381) | One options type for every play call |
| Speed of sound | `set3dSoundSpeed` returns `INVALID_PARAMETER` for 0 or less, and (patch 0064) NaN or infinity | `set_3d_sound_speed` returns `Err(ControlError::InvalidSoundSpeed)` | There are no result codes |
| Colliders and attenuators | `AudioCollider::collide(Soloud*, AudioSourceInstance3dData*, int)` and `AudioAttenuator`, raw pointers the application keeps alive | `Arc<dyn Collider>` and `Arc<dyn Attenuator>` (`Send + Sync`); the collider gets a `SpatialVoice` (handle, position, velocity, distances, rolloff, Doppler factor, listener-relative) | No engine pointer or lifetime contract; the same values for the math |
| Coordinate handedness | `LEFT_HANDED_3D` is an init flag | `MixerConfig::left_handed_3d`; the device output and the Bevy plugin use the default, right-handed, which is Bevy's convention (-Z forward, +Y up), so Bevy positions and directions pass unchanged | Kept as SoLoud computes it (see PORTING.md, "Behavior decisions to record") |
| Speaker positions | `getSpeakerPosition` and `setSpeakerPosition` return `INVALID_PARAMETER` for a channel the output does not have | `speaker_position` returns `Option`, `set_speaker_position` returns `Err(ControlError::NoSuchSpeaker)` | There are no result codes |
| 3D setters | `set3dSource*` change only voices played with `play3d`, through handles and groups | The same: a voice without the `PROCESS_3D` flag is left alone | Kept for parity |
| 3D in real time | `update3dAudio` runs on whichever thread calls it, computing without the engine mutex and applying under it to whatever voice is in each slot (see PORTING.md, "Still open in the reference") | The `Controller` keeps its own copy of the listener, speakers, speed of sound and each 3D voice's settings, computes every gain on the game thread, and sends them in one preallocated batch; the renderer applies each to the `VoiceId` it was computed for. Colliders see `Handle::INVALID` and `SpatialVoice::voice`. The 3D setters return nothing and ignore unknown IDs; the speed of sound and speakers return `ControllerError`. Voices played in 3D on the `Mixer` before `split` keep their gains and are not updated | Colliders never run on the audio thread, and a result cannot reach a voice that took another's slot |
| Bevy 3D defaults | — | `SpatialEmitter` attenuates by inverse distance by default (SoLoud: none); units are metres and the speed of sound 343.3 m/s; one listener, the lowest entity index of several (`ListenerState`); with none the last pose is kept at rest; velocities are derived from motion over virtual time unless `AudioVelocity` is set (the distance moved over the time since the position last changed, held for `SpatialAudioSettings::velocity_hold`, 0.1 s, while it does not change), and are 0 for a zero or paused time step, an `AudioTeleport` or a speed past `SpatialAudioSettings::max_derived_speed` (200 m/s); `PlaybackSettings::pan` is ignored for 3D sounds; a degenerate listener transform keeps the last pose, and an emitter whose transform is not finite waits to start; a 3D update is sent only in frames where the listener, the speed of sound or an emitter changed | Usable defaults for a game; deterministic listener choice; no Doppler spikes from frame hitches or respawns |
| Shutdown | `deinit` can free state while the device thread still runs (patch 0010) | The stream is dropped first, which stops the callback; the renderer is handed back (waiting up to half a second for backends that release the callback late) and freed afterwards | Teardown order is enforced by ownership |

Known reference defects fixed on both sides by patches 0018 to 0024 and 0026
to 0057 and 0060 to 0114 (pan ramp leftovers, surround fold-down volumes, no-free-voice
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
end, Doppler shifting the wrong way, an infinite or NaN Doppler speed
poisoning the voice's position, a source past the speed of sound playing
millions of times too fast when rounding left its divisor above 0, NaN and
infinite speeds of sound, `play3dClocked` measuring distance delay from
the origin, the engine's volume read past its channels, and a mix of no
frames crashing with visualization on, noise weights summing to 0, a stream reloaded under its voices, Vic
register indices past the last, short sfxr files loading partly, the sfxr
filter flag read into a `bool`, an ended sfxr sound read past its
envelope, Monotone songs uninitialised before a load and left half loaded after a
failed one, short Monotone files loading, titles past their 40 bytes,
more than 12 tracks or hardware channels, note, order and pattern
indices out of range, the note table's unset entry 1, a tick of 0
samples below 60 Hz, uninitialised channel frequencies, the TedSid SID
model read from the source's 3D flags, chip dumps reloaded under their
voices, dump sizes and loop positions out of range, loops that never
wait, unbounded zx7 decompression, SID envelope levels past 8 bits, and
the SID envelope rate starting as a period instead of an index, AY
chips starting from uninitialised counters, the AY's right channel
written at the wrong stride, AY dumps reloaded under their voices or
with sizes out of range or unbounded zx7, AY speeds that divide by zero,
never advance or overrun the player's buffer, an AY dump's last write
dropped, long AY waits overrunning the buffer, AY voices ending before
their buffered samples, AY rewinds keeping the chips' state, speech
speeds giving frames of no samples, playing speech voices taking a
speed their buffer was not sized for, the speech stress lookahead and
phoneme lookup reading past their arrays, stress marks never
applying, Vizsn noise from the process-wide `rand()`, Vizsn voices
without a text reading a null pointer or ending before their
buffered samples, Vizsn reads overrunning its buffer, Vizsn
voices that could neither rewind nor loop, a looping sfxr sound
dividing by a period of 0 at its restart, and TedSid and AY dumps
whose operations or chip clock made unbounded work on the audio
thread) are
listed in
[PORTING.md](PORTING.md#reference-patch-ledger). Patch 0025 (sounds, buses
and queues that outlive their engine or play on several engines) has no Rust
counterpart: sounds hold no engine pointer, and voices own their samples.

### API changes in milestone F

- `bevy_solouder::AudioClip::duration_seconds` returns `Option<f64>`:
  `None` for a generated clip, whose generator decides when it ends.
  Code that only loads decoded or streamed clips can take
  `.unwrap_or_default()`; `AudioClip::synth` tells the generated ones apart.
- `Ay::chip_type` returns `Option<AyChipType>` rather than the dump's
  byte; `AyChipType::to_byte` gives the byte.
- `Monotone::set_params` takes a `MonotoneWaveform` and `Monotone::waveform`
  returns one; `set_params_raw` and `waveform_raw` keep SoLoud's `int`
  (values past 8 play square waves), and `MonotoneSettings::waveform`
  stays an `i32` (`i32::from` a `MonotoneWaveform`).

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
- Visualization: `set_visualization`, the mix's `wave`, `fft`,
  `approximate_volume` and `visualization_data`, and buses' `bus_wave`,
  `bus_fft` and `bus_approximate_volume`, read from what the renderer
  published at its last render (`visual_tests`, and the `visualization`
  scenario replayed through the controller).
- Streams, played and queued: the `StreamWorker` decodes them ahead of the
  audio thread (`stream_tests`).
- 3D audio: `play_3d`, `play_3d_clocked`, `bus_play_3d` and
  `bus_play_3d_clocked`, the listener, source, speed of sound and speaker
  setters and getters, and `update_3d_audio`, whose math (attenuation,
  colliders, attenuators, Doppler, distance delay) runs on the game thread
  and whose results reach the audio thread as one batched command
  (`spatial_tests`, and every spatial scenario replayed through the
  controller in `solouder-parity`'s `realtime` test).
- Generators (`Synth`), played, seeked, looping and queued
  (`realtime_no_alloc` in `solouder-sources`).

Limits:

| Area | Behavior |
|---|---|
| Mixer only | Voice groups, voice getters other than those in `Status`, bus voice counts, queue counts and `is_current`, and filter parameter reads |
| Visualization | The renderer publishes the mix's data, and each watched bus's (about a kilobyte each), after every render while the mix records or any bus is watched; reads see the last render's data. A bus is watched once `set_bus_visualization(bus, true)` or a play of a bus that records goes through the controller, and holds its slot until `set_bus_visualization(bus, false)` releases it (its last data stays readable until another bus takes the slot over; `a_released_slot_goes_to_the_next_bus`); with every one of `RealtimeConfig::visualized_buses` (default 8) held, those calls fail with `TooManyVisualizedBuses`. A bus that was not playing at the last render reads `None` (0 for volumes), as on the `Mixer`. A render longer than the scratch buffer records its last pass |
| Seek errors | A seek backwards on a bus or queue voice, which they cannot do, is ignored; `CannotSeekBack` is not reported |
| Seek cost | A seek on a generator (`Synth`) voice renders and discards the whole distance (from the start, after a rewind, when seeking back) in passes of the scratch size, inside the render that applies it, as SoLoud's generic seek does (`features/playback/generator.rs`). That render's work grows with the distance: seeking minutes into a chip dump or speech costs minutes of synthesis on the audio thread, enough to miss the device's deadline. Streams seek on the `StreamWorker` instead (WAV by frame, Ogg to the target's frame, FLAC and MPEG decoding up to the target), off the audio thread |
| Queued sounds | A sound the renderer cannot queue (queue not playing, or full) is retired and counted in `Status::rejected_queue_sounds`, not reported on its own. Each queued sound holds a retirement credit until it has played and been collected. A controller plays at most `RealtimeConfig::queues` distinct queues (default 64) besides those the mixer already had at `split`; each new queue's list is allocated on its first play |
| Filter sizing | Memory is reserved for a voice's filters at its channels and starting rate, for global filters at the output layout, and for bus filters at the largest layout among the bus's voices this controller requested and has not seen finish (channels, and the fastest speed each was set, faded or oscillated to); a bus played before `split` uses the `Bus`'s current settings. `split` reserves for the filters the mixer already holds, at each voice's current rate or the top of its speed fade. Echo reserves for the delay it was created with, its `DELAY` parameter's maximum (larger values are accepted); the flanger for 0.1 s, its `DELAY` maximum, or its own delay if longer. The render allocates when a filter needs more than that: echo or DC removal first running at a higher rate than reserved (a voice sped up or given a higher sample rate before its first mixed block), an echo delay set above its maximum before its first block, or a flanger whose rate or delay grows past its reserved line at any time. A 3D voice's filters are reserved at its starting speed times its starting Doppler factor (at least 1). Doppler never makes the render allocate: the controller tracks the rate each 3D voice's filters have memory for (its sample rate and the fastest speed it was set, faded or oscillated to, times the Doppler factor), and an update that would run them faster carries memory prepared on the game thread for 1.25 times the rate needed, which the renderer swaps into the filter instances (`FilterInstance::grow`, contents kept) before applying the gains; the memory they gave up returns in the batch and is freed on the game thread. A 3D bus voice's later bus filters are prepared for its raised rate too. A speed setter on a 3D voice still allocates until the next update grows its filters, as for other voices |
| Finish reasons | `FinishReason::Ended` covers playing to the end, including during a bus seek, and a scheduled stop coming due; `Stopped` covers stop commands, stopping a bus or a sound's voices, a play that replaced the voice, and a 3D voice killed for being inaudible |
| 3D updates | `update_3d_audio` fails with `QueueFull` while `RealtimeConfig::spatial_batches` updates (default 2) are in flight; each batch holds room for every voice ID, allocated by `split`. It is the newest state that is refused while the older ones in flight still apply, so what is heard can lag the game by that many updates (about a buffer) until the renderer catches up. Colliders run in `VoiceId` order rather than slot order, and also for voices that ended on the audio thread whose final event is unread (their results are dropped). The setters only change the controller's copy, so nothing reaches the audio thread until the next update or 3D play. A voice played in 3D on the `Mixer` before `split` is not updated |

## Not yet covered

- Device reconnection; device output outside Windows WASAPI (a PulseAudio
  null sink in a container only, see "Platforms"; underruns on real
  hardware are not measured); CoreAudio and arm64 (CI configured, never
  run).
- Voice groups, voice getters, bus voice counts, queue counts, and filter
  parameter reads through the `Controller` (see "Real-time
  coverage").
- WAV ADPCM; chained Ogg files (several logical streams one after another) are untested.
