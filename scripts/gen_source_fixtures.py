#!/usr/bin/env python3
"""Generate the generator fixtures in tests/fixtures/ (sfxr/, monotone/,
chipdump/, speech/, vizsn/).

SoLoud ships none: sfxr parameter files, Monotone songs and CHIPTUNE
register dumps are built here field by field, in the layouts
`Sfxr::loadParamsFile`, `Monotone::loadFile`, `TedSid::loadFile` and
`Ay::loadFile` read,
together with damaged variants, and texts for `Speech::setText`. Compressed dumps use the zx7 encoder below,
written for SoLouder from the format SoLoud's decompressor reads (the zx7
compressor itself has a different licence).
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
        # A base frequency of 20: a period below 1, which a looping voice's
        # restart left at 0 (reference patch 0111).
        "v102_high_base.sfx": sfxr_file(102, 0, {"sound_vol": 0.5, "base_freq": 20.0, "env_sustain": 0.02,
                                                 "env_decay": 0.03, "lpf_freq": 1.0}),
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


# zx7: a literal first byte, then a flag bit per step: 0 and a literal, or
# 1, an Elias gamma length - 1 and an offset - 1 (a byte, or for 128 and
# more a byte with its top bit set and 4 more bits). 16 zeros and a 1 end it.
class BitWriter:
    def __init__(self) -> None:
        self.out = bytearray()
        self.bit_index = -1
        self.mask = 0

    def byte(self, value: int) -> None:
        self.out.append(value)

    def bit(self, value: int) -> None:
        if self.mask == 0:
            self.bit_index = len(self.out)
            self.out.append(0)
            self.mask = 128
        if value:
            self.out[self.bit_index] |= self.mask
        self.mask >>= 1

    def gamma(self, value: int) -> None:
        bits = value.bit_length()
        for _ in range(bits - 1):
            self.bit(0)
        self.bit(1)
        for i in range(bits - 2, -1, -1):
            self.bit((value >> i) & 1)


def zx7_compress(data: bytes) -> bytes:
    assert data
    w = BitWriter()
    w.byte(data[0])
    i = 1
    while i < len(data):
        best_len, best_off = 0, 0
        for off in range(1, min(i, 2176) + 1):
            n = 0
            while i + n < len(data) and n < 65536 and data[i + n] == data[i + n - off]:
                n += 1
            if n > best_len:
                best_len, best_off = n, off
        if best_len >= 2:
            w.bit(1)
            w.gamma(best_len - 1)
            o = best_off - 1
            if o < 128:
                w.byte(o)
            else:
                o -= 128
                w.byte((o & 127) | 128)
                for b in (8, 4, 2, 1):
                    w.bit(1 if (o >> 7) & b else 0)
            i += best_len
        else:
            w.bit(0)
            w.byte(data[i])
            i += 1
    w.bit(1)
    for _ in range(16):
        w.bit(0)
    w.bit(1)
    return bytes(w.out)


def zx7_decompress(data: bytes) -> tuple[bytes, int]:
    pos = 0
    mask = 0
    value = 0
    out = bytearray()

    def byte() -> int:
        nonlocal pos
        b = data[pos]
        pos += 1
        return b

    def bit() -> int:
        nonlocal mask, value
        mask >>= 1
        if mask == 0:
            mask = 128
            value = byte()
        return 1 if value & mask else 0

    out.append(byte())
    while True:
        if not bit():
            out.append(byte())
            continue
        i = 0
        while not bit():
            i += 1
        if i > 15:
            return bytes(out), pos
        v = 1
        for _ in range(i):
            v = v << 1 | bit()
        length = v + 1
        o = byte()
        if o >= 128:
            hi = 0
            for _ in range(4):
                hi = hi << 1 | bit()
            o = (o & 127 | hi << 7) + 128
        off = o + 1
        for _ in range(length):
            out.append(out[len(out) - off])


def chip_dump(chip: int, flags: int, ops: list[int], loop_pos: int = 0, compress: bool | None = None,
              length: int | None = None, data_offset: int = 28, cpu_speed: int = 50,
              chip_speed: int = 985248) -> bytes:
    """A CHIPTUNE dump: stored (flags bit 0) unless `compress`, in which case
    each 1024-byte chunk is a zx7 stream. `length` overrides the data length."""
    data = b"".join(struct.pack("<H", op) for op in ops)
    size = len(data) if length is None else length
    chunks = (size + 1023) // 1024
    last = size - (chunks - 1) * 1024 if chunks else 0
    if compress is None:
        compress = not flags & 1
    header = b"CHIPTUNE" + struct.pack("<HBBHHHHII", data_offset, chip, flags, chunks, last,
                                       loop_pos // 1024, loop_pos % 1024, cpu_speed, chip_speed)
    if compress:
        payload = b"".join(zx7_compress(data[i:i + 1024]) for i in range(0, len(data), 1024))
    else:
        payload = data
    return header + b"\0" * (data_offset - 28) + payload


def sid_tune() -> tuple[list[int], int]:
    """Every waveform, ring modulation, sync, the test bit, every filter
    type with a cutoff sweep and resonance, voice 3 muted, an attack from
    full level, a release from 0, and a waveform past 8. Returns the ops and
    the loop position (after the volume)."""
    ops: list[int] = []

    def w(reg: int, value: int) -> None:
        ops.append(reg << 8 | value)

    def wait(samples: int) -> None:
        while samples > 0:
            ops.append(0x8000 | min(samples, 0x7FFF))
            samples -= 0x7FFF

    w(24, 0x0F)
    loop = len(ops) * 2
    freqs = [0x1167, 0x1389, 0x15ED, 0x1A13, 0x1D45, 0x22D0, 0x2710, 0x0C8F]
    waves = [0x11, 0x21, 0x41, 0x81, 0x31, 0x51, 0x61, 0x71, 0x15, 0x23, 0x45, 0x91]
    for v in range(3):
        w(7 * v + 5, 0x19 + v * 0x22)
        w(7 * v + 6, 0xA4 - v * 0x31)
        w(7 * v + 2, 0x00)
        w(7 * v + 3, 0x08 - v * 2)
    for step, wave in enumerate(waves):
        for v in range(3):
            f = freqs[(step + 3 * v) % len(freqs)] >> v
            w(7 * v + 0, f & 0xFF)
            w(7 * v + 1, f >> 8)
            w(7 * v + 4, wave if v == step % 3 else (0x11, 0x41, 0x21)[v])
        wait(5000 + 700 * (step % 4))
        for v in range(3):
            w(7 * v + 4, (wave if v == step % 3 else 0x10) & 0xFE)
        wait(1500)
    # The test bit, held and released.
    w(4, 0x49)
    wait(800)
    w(4, 0x41)
    wait(2500)
    # Filters: every type over a cutoff sweep, all voices filtered.
    w(23, 0xF7)
    for kind in range(8):
        w(24, kind << 4 | 0x0F)
        for v in range(3):
            w(7 * v + 4, (0x21, 0x41, 0x81)[v])
        for cut in range(0, 256, 37):
            w(22, cut)
            w(21, cut & 7)
            wait(600)
        for v in range(3):
            w(7 * v + 4, (0x20, 0x40, 0x80)[v])
        wait(400)
    w(23, 0x03)
    w(24, 0x8F)  # voice 3 muted
    w(18, 0x21)
    wait(3000)
    w(24, 0x0F)
    # An attack from full level: gate off and on with no wait between.
    w(5, 0x00)
    w(6, 0xF0)
    w(4, 0x11)
    wait(4000)
    w(4, 0x10)
    w(4, 0x11)
    wait(6000)
    # A release from level 0: decay to 0, then gate off.
    w(12, 0x00)
    w(13, 0x00)
    w(11, 0x21)
    wait(8000)
    w(11, 0x20)
    wait(5000)
    for v in range(3):
        w(7 * v + 4, 0x00)
    wait(2000)
    return ops, loop


def ted_tune() -> tuple[list[int], int]:
    """Both oscillators, noise, volumes, the DC frequency 0x3FE, high
    frequency bits, and the digital mode."""
    ops: list[int] = []

    def w(reg: int, value: int) -> None:
        ops.append((64 + reg) << 8 | value)

    def wait(samples: int) -> None:
        ops.append(0x8000 | samples)

    w(3, 0x08)
    loop = len(ops) * 2
    for step in range(16):
        f1 = 0x300 + step * 23
        f2 = 0x280 + step * 31
        w(0, f1 & 0xFF)
        w(4, f1 >> 8)
        w(1, f2 & 0xFF)
        w(2, f2 >> 8)
        w(3, (0x10, 0x20, 0x30, 0x40, 0x50, 0x60)[step % 6] | (step % 9))
        wait(4000 + 300 * (step % 5))
    w(0, 0xFE)
    w(4, 0x03)  # 0x3FE: a constant level
    w(3, 0x18)
    wait(2000)
    w(4, 0xFF)  # bits past 0x300
    w(3, 0x38)
    wait(2000)
    for value in (0x98, 0xB8, 0x88, 0x38):
        w(3, value)  # digital mode
        wait(1500)
    w(3, 0x00)
    wait(1000)
    return ops, loop


def ay_tune(second_chip: bool = False, steps: int = 16) -> list[int]:
    """Every channel's tone, noise at several periods, every mixer setting
    used, fixed and enveloped volumes, every envelope shape, register
    numbers past 15, a pause longer than a frame's room, and a last op that
    writes. With `second_chip`, writes with bit 4 set drive a second chip.
    Waits are in ticks."""
    ops: list[int] = []

    def w(reg: int, value: int, chip2: bool = False) -> None:
        ops.append((reg | 0x10 if chip2 else reg) << 8 | value)

    def wait(ticks: int) -> None:
        ops.append(0x8000 | ticks)

    periods = [0x1AB, 0x17C, 0x152, 0x11D, 0x0FD, 0x0E1, 0x0BE, 0x0A9]
    mixers = [0x38, 0x30, 0x28, 0x18, 0x07, 0x00, 0x3E, 0x2A]
    for step in range(steps):
        for ch in range(3):
            period = periods[(step + 3 * ch) % 8] >> (ch % 2)
            w(2 * ch, period & 0xFF)
            w(2 * ch + 1, period >> 8 | 0xF0)  # the high bits are masked off
        w(6, (step * 3) % 40)  # noise period, past 31 masked
        w(7, mixers[step % 8])
        w(8, 0x0F - step % 4)
        w(9, 0x0A + step % 6)
        w(10, 0x10 if step % 2 else 0x08)
        w(11, (step * 37) & 0xFF)
        w(12, step % 3)
        w(13, step % 16)  # every envelope shape
        if step % 5 == 0:
            w(13, step % 16)  # a rewrite of 13 restarts the envelope
        if second_chip:
            w(0, periods[step % 8] & 0xFF, True)
            w(1, 0, True)
            w(7, 0x3C, True)
            w(8, 0x10 if step % 3 else 0x0C, True)
            w(13, (step + 5) % 16, True)
        if step == 7:
            w(0x2E, 0x55)  # an AY masks this to register 14; a YM ignores it
        wait(1 + step % 3)
    # A pause longer than a frame's room, the envelope running.
    w(10, 0x10)
    w(13, 0x0E)
    wait(30)
    w(8, 0)
    w(9, 0)
    wait(2)
    w(7, 0x3F)  # the last op writes (reference patch 0096)
    return ops


def ay_fixtures() -> dict[str, bytes]:
    tune = ay_tune()
    turbo = ay_tune(second_chip=True)
    clock = 1774400
    files = {
        "ay_ay.dump": chip_dump(1, 0x01, tune, chip_speed=clock),
        "ay_ym.zx7.dump": chip_dump(1, 0x40, tune, chip_speed=clock),
        "ay_turbo.dump": chip_dump(2, 0x41, turbo, chip_speed=clock),
        "ay_next.zx7.dump": chip_dump(3, 0x00, turbo, cpu_speed=60, chip_speed=2000000),
        # The slowest speeds the player renders (reference patch 0095).
        "ay_slow.dump": chip_dump(1, 0x01, ay_tune(steps=4), cpu_speed=11, chip_speed=8 * 44100),
        "ay_fast.zx7.dump": chip_dump(1, 0x40, tune * 4, cpu_speed=1000, chip_speed=3546900),
        # Over several chunks, with the loop position at the end, which Ay
        # does not use.
        "ay_long.zx7.dump": chip_dump(1, 0x00, ay_tune(steps=96), loop_pos=len(ay_tune(steps=96)) * 2,
                                      chip_speed=clock),
    }
    good = chip_dump(1, 0x01, tune, chip_speed=clock)
    files["bad_ay_chip_0.dump"] = chip_dump(0, 0x01, tune, chip_speed=clock)
    files["bad_ay_cpu_0.dump"] = chip_dump(1, 0x01, tune, cpu_speed=0, chip_speed=clock)
    files["bad_ay_cpu_10.dump"] = chip_dump(1, 0x01, tune, cpu_speed=10, chip_speed=clock)
    files["bad_ay_chip_speed.dump"] = chip_dump(1, 0x01, tune, chip_speed=8 * 44100 - 1)
    files["bad_ay_no_chunks.dump"] = good[:12] + b"\0\0" + good[14:]
    files["bad_ay_cut.dump"] = good[:-3]
    files["bad_ay_zx7_cut.dump"] = chip_dump(1, 0x40, tune, chip_speed=clock)[:-4]
    return files


def chipdump_fixtures() -> dict[str, bytes]:
    sid, sid_loop = sid_tune()
    ted, ted_loop = ted_tune()
    both = sid[:1] + ted[:1] + sid[1:] + ted[1:]
    files = {
        "sid_6581.dump": chip_dump(0, 0x01, sid, sid_loop),
        "sid_8580.zx7.dump": chip_dump(0, 0x10, sid, sid_loop),
        "sid_8580db.dump": chip_dump(0, 0x21, sid, sid_loop),
        "sid_6581r1.zx7.dump": chip_dump(0, 0x30, sid, sid_loop),
        "ted.dump": chip_dump(4, 0x01, ted, ted_loop),
        # Over several chunks, looping into the second, from an odd byte.
        "sidted.zx7.dump": chip_dump(5, 0x10, both, 1025),
        "sidted_6.dump": chip_dump(6, 0x01, both, 4, data_offset=40),
        # An odd length: the last op's high byte is 0.
        "odd_length.dump": chip_dump(0, 0x01, sid, sid_loop, length=len(sid) * 2 - 1),
    }
    good = chip_dump(0, 0x01, sid, sid_loop)
    files["bad_short.dump"] = good[:33]
    files["bad_magic.dump"] = b"CHIPTUNA" + good[8:]
    files["bad_chip_1.dump"] = chip_dump(1, 0x01, sid, sid_loop)
    files["bad_no_chunks.dump"] = good[:12] + b"\0\0" + good[14:]
    files["bad_loop_end.dump"] = chip_dump(0, 0x01, sid, len(sid) * 2)
    files["bad_data_offset.dump"] = good[:8] + struct.pack("<H", len(good) + 1) + good[10:]
    files["bad_cut.dump"] = good[:-3]
    files["bad_no_wait.dump"] = chip_dump(0, 0x01, [0x180F, 0x0411, 0x0122] * 4)
    files["bad_zero_waits.dump"] = chip_dump(0, 0x01, [0x8100, 0x180F, 0x8000, 0x0411], loop_pos=2)
    files["bad_zx7_overrun.dump"] = chip_dump(0, 0x00, [0] * 20, length=40)[:28] + zx7_compress(bytes(3001))
    packed = chip_dump(0, 0x10, sid, sid_loop)
    files["bad_zx7_cut.dump"] = packed[:-5]
    files.update(ay_fixtures())
    return files


def speech_fixtures() -> dict[str, bytes]:
    """Texts for `Speech::setText`, which reads bytes up to the first 0."""
    fox = b"The quick brown fox jumps over the lazy dog. "
    return {
        "sentence.txt": b"Hello, world! This is SoLouder's speech test: 42 cats, 3.14 pies, "
                        b"-7 degrees (and 1999 years); \"quoted\" words?\n",
        # Phonemes as they are, with stress marks and bytes that are no
        # phonemes, and an unclosed bracket.
        "phonemes.txt": b"[h@'l@U] ['wVn] [,tu:] [+Tri:] [S0p?] the [end",
        # Bytes from 0x80 (UTF-8), control bytes, spelled words, numbers
        # past the int range, and suffix rules.
        "odd.txt": "Caf\u00e9 na\u00efve \u2014 \u00dcber \u4e16\u754c".encode("utf-8")
                   + b"\tNASA McDonald x86_64 it's re-enter e.g. a.b\x0bc\x01d\x7f "
                   + b"12345678901234 -0 1000000 1000000000 2000000005 1100 1099 "
                   + b"jumping lovely closer runs used madness [unclosed\n",
        # Text after a 0 is not read.
        "stops_at_zero.txt": b"spoken\0 never spoken",
        # More than 128 elements.
        "long.txt": fox * 6,
    }


def vizsn_fixtures() -> dict[str, bytes]:
    """Texts for `Vizsn::setText`: CP437 Finnish, read up to the first 0."""
    return {
        # 0x84 and 0x94 are ä and ö, as are `{` and `|`; capitals, digits,
        # punctuation and bytes from 0x80 are pauses.
        "finnish.txt": b"Hyv\x84\x84 p\x84iv\x84\x84, mit\x84 kuuluu? t{m{ on s|p|, "
                       b"\x8e\x99 \xe4 kolme 3 - loppu.\0 ei luettu",
    }


def fixtures() -> dict[str, bytes]:
    out = {f"sfxr/{name}": data for name, data in sfxr_fixtures().items()}
    out.update({f"monotone/{name}": data for name, data in monotone_fixtures().items()})
    out.update({f"chipdump/{name}": data for name, data in chipdump_fixtures().items()})
    out.update({f"speech/{name}": data for name, data in speech_fixtures().items()})
    out.update({f"vizsn/{name}": data for name, data in vizsn_fixtures().items()})
    return out


GLOBS = ["sfxr/*.sfx", "monotone/*.mon", "chipdump/*.dump", "speech/*.txt", "vizsn/*.txt"]


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
