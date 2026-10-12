# Port status

What the Rust port does today, where it deliberately differs from the C++
reference, and how it is verified. Per-behavior status lives in
[parity-ledger.csv](parity-ledger.csv); the plan is in
[PORTING.md](PORTING.md#milestones-and-acceptance-gates).

## Summary

Milestone B (offline playback) is done. The mixer plays prepared PCM with 1
to 8 channels on outputs of 1 to 8 channels: play, stop, pause, loop, volume,
pan, relative speed, point/linear/Catmull-Rom resampling, delayed starts,
protected voices, the active-voice limit, global volume and both clippers.
WAV files decode into that PCM. Every scenario renders the same samples as
the C++ reference, bit for bit, except 8-bit WAV (see below). No device
output, other codecs or Bevy integration exist yet.

| Crate | Slices / modules | Contents |
|---|---|---|
| `solouder-core` | `contracts` | `Pcm` (validated planar samples), `SAMPLE_GRANULARITY`, `MAX_CHANNELS` |
| `solouder-core` | `dsp` | Resamplers, pan-and-expand for every channel layout, round-off and hard clipping |
| `solouder-core` | `features/playback` | `Mixer`, `MixerConfig`, `Sound`, `Handle`, `PlayOptions`, voice table, active-voice selection, resample-buffer pool, bus mixing, voice control and queries |
| `solouder-decoders` | `features/decoded_audio` | `decode_wav` (Symphonia for parsing, dr_wav's sample conversion), `DecodeLimits`, `DecodeError` |
| `solouder-parity` | — | Scenario parser, test patterns, WAV fixture loading, Rust runner, comparison with C++ output |

Ledger: the Milestone B playback behaviors are `ported`; `Soloud::init`,
`AudioSource::setVolume` and `AudioSource::setLooping` are `in-progress`
(init flags and the rest of the source-level defaults remain), as are
`Wav::load` and `Wav::loadMem` (WAV only; the other codecs come in E).

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
| `active_voices` | Protected and loudest voices win the active slots; changing and invalid limits | 2,412 |
| `delay_samples` | Delayed starts spanning mix calls | 1,666 |
| `wav_decode` | 16/24/32-bit integer, 32/64-bit float, A-law and mu-law WAV; extensible 5.1, 7.1 and 24-in-32-bit headers; extra and odd-sized chunks; four files both sides reject | 14,282 |
| `wav_decode_u8` | 8-bit WAV (tolerance 1/128, see below) | 1,000 |
| `wav_playback` | Decoded files played at and away from the output rate, a 5.1 fold-down, a looping file | 8,246 |

Every scenario has an identical log and a maximum sample difference of 0 on
Windows x86-64, except `wav_decode_u8` (7.8e-3, the 8-bit conversion) and
`wav_decode` (3e-39: one subnormal float sample the C++ build flushes to
zero). Run with `python scripts/run_parity.py` or
`python scripts/check_all.py --reference --parity`.

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
| Voice end | The audio thread releases the source instance under the engine mutex | The voice's `Arc<[f32]>` is dropped inside the mix call | Acceptable offline; Milestone C moves retirement off the real-time thread |

Known reference defects fixed on both sides by patches 0018 to 0022 (pan ramp
leftovers, surround fold-down volumes, no-free-voice handling, raw layout
validation, the #343 hang) are listed in
[PORTING.md](PORTING.md#reference-patch-ledger).

## Not yet covered

- Init flags other than the clipper (`LEFT_HANDED_3D`, ...), `mixSigned16`,
  device backends and the real-time command queue (Milestone C).
- Inaudible behavior, 3D audio, filters, buses, faders, voice groups and
  queues (Milestone D).
- Streaming, Ogg Vorbis, FLAC, MP3 and WAV ADPCM (Milestone E).
