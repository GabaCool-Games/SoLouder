#!/usr/bin/env python3
"""Generate the generator fixtures in tests/fixtures/ (sfxr/, ...).

SoLoud ships none: sfxr parameter files are built here field by field, in
the layout `Sfxr::loadParamsFile` reads, together with damaged variants.
Every file is deterministic. With --check, fail if a committed fixture
differs from what this script generates.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import struct
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = REPOSITORY_ROOT / "tests" / "fixtures"

# Fields after the wave type, in file order: (name, versions that have it).
SFXR_FIELDS = [
    ("sound_vol", {102}),
    ("base_freq", {100, 101, 102}),
    ("freq_limit", {100, 101, 102}),
    ("freq_ramp", {100, 101, 102}),
    ("freq_dramp", {101, 102}),
    ("duty", {100, 101, 102}),
    ("duty_ramp", {100, 101, 102}),
    ("vib_strength", {100, 101, 102}),
    ("vib_speed", {100, 101, 102}),
    ("vib_delay", {100, 101, 102}),
    ("env_attack", {100, 101, 102}),
    ("env_sustain", {100, 101, 102}),
    ("env_decay", {100, 101, 102}),
    ("env_punch", {100, 101, 102}),
    ("filter_on", {100, 101, 102}),
    ("lpf_resonance", {100, 101, 102}),
    ("lpf_freq", {100, 101, 102}),
    ("lpf_ramp", {100, 101, 102}),
    ("hpf_freq", {100, 101, 102}),
    ("hpf_ramp", {100, 101, 102}),
    ("pha_offset", {100, 101, 102}),
    ("pha_ramp", {100, 101, 102}),
    ("repeat_speed", {100, 101, 102}),
    ("arp_speed", {101, 102}),
    ("arp_mod", {101, 102}),
]


def sfxr_file(version: int, wave_type: int, values: dict[str, float], filter_byte: int = 0) -> bytes:
    out = struct.pack("<ii", version, wave_type)
    for name, versions in SFXR_FIELDS:
        if version not in versions:
            continue
        if name == "filter_on":
            out += bytes([filter_byte])
        else:
            out += struct.pack("<f", values.get(name, 0.0))
    return out


# A laser-like sound with every effect in use.
LASER = {
    "sound_vol": 0.4, "base_freq": 0.62, "freq_limit": 0.21, "freq_ramp": -0.27,
    "freq_dramp": 0.05, "duty": 0.35, "duty_ramp": 0.12, "vib_strength": 0.3,
    "vib_speed": 0.45, "vib_delay": 0.0, "env_attack": 0.02, "env_sustain": 0.18,
    "env_decay": 0.22, "env_punch": 0.4, "lpf_resonance": 0.5, "lpf_freq": 0.7,
    "lpf_ramp": -0.1, "hpf_freq": 0.08, "hpf_ramp": 0.02, "pha_offset": 0.12,
    "pha_ramp": -0.05, "repeat_speed": 0.0, "arp_speed": 0.6, "arp_mod": 0.35,
}

# A noise burst that repeats, with a falling arpeggio.
BURST = {
    "sound_vol": 0.6, "base_freq": 0.2, "freq_limit": 0.0, "freq_ramp": 0.1,
    "freq_dramp": -0.02, "duty": 0.0, "duty_ramp": 0.0, "vib_strength": 0.0,
    "vib_speed": 0.0, "env_attack": 0.0, "env_sustain": 0.12, "env_decay": 0.3,
    "env_punch": 0.6, "lpf_resonance": 0.0, "lpf_freq": 1.0, "lpf_ramp": 0.0,
    "hpf_freq": 0.0, "hpf_ramp": 0.0, "pha_offset": -0.2, "pha_ramp": 0.1,
    "repeat_speed": 0.55, "arp_speed": 0.7, "arp_mod": -0.5,
}

# A sine tone that slides down to its limit.
SINE = {
    "base_freq": 0.45, "freq_limit": 0.3, "freq_ramp": -0.3, "env_attack": 0.1,
    "env_sustain": 0.2, "env_decay": 0.2, "lpf_freq": 1.0,
}


def sfxr_fixtures() -> dict[str, bytes]:
    files = {
        "v102_laser.sfx": sfxr_file(102, 0, LASER),
        "v101_burst.sfx": sfxr_file(101, 3, BURST),
        "v100_sine.sfx": sfxr_file(100, 2, SINE),
        "v102_saw.sfx": sfxr_file(102, 1, {**SINE, "sound_vol": 0.3, "duty": 0.2}),
        # A wave type without a waveform: every sample is 0 before filtering.
        "v102_wave_7.sfx": sfxr_file(102, 7, LASER),
        # A filter flag byte other than 0 or 1.
        "v102_filter_byte_2.sfx": sfxr_file(102, 0, LASER, filter_byte=2),
        # Extra bytes after the last field are ignored.
        "v101_trailing.sfx": sfxr_file(101, 0, BURST) + b"trailing bytes",
    }
    whole = sfxr_file(102, 0, LASER)
    files["bad_cut_102.sfx"] = whole[: len(whole) - 3]
    files["bad_cut_header.sfx"] = whole[:6]
    files["bad_version_103.sfx"] = sfxr_file(102, 0, LASER).replace(struct.pack("<i", 102), struct.pack("<i", 103), 1)
    files["bad_empty.sfx"] = b""
    return files


def fixtures() -> dict[str, bytes]:
    return {f"sfxr/{name}": data for name, data in sfxr_fixtures().items()}


GLOBS = ["sfxr/*.sfx"]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify instead of writing")
    args = parser.parse_args(argv)

    expected = fixtures()
    if args.check:
        stale = [name for name, data in expected.items()
                 if not (FIXTURE_ROOT / name).is_file() or (FIXTURE_ROOT / name).read_bytes() != data]
        present = {p.relative_to(FIXTURE_ROOT).as_posix() for glob in GLOBS for p in FIXTURE_ROOT.glob(glob)}
        extra = sorted(present - set(expected))
        if stale or extra:
            print("Generator fixtures are out of date; run python scripts/gen_source_fixtures.py", file=sys.stderr)
            for name in stale + extra:
                print(f"  {name}", file=sys.stderr)
            return 1
        print(f"Generator fixtures are up to date ({len(expected)} files).")
        return 0

    for name, data in expected.items():
        path = FIXTURE_ROOT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    print(f"Wrote {len(expected)} fixtures to {FIXTURE_ROOT.relative_to(REPOSITORY_ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
