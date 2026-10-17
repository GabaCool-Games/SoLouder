#!/usr/bin/env python3
"""Generate the generator fixtures in tests/fixtures/ (sfxr/, monotone/, ...).

SoLoud ships none: sfxr parameter files and Monotone songs are built here
field by field, in the layouts `Sfxr::loadParamsFile` and
`Monotone::loadFile` read, together with damaged variants.
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


MONOTONE_MAGIC = b"\x08MONOTONE"


def cell(note: int, effect: int = 0, data: int = 0) -> int:
    """A Monotone cell: note 1 to 126 (127 ends the note, 0 keeps the
    last), effect 0 to 7 (arpeggio, slide up, slide down, slide to the note,
    vibrato, order jump, row jump, ticks a row) and 6 bits of data."""
    return note << 9 | effect << 6 | data


def pascal(text: bytes, length: int | None = None) -> bytes:
    return bytes([len(text) if length is None else length]) + text.ljust(40, b"\0")[:40]


def monotone_song(tracks: int, patterns: list[dict[tuple[int, int], int]], order: list[int],
                  title: bytes = b"", comment: bytes = b"", title_length: int | None = None,
                  comment_length: int | None = None, version: int = 1, cell_size: int = 2) -> bytes:
    """`patterns` maps (row, track) to a cell; the order is padded with 0xff."""
    out = MONOTONE_MAGIC + pascal(title, title_length) + pascal(comment, comment_length)
    out += bytes([version, len(patterns), tracks, cell_size])
    out += bytes(order + [0xFF] * (256 - len(order)))
    for pattern in patterns:
        for row in range(64):
            for track in range(tracks):
                out += struct.pack("<H", pattern.get((row, track), 0))
    return out


def demo_patterns() -> list[dict[tuple[int, int], int]]:
    p0: dict[tuple[int, int], int] = {(0, 3): cell(0, 7, 3)}
    melody = [49, 53, 56, 61, 56, 53, 49, 44]
    for i, row in enumerate(range(0, 64, 4)):
        # An arpeggio of 4 and 7 notes up on the second quarter.
        p0[(row, 0)] = cell(melody[i % 8], 0, (4 << 3 | 7) if 16 <= row < 32 else 0)
    p0[(0, 1)] = cell(25)
    for row in range(1, 8):
        p0[(row, 1)] = cell(0, 1, 3)
    p0[(8, 1)] = cell(30)
    for row in range(9, 16):
        p0[(row, 1)] = cell(0, 2, 5)
    p0[(16, 1)] = cell(127)
    p0[(24, 1)] = cell(28, 4, 3 << 3 | 4)
    for row in range(25, 41):
        p0[(row, 1)] = cell(0, 4, 0)
    # A slide to the first note from 0 Hz, then between notes.
    p0[(0, 2)] = cell(37, 3, 6)
    p0[(8, 2)] = cell(44, 3, 8)
    for row in range(9, 16):
        p0[(row, 2)] = cell(0, 3, 8)
    p0[(16, 2)] = cell(32, 3, 20)
    for row in range(17, 24):
        p0[(row, 2)] = cell(0, 3, 20)
    p0[(32, 3)] = cell(60, 0, 3 << 3 | 7)
    p0[(48, 3)] = cell(127)

    p1: dict[tuple[int, int], int] = {(0, 3): cell(0, 7, 5)}
    for i, row in enumerate(range(0, 64, 2)):
        p1[(row, 0)] = cell(70 + i % 30)
    p1[(62, 0)] = cell(99, 0, 7 << 3 | 7)
    # Vibrato 7 deep on note 1 reads the table's entry 1.
    p1[(8, 2)] = cell(1, 4, 2 << 3 | 7)
    for row in range(9, 30):
        p1[(row, 2)] = cell(0, 4, 0)
    p1[(16, 1)] = cell(12, 4, 7 << 3 | 1)
    p1[(17, 1)] = cell(0, 4, 0)

    p2: dict[tuple[int, int], int] = {(0, 3): cell(0, 7, 2)}
    for i, row in enumerate(range(0, 32, 2)):
        # Notes past 99 play the table's last entry.
        p2[(row, 0)] = cell(100 + i, 0, (i % 8) << 3 | (7 - i % 8))
        p2[(row, 1)] = cell(126 - i)
    p2[(32, 3)] = cell(0, 6, 8)
    return [p0, p1, p2]


def monotone_fixtures() -> dict[str, bytes]:
    demo = monotone_song(4, demo_patterns(), [0, 1, 2, 1], title=b"SoLouder demo", comment=b"Every effect")
    jumps = monotone_song(2, [
        {(0, 0): cell(40), (0, 1): cell(52), (20, 0): cell(0, 5, 5)},
        {(0, 0): cell(45, 0, 3 << 3 | 5), (0, 1): cell(0, 7, 2), (16, 1): cell(0, 5, 1)},
    ], [1, 0], title=b"Jumps")
    twelve_cells = {}
    for track in range(12):
        twelve_cells[(track * 2, track)] = cell(30 + 3 * track, track % 5, (track * 5) % 64 if track % 5 else 0)
        twelve_cells[(40 + track, track)] = cell(127)
    twelve = monotone_song(12, [twelve_cells], [0], title=b"Twelve tracks")
    no_end = monotone_song(1, [{(0, 0): cell(40, 7, 1), (10, 0): cell(52), (63, 0): cell(0, 6, 2)}], [0] * 256,
                           title=b"No end marker")
    files = {
        "demo.mon": demo,
        "jumps.mon": jumps,
        "twelve.mon": twelve,
        "no_end.mon": no_end,
        # Lengths past the 40 bytes each holds; the comment has a NUL.
        "long_title.mon": monotone_song(1, [{(0, 0): cell(50)}], [0], title=b"T" * 40, title_length=255,
                                        comment=b"Comment\0hidden", comment_length=41),
        "trailing.mon": monotone_song(1, [{(0, 0): cell(50)}], [0]) + b"trailing bytes",
        "bad_magic.mon": b"\x09" + demo[1:],
        "bad_version_2.mon": monotone_song(4, demo_patterns(), [0], version=2),
        "bad_cell_size_1.mon": monotone_song(4, demo_patterns(), [0], cell_size=1),
        "bad_13_tracks.mon": monotone_song(13, [{}], [0]),
        "bad_cut_title.mon": demo[:30],
        "bad_cut_order.mon": demo[:200],
        "bad_cut_cells.mon": demo[:-1],
        "bad_empty.mon": b"",
    }
    return files


def fixtures() -> dict[str, bytes]:
    out = {f"sfxr/{name}": data for name, data in sfxr_fixtures().items()}
    out.update({f"monotone/{name}": data for name, data in monotone_fixtures().items()})
    return out


GLOBS = ["sfxr/*.sfx", "monotone/*.mon"]


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
