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

- `ogg/` and `mp3/`: written by `python scripts/gen_lossy_fixtures.py
  --encode`, which encodes generated tones, sweeps and noise with ffmpeg
  (libvorbis; libmp3lame for layer 3, libtwolame for layer 2). Encoders
  differ between versions, so the encoded files are committed and only
  re-encoded on purpose. Vorbis files cover 1 to 8 channels, 8 to 48 kHz,
  qualities 0 to 10 and a file shorter than one block; MP3 files cover
  MPEG-1, 2 and 2.5, mono, joint and plain stereo, constant and variable
  bit rates, with and without a Xing/LAME frame and an ID3v2 tag, and
  layer 2. The script derives the damaged ones from them (`--check` works as
  for WAV for those): `bad_truncated_*` is cut off, `bad_payload_*` has
  garbled audio data (an Ogg page with a valid checksum, MP3 frames),
  `bad_crc_*` an Ogg page with a wrong checksum, `bad_headers_only` no
  audio pages, `bad_setup` a broken Vorbis setup header, `bad_cut_header`
  only part of its first page, `bad_junk_prefix_*` bytes before the first
  MP3 frame, and `bad_not_audio` no audio at all.

Change a fixture by editing the generator and rerunning it, never by hand.
