# Test fixtures

Small audio files used by the parity scenarios (`load file=...`) and the
decoder unit tests. They are generated, not recorded, so they carry the
repository's license and contain no third-party audio.

- `wav/`: written by `python scripts/gen_wav_fixtures.py` from integer
  formulas; `--check` (part of `scripts/check_all.py`) fails if a committed file
  differs from what the script generates. Files named `bad_*` are malformed on
  purpose; `short_data_*` ends before its declared data length.

- `flac/`: written by `python scripts/gen_flac_fixtures.py`, a small FLAC
  encoder covering every subframe type, both residual codings, escaped
  partitions, wasted bits, stereo decorrelation, variable block sizes and
  metadata blocks; `--check` works as for WAV. `*_variable_equal` uses
  variable blocking with blocks of one size; `s32_stereo_*` has 33-bit side
  subframes, some verbatim, and loud mid/side. `bad_crc_*` has a frame with a
  wrong checksum, `bad_crc_runs_*` runs of them, `bad_crc8_*` a frame header
  with a wrong checksum, `bad_overlap_*` two
  frames with the same number, `short_data_*` is cut off in its last frame,
  `long_claim_*` declares more frames than it holds, `unknown_length_*`
  declares none and `bad_overflow_*` holds subframes far beyond its sample
  size, which overflow 32-bit arithmetic when decorrelated, followed by a
  good frame.

Change a fixture by editing the generator and rerunning it, never by hand.
