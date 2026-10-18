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

- `sfxr/` (and the other generator inputs): written by `python
  scripts/gen_source_fixtures.py`, field by field in the layouts SoLoud's
  loaders read; `--check` works as for WAV. Sfxr parameter files cover
  versions 100, 101 and 102, every wave type and one without a waveform
  (`wave_7`), a filter flag byte of 2 and trailing bytes; `bad_cut_*` ends
  inside the fields, `bad_version_*` has an unknown version and `bad_empty`
  is empty. Monotone songs: `demo` uses every effect, notes past 99 and
  vibrato on note 1; `jumps` jumps into a missing pattern; `twelve` has
  the most tracks; `no_end` has no end marker in its order; `long_title`
  has title and comment lengths past their 40 bytes and a NUL;
  `trailing` has bytes after its last cell. `bad_magic`, `bad_version_2`,
  `bad_cell_size_1` and `bad_13_tracks` are refused; `bad_cut_*` end in
  the title, the order and the cells; `bad_empty` is empty. Chip dumps
  (`chipdump/`, the `CHIPTUNE` layout of SoLoud's `tedsid2dump`; `.zx7`
  ones are compressed with the script's own zx7 encoder): a SID tune for
  each model (`sid_6581`, `sid_8580`, `sid_8580db`, `sid_6581r1`), a TED
  tune (`ted`), both chips over two chunks looping into the second from
  an odd byte (`sidted`), chip type 6 with a later data offset
  (`sidted_6`), and an odd length (`odd_length`). `bad_short`,
  `bad_magic`, `bad_chip_1`, `bad_no_chunks`, `bad_loop_end` and
  `bad_data_offset` have bad headers; `bad_cut` ends in its data;
  `bad_no_wait` and `bad_zero_waits` loop without waiting;
  `bad_zx7_overrun` decompresses past its length and `bad_zx7_cut` ends
  in a compressed chunk. AY dumps (`ay_*`): one AY chip, a YM chip
  (flag 64), TurboSound and TurboSound Next (chip types 2 and 3, with
  writes to a second chip), the slowest tick rate and chip clock the
  player renders (`ay_slow`), a fast tick rate (`ay_fast`), and several
  chunks with the loop position at the end, which Ay does not use
  (`ay_long`); each tune uses every tone, noise, mixer and envelope
  setting, a pause longer than a frame and a last op that writes.
  `bad_ay_chip_0` is a TedSid chip type, `bad_ay_cpu_0`, `bad_ay_cpu_10`
  and `bad_ay_chip_speed` have speeds the player cannot render, and
  `bad_ay_no_chunks`, `bad_ay_cut` and `bad_ay_zx7_cut` are damaged.

Change a fixture by editing the generator and rerunning it, never by hand.
