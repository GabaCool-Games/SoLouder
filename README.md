# SoLouder

SoLouder is a native Rust port of the [SoLoud](https://github.com/jarikomppa/soloud)
audio engine, with a Bevy plugin, carrying the fixes and extensions developed
while shipping SoLoud in a Unity game.

**SoLouder is an altered version of SoLoud. It is not the original SoLoud and
is not affiliated with or endorsed by SoLoud's authors.** SoLoud was created by
Jari Komppa and developed with many contributors; see
[CONTRIBUTORS.md](CONTRIBUTORS.md).

## Status

All six milestones of [the plan](docs/PORTING.md#milestones-and-acceptance-gates)
are done: SoLouder plays WAV, FLAC, Ogg Vorbis and MP3 files (decoded or
streamed) and SoLoud's generated sources (Sfxr, speech, Vizsn, noise,
Monotone, TedSid, Vic, AY) on an audio device, from plain Rust or from Bevy
entities, with SoLoud's voice controls, fades, voice groups, filters, buses,
queues, 3D audio and visualization. Verified on Windows (WASAPI) and in a
Linux container; macOS is configured in CI but has never run, and no CI
job has run on a host yet. [docs/port-status.md](docs/port-status.md) has
the details, and [the milestone F review](docs/PORTING.md#milestone-f-review)
lists what stays open.

- **A patched C++ reference.** Upstream SoLoud, pinned in
  `reference/upstream.json`, plus 115 patches in `reference/patches/`. They fix
  upstream crashes, hangs, and memory-safety bugs that were never merged
  upstream, including the WavStream seek heap corruption
  ([#405](https://github.com/jarikomppa/soloud/issues/405)). Every behavioral
  fix has a C++ regression test in `reference/tests/`.
- **A parity ledger.** [docs/parity-ledger.csv](docs/parity-ledger.csv) lists
  every behavior of SoLoud's C API, 208 of them. That is the 1:1 target.
- **A parity harness.** 56 scenarios in [tests/scenarios/](tests/scenarios/)
  run through both the C++ reference and the Rust port, and the outputs are
  compared sample by sample. All match bit for bit except one documented
  8-bit WAV conversion, on Windows and on Linux.
- **The Rust crates.**
  - `solouder-core`: the mixer (play, stop, pause, loop, seek, volume, pan,
    speed, fades, three resamplers, 1 to 8 channels, clipping, voice groups,
    ten filters, buses, queues, streaming, 3D audio and visualization) and a
    wait-free controller/renderer split for real-time use.
  - `solouder-decoders`: WAV, FLAC, Ogg Vorbis and MP3, decoded or streamed.
  - `solouder-sources`: the generated sources, one Cargo feature each.
  - `solouder-cpal`: output on the default audio device.
  - `bevy_solouder`: the Bevy plugin (audio and generated clip assets,
    `SoundPlayer` entities that stop when despawned, settings, status and
    messages, a spatial listener and emitters, visualization data).
  - `solouder-parity`: the harness.
  - `solouder-bench`: render-time benchmarks against the C++ reference and a
    multi-threaded soak test (not published).
- **Design and porting guides.** [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
  and [docs/PORTING.md](docs/PORTING.md).

## Getting started

Requirements: Python 3.10+, Git and Rust (the version in
`rust-toolchain.toml`; rustup installs it). The C++ reference tests and the
parity comparison also need CMake and a C++ compiler (Visual Studio 2022 on
Windows).

```sh
python scripts/fetch_reference.py           # creates the git-ignored reference/soloud
python scripts/check_all.py --reference --parity
```

On Linux, CPAL also needs the ALSA headers (`libasound2-dev` on Debian and
Ubuntu). To hear something:

```sh
cargo run -p solouder-cpal --example play_wav -- tests/fixtures/wav/s16_stereo_44100.wav
cargo run -p bevy_solouder --example play_and_despawn
```

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | Rust crates: `solouder-core`, `solouder-decoders`, `solouder-sources`, `solouder-cpal`, `bevy_solouder`, `solouder-parity`, `solouder-bench` |
| `tests/fixtures/` | WAV, FLAC, Ogg, MP3 and generated-source fixtures (`scripts/gen_*_fixtures.py`) |
| `tests/scenarios/` | Parity scenarios shared by the C++ and Rust runners |
| `docs/` | Architecture, porting guide, parity ledger, port status |
| `reference/upstream.json` | Upstream URL and pinned commit |
| `reference/patches/` | Patches applied to upstream, in order |
| `reference/tests/` | C++ regression tests and the C++ parity runner (CMake) |
| `reference/soloud/` | Git-ignored C++ checkout created by `scripts/fetch_reference.py` |
| `scripts/` | Reference fetch/export/test, parity run, benchmarks, all checks, ledger generator, file-size check |

## License

zlib/libpng, the same as SoLoud ([LICENSE](LICENSE)). Third-party code is
listed in [THIRD_PARTY.md](THIRD_PARTY.md). Contributions: see
[CONTRIBUTING.md](CONTRIBUTING.md) and [CONTRIBUTORS.md](CONTRIBUTORS.md).
