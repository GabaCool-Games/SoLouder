#!/usr/bin/env python3
"""Generate the WAV fixtures in tests/fixtures/wav/.

Every file is built byte by byte from integer formulas, so the output is
deterministic and the expected samples are known exactly. With --check, fail
if a committed fixture differs from what this script generates.
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path
import struct
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = REPOSITORY_ROOT / "tests" / "fixtures" / "wav"

PCM = 0x0001
IEEE_FLOAT = 0x0003
ALAW = 0x0006
MULAW = 0x0007
EXTENSIBLE = 0xFFFE
GUID_TAIL = bytes([0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71])


def chunk(tag: bytes, body: bytes, declared: int | None = None) -> bytes:
    size = len(body) if declared is None else declared
    pad = b"\0" if len(body) % 2 else b""
    return tag + struct.pack("<I", size) + body + pad


def fmt_chunk(tag: int, channels: int, rate: int, bits: int, *, cb_size: bool = False) -> bytes:
    block = (channels * bits // 8) & 0xFFFF
    body = struct.pack("<HHIIHH", tag, channels, rate, (rate * block) & 0xFFFFFFFF, block, bits)
    if cb_size:
        body += struct.pack("<H", 0)
    return chunk(b"fmt ", body)


def fmt_extensible(sub_format: int, channels: int, rate: int, bits: int, valid_bits: int, mask: int) -> bytes:
    block = (channels * bits // 8) & 0xFFFF
    body = struct.pack("<HHIIHH", EXTENSIBLE, channels, rate, (rate * block) & 0xFFFFFFFF, block, bits)
    body += struct.pack("<HHI", 22, valid_bits, mask) + struct.pack("<H", sub_format) + GUID_TAIL
    return chunk(b"fmt ", body)


def riff(*chunks: bytes) -> bytes:
    body = b"WAVE" + b"".join(chunks)
    return b"RIFF" + struct.pack("<I", len(body)) + body


def lcg(seed: int):
    state = seed
    while True:
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        yield state


def int_frames(frames: int, channels: int, bits: int, seed: int) -> list[list[int]]:
    """Signed samples: full-scale extremes first, then a ramp and noise."""
    lo, hi = -(1 << (bits - 1)), (1 << (bits - 1)) - 1
    noise = lcg(seed)
    out = []
    for i in range(frames):
        frame = []
        for c in range(channels):
            if i < 4:
                value = [lo, hi, 0, -1][(i + c) % 4]
            elif i % 3 == 0:
                value = lo + (i * 7919 + c * 104729) % (hi - lo + 1)
            else:
                value = next(noise) % (hi - lo + 1) + lo
            frame.append(value)
        out.append(frame)
    return out


def pack_ints(frames: list[list[int]], bits: int, container: int | None = None) -> bytes:
    container = container or bits
    shift = container - bits
    data = bytearray()
    for frame in frames:
        for value in frame:
            data += (value << shift).to_bytes(container // 8, "little", signed=True)
    return bytes(data)


def float_frames(frames: int, channels: int, seed: int) -> list[list[float]]:
    noise = lcg(seed)
    specials = [1.0, -1.0, 1.5, -2.25, 0.0, -0.0, 1e-30, 3.0e-39, 0.1, 1 / 3]
    out = []
    for i in range(frames):
        frame = []
        for c in range(channels):
            if i < len(specials):
                value = specials[(i + c) % len(specials)]
            elif i % 2:
                value = math.sin(i * 0.01 * (c + 1)) * 0.9
            else:
                value = (next(noise) / 0x3FFFFFFF) - 1.0
            frame.append(value)
        out.append(frame)
    return out


def pack_floats(frames: list[list[float]], code: str) -> bytes:
    return b"".join(struct.pack("<" + code * len(frame), *frame) for frame in frames)


def fixtures() -> dict[str, bytes]:
    files: dict[str, bytes] = {}

    u8 = bytes((i * 37 + (i >> 8)) & 0xFF for i in range(1000))
    files["u8_mono_8000.wav"] = riff(fmt_chunk(PCM, 1, 8000, 8), chunk(b"data", u8))

    s16 = pack_ints(int_frames(1500, 2, 16, 1), 16)
    files["s16_stereo_44100.wav"] = riff(fmt_chunk(PCM, 2, 44100, 16), chunk(b"data", s16))

    s24 = pack_ints(int_frames(700, 1, 24, 2), 24)
    files["s24_mono_48000.wav"] = riff(fmt_chunk(PCM, 1, 48000, 24), chunk(b"data", s24))

    s32 = pack_ints(int_frames(600, 2, 32, 3), 32)
    files["s32_stereo_22050.wav"] = riff(fmt_chunk(PCM, 2, 22050, 32), chunk(b"data", s32))

    f32 = pack_floats(float_frames(520, 4, 4), "f")
    files["f32_quad_48000.wav"] = riff(fmt_chunk(IEEE_FLOAT, 4, 48000, 32, cb_size=True), chunk(b"data", f32))

    f64 = pack_floats(float_frames(300, 1, 5), "d")
    files["f64_mono_96000.wav"] = riff(fmt_chunk(IEEE_FLOAT, 1, 96000, 64, cb_size=True), chunk(b"data", f64))

    codes = bytes(range(256)) * 2
    files["alaw_mono_8000.wav"] = riff(fmt_chunk(ALAW, 1, 8000, 8, cb_size=True), chunk(b"data", codes))
    files["mulaw_mono_8000.wav"] = riff(fmt_chunk(MULAW, 1, 8000, 8, cb_size=True), chunk(b"data", codes))

    ext16 = pack_ints(int_frames(400, 6, 16, 6), 16)
    files["ext_s16_5.1_48000.wav"] = riff(fmt_extensible(PCM, 6, 48000, 16, 16, 0x3F), chunk(b"data", ext16))

    ext_f32 = pack_floats(float_frames(300, 8, 7), "f")
    files["ext_f32_7.1_44100.wav"] = riff(
        fmt_extensible(IEEE_FLOAT, 8, 44100, 32, 32, 0x63F), chunk(b"data", ext_f32)
    )

    ext24in32 = pack_ints(int_frames(256, 2, 24, 8), 24, container=32)
    files["ext_s24in32_stereo_32000.wav"] = riff(
        fmt_extensible(PCM, 2, 32000, 32, 24, 0x3), chunk(b"data", ext24in32)
    )

    extra = pack_ints(int_frames(333, 2, 16, 9), 16)
    info = chunk(b"LIST", b"INFO" + chunk(b"INAM", b"fixture\0"))
    files["chunks_s16_stereo_11025.wav"] = riff(
        chunk(b"junk", b"odd"),
        fmt_chunk(PCM, 2, 11025, 16, cb_size=True),
        info,
        chunk(b"data", extra),
        chunk(b"cue ", struct.pack("<I", 0)),
    )

    # The data chunk (and RIFF size) claim 1000 frames; the file ends after 600.
    short = riff(fmt_chunk(PCM, 1, 22050, 16), chunk(b"data", pack_ints(int_frames(1000, 1, 16, 12), 16)))
    files["short_data_s16_mono_22050.wav"] = short[: len(short) - 800]

    nine = pack_ints(int_frames(10, 9, 16, 10), 16)
    files["bad_nine_channels.wav"] = riff(fmt_chunk(PCM, 9, 44100, 16), chunk(b"data", nine))
    files["bad_no_frames.wav"] = riff(fmt_chunk(PCM, 1, 44100, 16), chunk(b"data", b""))
    files["bad_zero_rate.wav"] = riff(fmt_chunk(PCM, 1, 0, 16), chunk(b"data", pack_ints(int_frames(8, 1, 16, 11), 16)))
    files["bad_cut_header.wav"] = riff(fmt_chunk(PCM, 2, 44100, 16))[:30]

    # Extensible companded audio, every code twice.
    files["ext_alaw_mono_8000.wav"] = riff(fmt_extensible(ALAW, 1, 8000, 8, 8, 0x4), chunk(b"data", codes))
    files["ext_mulaw_stereo_8000.wav"] = riff(fmt_extensible(MULAW, 2, 8000, 8, 8, 0x3), chunk(b"data", codes))
    # Channel masks are ignored: a single high bit, and none at all.
    odd = pack_ints(int_frames(64, 2, 16, 13), 16)
    files["ext_mask_high_s16_stereo_8000.wav"] = riff(fmt_extensible(PCM, 2, 8000, 16, 16, 0x80000000), chunk(b"data", odd))
    files["ext_mask_zero_s16_stereo_8000.wav"] = riff(fmt_extensible(PCM, 2, 8000, 16, 16, 0), chunk(b"data", odd))
    # Headers whose arithmetic overflows 16 bits, or claim more channels than a mask holds.
    files["bad_65535_channels.wav"] = riff(fmt_chunk(PCM, 65535, 8000, 32), chunk(b"data", bytes(64)))
    files["bad_33_channels_ext.wav"] = riff(fmt_extensible(PCM, 33, 8000, 16, 16, 0), chunk(b"data", bytes(66 * 4)))
    return files


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify instead of writing")
    args = parser.parse_args(argv)

    expected = fixtures()
    if args.check:
        stale = [name for name, data in expected.items()
                 if not (FIXTURE_DIR / name).is_file() or (FIXTURE_DIR / name).read_bytes() != data]
        extra = sorted(p.name for p in FIXTURE_DIR.glob("*.wav") if p.name not in expected)
        if stale or extra:
            print("WAV fixtures are out of date; run python scripts/gen_wav_fixtures.py", file=sys.stderr)
            for name in stale + extra:
                print(f"  {name}", file=sys.stderr)
            return 1
        print(f"WAV fixtures are up to date ({len(expected)} files).")
        return 0

    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    for name, data in expected.items():
        (FIXTURE_DIR / name).write_bytes(data)
    print(f"Wrote {len(expected)} fixtures to {FIXTURE_DIR.relative_to(REPOSITORY_ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
