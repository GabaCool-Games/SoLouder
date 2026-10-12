# Third-party code and notices

SoLoud proper is zlib/libpng (see [LICENSE](LICENSE)). SoLoud also bundles
third-party code with its own terms, summarized from SoLoud's
`docsrc/legal.mmd`. When SoLouder ports one of these components, its notice
must be kept with the port and the component's row updated here.

None of this code has been ported yet. The C++ reference checkout under
`reference/soloud` is git-ignored and keeps its original notices.

| Component | SoLoud location | Origin and license | SoLouder plan |
|---|---|---|---|
| Ogg Vorbis decoding | `src/audiosource/wav/stb_vorbis.c` | stb_vorbis by Sean Barrett, public domain | Not translated; use a maintained Rust decoder |
| MP3 decoding | `src/audiosource/wav/dr_mp3.h` | dr_mp3 by David Reid, Unlicense (based on minimp3, CC0) | Not translated |
| WAV decoding | `src/audiosource/wav/dr_wav.h` | dr_wav by David Reid, Unlicense | Not translated |
| FLAC decoding | `src/audiosource/wav/dr_flac.h` | dr_flac by David Reid, Unlicense | Not translated |
| Freeverb | `src/filter/soloud_freeverbfilter.cpp` | Jezar at Dreampoint, public domain (original release, not GPL Freeverb3) | Port with attribution |
| FFT | `src/core/soloud_fft.cpp` | Based on fftreal by Laurent de Soras, WTFPL | Port with attribution |
| Sfxr | `src/audiosource/sfxr/` | Tomas Pettersson, relicensed to zlib by permission | Port; keep the zlib notice and permission note |
| Speech synthesizer | `src/audiosource/speech/` | rsynth by Nick Ing-Simmons et al., public domain; SoLoud changes public domain/CC0; `soloud_speech.*` zlib | Port with attribution |
| Vizsn | `src/audiosource/vizsn/` | Ville-Matias Heikkilä, WTFPL | Port with attribution |
| VIC 6560/6561 emulator | `src/audiosource/vic/` | Petri Häkkinen, zlib/libpng | Port; keep his zlib notice |
| TED/SID | `src/audiosource/tedsid/` | Based on tedplay by Attila Grósz, Unlicense | Port with attribution |
| AY | `src/audiosource/ay/` | YM-2149F emulator and resampler from Unreal Speccy by SMT, public domain; SoLoud wrapper zlib | Port with attribution |
| OpenMPT | `src/audiosource/openmpt/` | Loads libopenmpt (BSD-3-Clause) at runtime | Excluded unless needed |

Rust dependencies are audited separately when they are added. Note that
Symphonia, the candidate decoder, is MPL-2.0.
