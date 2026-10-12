# Test fixtures

Small audio files used by the parity scenarios (`load file=...`) and the
decoder unit tests. They are generated, not recorded, so they carry the
repository's license and contain no third-party audio.

- `wav/`: written by `python scripts/gen_wav_fixtures.py` from integer
  formulas; `--check` (part of `scripts/check_all.py`) fails if a committed file
  differs from what the script generates. Files named `bad_*` are malformed on
  purpose; `short_data_*` ends before its declared data length.

Change a fixture by editing the generator and rerunning it, never by hand.
