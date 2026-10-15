#!/usr/bin/env python3
"""Generate the Ogg Vorbis and MP3 fixtures in tests/fixtures/{ogg,mp3}/.

Source signals come from formulas (tones, sweeps, a pseudo-random noise
burst), written as WAV and encoded by ffmpeg with libvorbis, libmp3lame and
libtwolame. Encoder output depends on the encoder version, so the encoded
files are kept in the repository and only rewritten with --encode (which
needs ffmpeg on PATH or in $FFMPEG). Files named bad_* or derived from an
encoded file are made by editing its bytes in this script; without --encode
they are rebuilt from the committed files. With --check, fail if a derived
fixture differs from what this script makes or an encoded one is missing.
"""

from __future__ import annotations

import argparse
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = REPOSITORY_ROOT / "tests" / "fixtures"
BITEXACT = ["-fflags", "+bitexact", "-flags:a", "+bitexact", "-map_metadata", "-1"]


def signal(rate: int, channels: int, frames: int, seed: int) -> list[list[float]]:
    """Per channel: a tone, a sweep and a noise burst, each channel its own."""
    state = seed * 2654435761 % 2**32 or 1
    out: list[list[float]] = [[] for _ in range(channels)]
    for i in range(frames):
        t = i / rate
        state = (state * 1664525 + 1013904223) % 2**32
        noise = (state / 2**31 - 1.0) if (i // (rate // 8)) % 4 == 3 else 0.0
        for c in range(channels):
            tone = math.sin(2 * math.pi * (220 * (c + 1)) * t)
            sweep = math.sin(2 * math.pi * (50 + 3000 * t / max(frames / rate, 1e-9)) * t)
            env = min(1.0, i / (rate * 0.01)) * min(1.0, (frames - i) / (rate * 0.01))
            out[c].append(env * (0.35 * tone + 0.25 * sweep + 0.2 * noise))
    return out


def wav_bytes(rate: int, planes: list[list[float]]) -> bytes:
    channels, frames = len(planes), len(planes[0])
    data = bytearray()
    for i in range(frames):
        for c in range(channels):
            data += struct.pack("<h", max(-32768, min(32767, round(planes[c][i] * 32767))))
    header = struct.pack("<4sI4s4sIHHIIHH4sI", b"RIFF", 36 + len(data), b"WAVE", b"fmt ", 16, 1,
                         channels, rate, rate * channels * 2, channels * 2, 16, b"data", len(data))
    return header + bytes(data)


# name: (rate, channels, seconds, seed, ffmpeg codec arguments)
ENCODED = {
    "ogg/vorbis_stereo_44100.ogg": (44100, 2, 1.5, 1, ["-c:a", "libvorbis", "-q:a", "4"]),
    "ogg/vorbis_mono_22050_q0.ogg": (22050, 1, 1.0, 2, ["-c:a", "libvorbis", "-q:a", "0"]),
    "ogg/vorbis_5.1_48000.ogg": (48000, 6, 0.5, 3, ["-c:a", "libvorbis", "-q:a", "3"]),
    "ogg/vorbis_7.1_32000.ogg": (32000, 8, 0.4, 4, ["-c:a", "libvorbis", "-q:a", "2"]),
    "ogg/vorbis_quad_16000_q10.ogg": (16000, 4, 0.6, 5, ["-c:a", "libvorbis", "-q:a", "10"]),
    "ogg/vorbis_mono_8000_long.ogg": (8000, 1, 5.0, 6, ["-c:a", "libvorbis", "-q:a", "1"]),
    "ogg/vorbis_short_mono_44100.ogg": (44100, 1, 0.003, 7, ["-c:a", "libvorbis"]),
    "mp3/mpeg1_stereo_44100_cbr128.mp3": (44100, 2, 1.5, 11,
                                          ["-c:a", "libmp3lame", "-b:a", "128k", "-id3v2_version", "0"]),
    "mp3/mpeg1_stereo_48000_vbr_id3.mp3": (48000, 2, 1.0, 12,
                                           ["-c:a", "libmp3lame", "-q:a", "2", "-joint_stereo", "0"]),
    "mp3/mpeg1_mono_32000_noxing.mp3": (32000, 1, 0.8, 13,
                                        ["-c:a", "libmp3lame", "-b:a", "64k", "-write_xing", "0",
                                         "-id3v2_version", "0"]),
    "mp3/mpeg2_mono_22050_vbr.mp3": (22050, 1, 1.0, 14,
                                     ["-c:a", "libmp3lame", "-q:a", "6", "-id3v2_version", "0"]),
    "mp3/mpeg2_stereo_16000.mp3": (16000, 2, 0.7, 15,
                                   ["-c:a", "libmp3lame", "-b:a", "32k", "-id3v2_version", "0"]),
    "mp3/mpeg25_mono_8000.mp3": (8000, 1, 1.0, 16,
                                 ["-c:a", "libmp3lame", "-b:a", "16k", "-id3v2_version", "0"]),
    "mp3/mpeg25_stereo_11025_long.mp3": (11025, 2, 5.0, 17,
                                         ["-c:a", "libmp3lame", "-b:a", "32k", "-id3v2_version", "0"]),
    "mp3/layer2_stereo_44100.mp3": (44100, 2, 1.0, 18,
                                    ["-c:a", "libtwolame", "-b:a", "192k", "-f", "mp2"]),
    "mp3/layer2_mono_24000.mp3": (24000, 1, 0.6, 19,
                                  ["-c:a", "libtwolame", "-b:a", "64k", "-f", "mp2"]),
}


def ffmpeg() -> str:
    found = os.environ.get("FFMPEG") or shutil.which("ffmpeg")
    if not found:
        sys.exit("ffmpeg not found; set FFMPEG or put it on PATH")
    return found


def encode(name: str) -> bytes:
    rate, channels, seconds, seed, codec = ENCODED[name]
    source = wav_bytes(rate, signal(rate, channels, max(1, round(rate * seconds)), seed))
    with tempfile.TemporaryDirectory() as tmp:
        src, dst = Path(tmp) / "in.wav", Path(tmp) / ("out" + Path(name).suffix)
        src.write_bytes(source)
        command = [ffmpeg(), "-hide_banner", "-loglevel", "error", "-y", "-i", str(src),
                   *BITEXACT, *codec, str(dst)]
        subprocess.run(command, check=True)
        return dst.read_bytes()


def ogg_pages(data: bytes) -> list[tuple[int, int]]:
    """(offset, length) of each Ogg page."""
    pages, pos = [], 0
    while pos + 27 <= len(data) and data[pos:pos + 4] == b"OggS":
        segments = data[pos + 26]
        length = 27 + segments + sum(data[pos + 27:pos + 27 + segments])
        pages.append((pos, length))
        pos += length
    return pages


def ogg_crc(page: bytes) -> int:
    crc = 0
    for byte in page:
        crc ^= byte << 24
        for _ in range(8):
            crc = ((crc << 1) ^ 0x04C11DB7) if crc & 0x80000000 else crc << 1
            crc &= 0xFFFFFFFF
    return crc


def with_crc(page: bytearray) -> bytearray:
    page[22:26] = b"\0\0\0\0"
    page[22:26] = struct.pack("<I", ogg_crc(bytes(page)))
    return page


def derived(encoded: dict[str, bytes]) -> dict[str, bytes]:
    ogg = encoded["ogg/vorbis_stereo_44100.ogg"]
    pages = ogg_pages(ogg)
    headers_end = next(off for off, _ in pages if struct.unpack_from("<q", ogg, off + 6)[0] > 0)
    # A page in the middle with its payload damaged but a valid checksum, and
    # one with a wrong checksum: stb_vorbis does not check checksums when
    # reading in order.
    mid_off, mid_len = pages[len(pages) // 2]
    damaged = bytearray(ogg)
    page = bytearray(ogg[mid_off:mid_off + mid_len])
    body = 27 + page[26]
    for i in range(body + 7, len(page), 13):
        page[i] ^= 0x5A
    damaged[mid_off:mid_off + mid_len] = with_crc(page)
    bad_crc = bytearray(ogg)
    bad_crc[mid_off + 22] ^= 0xFF
    # The setup header (third packet) with its codebook sync pattern broken.
    setup = bytearray(ogg)
    setup_at = ogg.index(b"\x05vorbis")
    off, length = next((o, n) for o, n in pages if o <= setup_at < o + n)
    page = bytearray(setup[off:off + length])
    rel = setup_at - off
    page[rel + 1 + 6 + 1:rel + 1 + 6 + 4] = b"\0\0\0"
    setup[off:off + length] = with_crc(page)
    mp3 = encoded["mp3/mpeg1_stereo_44100_cbr128.mp3"]
    junk = bytes((i * 37 + 11) & 0xFF for i in range(700)).replace(b"\xff", b"\x7f")
    mp3_damaged = bytearray(mp3)
    for i in range(len(mp3) // 2, len(mp3) // 2 + 600, 3):
        mp3_damaged[i] ^= 0xA5
    return {
        "ogg/bad_truncated_stereo_44100.ogg": ogg[:len(ogg) * 3 // 5],
        "ogg/bad_payload_stereo_44100.ogg": bytes(damaged),
        "ogg/bad_crc_stereo_44100.ogg": bytes(bad_crc),
        "ogg/bad_headers_only.ogg": ogg[:headers_end],
        "ogg/bad_setup.ogg": bytes(setup),
        "ogg/bad_cut_header.ogg": ogg[:20],
        "mp3/bad_truncated_stereo_44100.mp3": mp3[:len(mp3) * 3 // 5 + 77],
        "mp3/bad_junk_prefix_stereo_44100.mp3": junk + mp3,
        "mp3/bad_payload_stereo_44100.mp3": bytes(mp3_damaged),
        "mp3/bad_not_audio.mp3": junk * 4,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--check", action="store_true", help="fail if fixtures differ")
    group.add_argument("--encode", action="store_true", help="re-encode with ffmpeg")
    args = parser.parse_args()
    encoded: dict[str, bytes] = {}
    missing = []
    for name in ENCODED:
        path = FIXTURE_DIR / name
        if args.encode:
            encoded[name] = encode(name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(encoded[name])
        elif path.exists():
            encoded[name] = path.read_bytes()
        else:
            missing.append(name)
    if missing:
        print(f"encoded fixtures missing: {missing}; run python scripts/gen_lossy_fixtures.py --encode")
        return 1
    files = derived(encoded)
    stale = []
    for name, data in files.items():
        path = FIXTURE_DIR / name
        if args.check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if args.check:
        known = set(files) | set(ENCODED)
        extra = sorted(f"{d}/{p.name}" for d in ("ogg", "mp3") for p in (FIXTURE_DIR / d).glob("*")
                       if f"{d}/{p.name}" not in known)
        if stale or extra:
            print(f"lossy fixtures out of date: {stale + extra}; run python scripts/gen_lossy_fixtures.py")
            return 1
        print(f"{len(files) + len(ENCODED)} Ogg Vorbis and MP3 fixtures up to date")
    else:
        print(f"wrote {len(files)} derived fixtures" + (f" and {len(ENCODED)} encoded" if args.encode else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
