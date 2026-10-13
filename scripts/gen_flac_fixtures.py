#!/usr/bin/env python3
"""Generate the FLAC fixtures in tests/fixtures/flac/.

A small FLAC encoder written for tests: every subframe type (constant,
verbatim, fixed and LPC), every stereo decorrelation, both residual coding
methods with escaped partitions, wasted bits, fixed and variable block sizes,
coded and STREAMINFO sample rates and sizes, and the metadata blocks a decoder
must skip. Samples come from integer formulas, so the output is deterministic.
With --check, fail if a committed fixture differs from what this script
generates.
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import struct
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = REPOSITORY_ROOT / "tests" / "fixtures" / "flac"

INDEPENDENT, LEFT_SIDE, RIGHT_SIDE, MID_SIDE = "independent", "left_side", "right_side", "mid_side"
RATE_CODES = {88200: 1, 176400: 2, 192000: 3, 8000: 4, 16000: 5, 22050: 6, 24000: 7,
              32000: 8, 44100: 9, 48000: 10, 96000: 11}
SIZE_CODES = {8: 1, 12: 2, 16: 4, 20: 5, 24: 6}


class Bits:
    def __init__(self) -> None:
        self.out = bytearray()
        self.acc = 0
        self.count = 0

    def put(self, value: int, bits: int) -> None:
        assert bits == 0 or 0 <= value < (1 << bits), (value, bits)
        self.acc = (self.acc << bits) | value
        self.count += bits
        while self.count >= 8:
            self.count -= 8
            self.out.append((self.acc >> self.count) & 0xFF)
        self.acc &= (1 << self.count) - 1

    def signed(self, value: int, bits: int) -> None:
        assert -(1 << (bits - 1)) <= value < (1 << (bits - 1)), (value, bits)
        self.put(value & ((1 << bits) - 1), bits)

    def unary(self, zeros: int) -> None:
        for _ in range(zeros // 32):
            self.put(0, 32)
        self.put(1, zeros % 32 + 1)

    def align(self) -> None:
        if self.count:
            self.put(0, 8 - self.count)

    def data(self) -> bytes:
        assert self.count == 0
        return bytes(self.out)


def crc8(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = ((crc << 1) ^ 0x07) & 0xFF if crc & 0x80 else (crc << 1) & 0xFF
    return crc


def crc16(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x8005) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


def utf8_number(value: int) -> bytes:
    if value < 0x80:
        return bytes([value])
    for length in range(2, 8):
        if value < 1 << (5 * length + 1):
            break
    out = []
    for _ in range(length - 1):
        out.append(0x80 | (value & 0x3F))
        value >>= 6
    lead = (0xFF << (8 - length)) & 0xFF
    return bytes([lead | value] + out[::-1])


# Samples ---------------------------------------------------------------------


def lcg(seed: int):
    state = seed
    while True:
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        yield state


def signal(frames: int, bits: int, seed: int, kind: str) -> list[int]:
    """Integer audio in range for `bits`: a smooth wave with some noise."""
    top = (1 << (bits - 1)) - 1
    noise = lcg(seed)
    out, smooth = [], 0
    period = 50 + seed % 37
    for i in range(frames):
        phase = i % period
        tri = (4 * phase - 2 * period) if phase < period // 2 else (2 * period - 4 * (phase - period // 2) - period)
        wave = tri * top // (2 * period)
        smooth = (smooth * 7 + (next(noise) % 2001 - 1000)) // 8
        if kind == "noise":
            value = (next(noise) % (2 * top + 1)) - top
        elif kind == "quiet":
            value = smooth // 64
        else:
            value = wave // 2 + smooth * top // 4000
        out.append(max(-top - 1, min(top, value)))
    return out


# Subframes -------------------------------------------------------------------

FIXED = [[], [1], [2, -1], [3, -3, 1], [4, -6, 4, -1]]


def residual_fixed(samples: list[int], order: int) -> list[int]:
    coeffs = FIXED[order]
    return [samples[n] - sum(c * samples[n - 1 - i] for i, c in enumerate(coeffs))
            for n in range(order, len(samples))]


def residual_lpc(samples: list[int], coeffs: list[int], shift: int) -> list[int]:
    order = len(coeffs)
    return [samples[n] - (sum(c * samples[n - 1 - i] for i, c in enumerate(coeffs)) >> shift)
            for n in range(order, len(samples))]


def zigzag(value: int) -> int:
    return 2 * value if value >= 0 else -2 * value - 1


def write_residual(bits: Bits, residual: list[int], block: int, order: int, method: int,
                   partition_order: int, escape: set[int]) -> None:
    bits.put(method, 2)
    bits.put(partition_order, 4)
    param_bits = 4 if method == 0 else 5
    escape_code = (1 << param_bits) - 1
    at = 0
    for partition in range(1 << partition_order):
        count = (block >> partition_order) - (order if partition == 0 else 0)
        values = residual[at:at + count]
        at += count
        if partition in escape:
            width = max((v.bit_length() + 1 for v in values), default=0)
            width = max(width, 1) if any(values) else 0
            bits.put(escape_code, param_bits)
            bits.put(width, 5)
            for v in values:
                if width:
                    bits.signed(v, width)
            continue
        mean = sum(zigzag(v) for v in values) // max(len(values), 1)
        param = min(max(mean.bit_length() - 1, 0), escape_code - 1)
        bits.put(param, param_bits)
        for v in values:
            u = zigzag(v)
            bits.unary(u >> param)
            if param:
                bits.put(u & ((1 << param) - 1), param)
    assert at == len(residual)


def write_subframe(bits: Bits, samples: list[int], width: int, plan: dict) -> None:
    wasted = plan.get("wasted", 0)
    if wasted:
        assert all(s % (1 << wasted) == 0 for s in samples)
        samples = [s >> wasted for s in samples]
        width -= wasted
    kind = plan["type"]
    block = len(samples)
    code = {"constant": 0, "verbatim": 1}.get(kind)
    if kind == "fixed":
        code = 8 | plan["order"]
    elif kind == "lpc":
        code = 32 | (len(plan["coeffs"]) - 1)
    bits.put(0, 1)
    bits.put(code, 6)
    bits.put(1 if wasted else 0, 1)
    if wasted:
        bits.unary(wasted - 1)
    if kind == "constant":
        assert len(set(samples)) == 1
        bits.signed(samples[0], width)
        return
    if kind == "verbatim":
        for s in samples:
            bits.signed(s, width)
        return
    method, partition_order = plan.get("method", 0), plan.get("partitions", 0)
    escape = set(plan.get("escape", ()))
    if kind == "fixed":
        order = plan["order"]
        for s in samples[:order]:
            bits.signed(s, width)
        write_residual(bits, residual_fixed(samples, order), block, order, method, partition_order, escape)
        return
    coeffs, precision, shift = plan["coeffs"], plan["precision"], plan["shift"]
    order = len(coeffs)
    for s in samples[:order]:
        bits.signed(s, width)
    bits.put(precision - 1, 4)
    bits.signed(shift, 5)
    for c in coeffs:
        bits.signed(c, precision)
    write_residual(bits, residual_lpc(samples, coeffs, shift), block, order, method, partition_order, escape)


# Frames and files ------------------------------------------------------------


def decorrelate(left: list[int], right: list[int], mode: str) -> tuple[list[int], list[int], int, int]:
    """The two subframes of a stereo frame and their extra sample bits."""
    side = [l - r for l, r in zip(left, right)]
    if mode == LEFT_SIDE:
        return left, side, 0, 1
    if mode == RIGHT_SIDE:
        return side, right, 1, 0
    if mode == MID_SIDE:
        return [(l + r) >> 1 for l, r in zip(left, right)], side, 0, 1
    return left, right, 0, 0


def frame(channels: list[list[int]], start: int, index: int, info: dict, spec: dict) -> bytes:
    block = len(channels[0])
    bits = Bits()
    bits.put(0x3FFE, 14)
    bits.put(0, 1)
    bits.put(1 if info["variable"] else 0, 1)
    size_code = {192: 1, 576: 2, 1152: 3, 2304: 4, 4608: 5, 256: 8, 512: 9, 1024: 10, 2048: 11,
                 4096: 12, 8192: 13, 16384: 14, 32768: 15}.get(block)
    if size_code is None or spec.get("block_code"):
        size_code = 6 if block <= 256 else 7
    bits.put(size_code, 4)
    rate = info["rate"]
    rate_code = 0
    if spec.get("rate_code") == "table":
        rate_code = RATE_CODES[rate]
    elif spec.get("rate_code") == "khz":
        rate_code = 12
    elif spec.get("rate_code") == "hz":
        rate_code = 13
    elif spec.get("rate_code") == "tens":
        rate_code = 14
    bits.put(rate_code, 4)
    mode = spec.get("mode", INDEPENDENT)
    assignment = {LEFT_SIDE: 8, RIGHT_SIDE: 9, MID_SIDE: 10}.get(mode, len(channels) - 1)
    bits.put(assignment, 4)
    bits.put(SIZE_CODES[info["bits"]] if spec.get("size_code") else 0, 3)
    bits.put(0, 1)
    header = bits.data() + utf8_number(start if info["variable"] else index)
    if size_code == 6:
        header += bytes([block - 1])
    elif size_code == 7:
        header += struct.pack(">H", block - 1)
    if rate_code == 12:
        header += bytes([rate // 1000])
    elif rate_code == 13:
        header += struct.pack(">H", rate)
    elif rate_code == 14:
        header += struct.pack(">H", rate // 10)
    header += bytes([crc8(header)])

    bits = Bits()
    subframes = list(channels)
    extra = [0] * len(channels)
    if len(channels) == 2:
        a, b, ea, eb = decorrelate(channels[0], channels[1], mode)
        subframes, extra = [a, b], [ea, eb]
    if "raw" in spec:
        # Subframe samples as given, out of range or not.
        subframes = spec["raw"]
    plans = spec["subframes"]
    for c, samples in enumerate(subframes):
        write_subframe(bits, samples, info["bits"] + extra[c], plans[c % len(plans)])
    bits.align()
    body = header + bits.data()
    crc = crc16(body)
    if spec.get("bad_crc"):
        crc ^= 0x5A5A
    return body + struct.pack(">H", crc)


def metadata_block(kind: int, body: bytes, last: bool) -> bytes:
    return bytes([(0x80 if last else 0) | kind]) + len(body).to_bytes(3, "big") + body


def md5_of(channels: list[list[int]], bits: int) -> bytes:
    width = (bits + 7) // 8
    data = bytearray()
    for i in range(len(channels[0])):
        for ch in channels:
            data += (ch[i] & ((1 << (8 * width)) - 1)).to_bytes(width, "little")
    return hashlib.md5(bytes(data)).digest()


def flac(info: dict, channels: list[list[int]], blocks: list[dict], extra_meta: list[tuple[int, bytes]] = (),
         total: int | None = None) -> bytes:
    frames = len(channels[0])
    sizes = [spec["frames"] for spec in blocks]
    assert sum(sizes) == frames, (sum(sizes), frames)
    block_max = max(sizes)
    block_min = min(sizes[:-1] or sizes) if info["variable"] else block_max
    streaminfo = struct.pack(">HH", block_min, block_max) + bytes(6)
    declared = frames if total is None else total
    packed = (info["rate"] << 44) | ((len(channels) - 1) << 41) | ((info["bits"] - 1) << 36) | declared
    streaminfo += packed.to_bytes(8, "big") + md5_of(channels, info["bits"])
    meta = [(0, streaminfo)] + list(extra_meta)
    out = bytearray(b"fLaC")
    for i, (kind, body) in enumerate(meta):
        out += metadata_block(kind, body, i == len(meta) - 1)
    start = 0
    for index, spec in enumerate(blocks):
        block = [ch[start:start + spec["frames"]] for ch in channels]
        out += frame(block, start, index, info, spec)
        start += spec["frames"]
    return bytes(out)


# Fixtures --------------------------------------------------------------------

LPC8 = {"type": "lpc", "coeffs": [3000, -1900, 700, -200, 100, -60, 30, -10], "precision": 13, "shift": 11}


def fixtures() -> dict[str, bytes]:
    files: dict[str, bytes] = {}

    # 16-bit stereo: every stereo mode, LPC and fixed subframes, Rice method 0.
    left, right = signal(5108, 16, 1, "wave"), signal(5108, 16, 2, "wave")
    blocks = [
        {"frames": 1152, "mode": INDEPENDENT, "subframes": [dict(LPC8, partitions=2), {"type": "fixed", "order": 2}]},
        {"frames": 1152, "mode": LEFT_SIDE, "subframes": [{"type": "fixed", "order": 1, "partitions": 3}]},
        {"frames": 1152, "mode": RIGHT_SIDE, "subframes": [dict(LPC8, method=1)], "size_code": True},
        {"frames": 1152, "mode": MID_SIDE, "subframes": [{"type": "fixed", "order": 3}], "rate_code": "table"},
        {"frames": 500, "mode": MID_SIDE, "subframes": [{"type": "verbatim"}]},
    ]
    files["s16_stereo_44100.flac"] = flac({"rate": 44100, "bits": 16, "variable": False}, [left, right], blocks)

    # 24-bit mono: long LPC orders, Rice method 1, deep partitions, escaped
    # partitions and a frame with wasted bits.
    mono = signal(4096 * 2 + 1000, 24, 3, "wave")
    mono[4096:8192] = [s & ~0xFF for s in mono[4096:8192]]
    lpc32 = {"type": "lpc", "coeffs": [1800 - 50 * i if i % 2 == 0 else -(900 - 25 * i) for i in range(32)],
             "precision": 15, "shift": 12, "method": 1, "partitions": 6, "escape": [3, 17]}
    blocks = [
        {"frames": 4096, "subframes": [lpc32]},
        {"frames": 4096, "subframes": [dict(LPC8, wasted=8, partitions=4, escape=[0])], "size_code": True},
        {"frames": 1000, "subframes": [{"type": "lpc", "coeffs": [1], "precision": 2, "shift": 0, "method": 1}],
         "block_code": True},
    ]
    files["s24_mono_48000.flac"] = flac({"rate": 48000, "bits": 24, "variable": False}, [mono], blocks)

    # 8-bit quad: constant and verbatim subframes, coded sizes and rates.
    quad = [signal(1500, 8, 10 + c, "wave") for c in range(4)]
    quad[1][:576] = [-17] * 576
    blocks = [
        {"frames": 576, "subframes": [{"type": "verbatim"}, {"type": "constant"}, {"type": "verbatim"},
                                      {"type": "fixed", "order": 1}], "size_code": True,
         "rate_code": "table"},
        {"frames": 576, "subframes": [{"type": "fixed", "order": 4}], "rate_code": "khz"},
        {"frames": 348, "subframes": [{"type": "fixed", "order": 0}], "rate_code": "hz"},
    ]
    files["s8_quad_8000.flac"] = flac({"rate": 8000, "bits": 8, "variable": False}, quad, blocks)

    # 12-bit 5.1 and 20-bit stereo with variable block sizes and long sample
    # numbers.
    six = [signal(2304 + 777, 12, 20 + c, "wave") for c in range(6)]
    blocks = [
        {"frames": 2304, "subframes": [{"type": "fixed", "order": c % 5} for c in range(6)], "size_code": True},
        {"frames": 777, "subframes": [dict(LPC8, partitions=0)], "rate_code": "tens"},
    ]
    files["s12_5.1_22050.flac"] = flac({"rate": 22050, "bits": 12, "variable": False}, six, blocks)
    left, right = signal(6000, 20, 30, "wave"), signal(6000, 20, 31, "noise")
    sizes = [192, 4608, 37, 1000, 163]
    blocks = [{"frames": n, "mode": [MID_SIDE, LEFT_SIDE, RIGHT_SIDE][i % 3],
               "subframes": [dict(LPC8, method=1, partitions=0)], "size_code": i % 2 == 0} for i, n in enumerate(sizes)]
    files["s20_stereo_96000_variable.flac"] = flac({"rate": 96000, "bits": 20, "variable": True}, [left, right], blocks)

    # 7.1 after every metadata block a decoder must skip.
    eight = [signal(1200, 16, 40 + c, "quiet") for c in range(8)]
    comment = struct.pack("<I", 4) + b"test" + struct.pack("<I", 1) + struct.pack("<I", 9) + b"TITLE=abc"
    picture = struct.pack(">II", 3, 9) + b"image/png" + struct.pack(">I", 0) + struct.pack(">IIIII", 1, 1, 24, 0, 4) + b"\x89PNG"
    seek = struct.pack(">QQH", 0, 0, 1200) + struct.pack(">QQH", 0xFFFFFFFFFFFFFFFF, 0, 0)
    meta = [(1, bytes(100)), (2, b"TEST" + bytes(10)), (3, seek), (4, comment), (6, picture)]
    blocks = [{"frames": 1200, "subframes": [{"type": "fixed", "order": 2, "partitions": 2}]}]
    files["s16_7.1_32000_metadata.flac"] = flac({"rate": 32000, "bits": 16, "variable": False}, eight, blocks, meta)

    # More than 24 bits: 32-bit mono and 28-bit stereo.
    big = signal(1500, 32, 50, "wave")
    blocks = [{"frames": 1024, "subframes": [{"type": "verbatim"}]},
              {"frames": 476, "subframes": [{"type": "fixed", "order": 1, "method": 1}]}]
    files["s32_mono_44100.flac"] = flac({"rate": 44100, "bits": 32, "variable": False}, [big], blocks)
    left, right = signal(1500, 28, 51, "wave"), signal(1500, 28, 52, "wave")
    blocks = [{"frames": 1024, "mode": INDEPENDENT, "subframes": [{"type": "fixed", "order": 2, "method": 1}]},
              {"frames": 476, "mode": MID_SIDE, "subframes": [{"type": "fixed", "order": 2, "method": 1}]}]
    files["s28_stereo_44100.flac"] = flac({"rate": 44100, "bits": 28, "variable": False}, [left, right], blocks)

    # A long file of small frames without a seek table, for seeking.
    long = signal(32 * 1152, 8, 70, "wave")
    blocks = [{"frames": 1152, "subframes": [{"type": "fixed", "order": 2, "partitions": 2}]} for _ in range(32)]
    files["s8_mono_8000_long.flac"] = flac({"rate": 8000, "bits": 8, "variable": False}, [long], blocks)

    # Damaged and unusual files.
    left = signal(4096, 16, 60, "wave")
    blocks = [{"frames": 1024, "subframes": [{"type": "fixed", "order": 2}], "bad_crc": i == 1} for i in range(4)]
    files["bad_crc_s16_mono_44100.flac"] = flac({"rate": 44100, "bits": 16, "variable": False}, [left], blocks)
    blocks = [{"frames": 1024, "subframes": [{"type": "fixed", "order": 2}]} for _ in range(4)]
    whole = flac({"rate": 44100, "bits": 16, "variable": False}, [left], blocks)
    files["short_data_s16_mono_44100.flac"] = whole[: len(whole) - 1500]
    files["unknown_length_s16_mono_44100.flac"] = flac({"rate": 44100, "bits": 16, "variable": False}, [left],
                                                       blocks, total=0)
    files["long_claim_s16_mono_44100.flac"] = flac({"rate": 44100, "bits": 16, "variable": False}, [left],
                                                   blocks, total=10_000)
    files["bad_cut_header.flac"] = whole[:20]
    no_rate = bytearray(whole)
    no_rate[18:21] = bytes([0, 0, no_rate[20] & 0x0F])
    files["bad_zero_rate.flac"] = bytes(no_rate)
    # Mid and side subframes far beyond 16 bits: decorrelating them overflows
    # 32-bit arithmetic.
    ramp = [i << 20 for i in range(4096)]
    blocks = [{"frames": 4096, "mode": MID_SIDE, "raw": [ramp, ramp],
               "subframes": [{"type": "fixed", "order": 1, "method": 1}]}]
    files["bad_overflow_s16_stereo_44100.flac"] = flac({"rate": 44100, "bits": 16, "variable": False},
                                                       [[0] * 4096, [0] * 4096], blocks)
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if committed fixtures differ")
    args = parser.parse_args()
    files = fixtures()
    stale = []
    if not args.check:
        FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    for name, data in files.items():
        path = FIXTURE_DIR / name
        if args.check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if args.check:
        extra = sorted(p.name for p in FIXTURE_DIR.glob("*.flac") if p.name not in files)
        if stale or extra:
            print(f"FLAC fixtures out of date: {stale + extra}; run python scripts/gen_flac_fixtures.py")
            return 1
        print(f"{len(files)} FLAC fixtures up to date")
    else:
        print(f"wrote {len(files)} FLAC fixtures to {FIXTURE_DIR}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
