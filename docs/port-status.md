# Port status

What the Rust port does today, where it deliberately differs from the C++
reference, and how it is verified. Per-behavior status lives in
[parity-ledger.csv](parity-ledger.csv); the plan is in
[PORTING.md](PORTING.md#milestones-and-acceptance-gates).

## Summary

Milestone B (offline playback), except decoding WAV files. The mixer plays
prepared PCM with 1 to 8 channels on outputs of 1 to 8 channels: play, stop,
pause, loop, volume, pan, relative speed, point/linear/Catmull-Rom
resampling, delayed starts, protected voices, the active-voice limit, global
volume and both clippers. Every scenario renders the same samples as the C++
reference, bit for bit. No device output, decoders or Bevy integration exist
yet.

| Crate | Slices / modules | Contents |
|---|---|---|
| `solouder-core` | `contracts` | `Pcm` (validated planar samples), `SAMPLE_GRANULARITY`, `MAX_CHANNELS` |
| `solouder-core` | `dsp` | Resamplers, pan-and-expand for every channel layout, round-off and hard clipping |
| `solouder-core` | `features/playback` | `Mixer`, `MixerConfig`, `Sound`, `Handle`, `PlayOptions`, voice table, active-voice selection, resample-buffer pool, bus mixing, voice control and queries |
| `solouder-parity` | — | Scenario parser, test patterns, Rust runner, comparison with C++ output |

Ledger: the Milestone B playback behaviors are `ported`; `Soloud::init`,
`AudioSource::setVolume` and `AudioSource::setLooping` are `in-progress`
(init flags and the rest of the source-level defaults remain).

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

Every scenario has an identical log and a maximum sample difference of 0 on
Windows x86-64. Run with `python scripts/run_parity.py` or
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
| Voice end | The audio thread releases the source instance under the engine mutex | The voice's `Arc<[f32]>` is dropped inside the mix call | Acceptable offline; Milestone C moves retirement off the real-time thread |

Known reference defects fixed on both sides by patches 0018 to 0022 (pan ramp
leftovers, surround fold-down volumes, no-free-voice handling, raw layout
validation, the #343 hang) are listed in
[PORTING.md](PORTING.md#reference-patch-ledger).

## Not yet covered

- Decoding WAV files into `Pcm` (the remaining Milestone B item).
- Init flags other than the clipper (`LEFT_HANDED_3D`, ...), `mixSigned16`,
  device backends and the real-time command queue (Milestone C).
- Inaudible behavior, 3D audio, filters, buses, faders, voice groups and
  queues (Milestone D).
- Streaming and the remaining decoders (Milestone E).
