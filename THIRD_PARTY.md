# Third-party code and notices

SoLoud proper is zlib/libpng (see [LICENSE](LICENSE)). SoLoud also bundles
third-party code with its own terms, summarized from SoLoud's
`docsrc/legal.mmd`. When SoLouder ports one of these components, its notice
must be kept with the port and the component's row updated here.

Rows saying "ported" name the SoLouder code translated from them; the rest has not been translated. The C++ reference checkout under
`reference/soloud` is git-ignored and keeps its original notices.

| Component | SoLoud location | Origin and license | SoLouder plan |
|---|---|---|---|
| Ogg Vorbis decoding | `src/audiosource/wav/stb_vorbis.c` | stb_vorbis 1.22 by Sean Barrett and contributors, public domain or MIT | Pull-mode decoding path ported to `solouder-decoders` (`decoded_audio/vorbis/`), credited in its files |
| MP3 decoding | `src/audiosource/wav/dr_mp3.h` | dr_mp3 0.6.31 by David Reid, public domain (Unlicense) or MIT No Attribution, based on minimp3 by lieff, CC0 | Decoding path ported to `solouder-decoders` (`decoded_audio/mp3/`), credited in its files |
| WAV decoding | `src/audiosource/wav/dr_wav.h` | dr_wav by David Reid, Unlicense | Not translated; `solouder-decoders` has its own RIFF reader and reuses dr_wav's conversion formulas and G.711 tables (credited in `convert.rs`) |
| FLAC decoding | `src/audiosource/wav/dr_flac.h` | dr_flac by David Reid, Unlicense or MIT No Attribution | Decoding path ported to `solouder-decoders` (`decoded_audio/flac/`), credited in its files |
| Freeverb | `src/filter/soloud_freeverbfilter.cpp` | Jezar at Dreampoint, public domain (original release, not GPL Freeverb3) | Port with attribution |
| FFT | `src/core/soloud_fft.cpp` | Based on fftreal by Laurent de Soras, WTFPL | Port with attribution |
| Sfxr | `src/audiosource/sfxr/` | Tomas Pettersson, relicensed to zlib by permission | Port; keep the zlib notice and permission note |
| Speech synthesizer | `src/audiosource/speech/` | rsynth by Nick Ing-Simmons et al., public domain; SoLoud changes public domain/CC0; `soloud_speech.*` zlib | Port with attribution |
| Vizsn | `src/audiosource/vizsn/` | Ville-Matias Heikkilä, WTFPL | Port with attribution |
| VIC 6560/6561 emulator | `src/audiosource/vic/` | Petri Häkkinen, zlib/libpng | Ported to `solouder-sources` (`features/vic/`), with his zlib notice in the file |
| TED/SID | `src/audiosource/tedsid/` | Based on tedplay by Attila Grósz, Unlicense | Port with attribution |
| AY | `src/audiosource/ay/` | YM-2149F emulator and resampler from Unreal Speccy by SMT, public domain; SoLoud wrapper zlib | Port with attribution |
| WELL512 random numbers | `src/core/soloud_misc.cpp` (`Misc::Prg`) | WELL512 by Chris Lomont, public domain | Ported to `solouder-sources` (`prg.rs`), credited in the file |
| zx7 decompression | `include/zx7decompress.h` | Jari Komppa, public domain (Unlicense), based on zx7 by Einar Saukas, who asks to be credited; the zx7 compressor has a different licence and is not used | Port with attribution to both (TedSid and AY dumps); SoLoud's `legal.mmd` mentions neither zx7 nor AY |
| OpenMPT | `src/audiosource/openmpt/` | Loads libopenmpt (BSD-3-Clause) at runtime | Excluded unless needed |

## Rust dependencies

Direct dependencies, checked when they were added. Their own dependencies
are MIT, Apache-2.0, Zlib, Unicode-3.0 or CC0 variants; check again with `cargo tree -e normal -f '{p} {l}'`.

| Crate | Used by | License | Notes |
|---|---|---|---|
| `cpal` 0.18 | `solouder-cpal` | Apache-2.0 | Links the platform audio API (WASAPI, ALSA, CoreAudio). |
| `rtrb` 0.4 | `solouder-core`, `solouder-cpal` | MIT OR Apache-2.0 | Wait-free ring buffers between game and audio thread. |
| `bevy` 0.20 (`std`, `bevy_asset`) | `bevy_solouder` | MIT OR Apache-2.0 | |
| `stats_alloc` 0.1 | `solouder-core` and `solouder-sources` tests only | MIT | Counts allocations in `realtime_no_alloc`. |

WAV, FLAC, Ogg Vorbis and MP3 are decoded by first-party code (the last three ported from the libraries above).
