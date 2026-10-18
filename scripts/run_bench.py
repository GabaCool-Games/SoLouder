#!/usr/bin/env python3
"""Time the render of every benchmark case in Rust and in the C++ reference.

Builds crates/solouder-bench and reference/tests' reference_bench in release,
runs both with the same options, and writes build/bench/rust.tsv,
build/bench/cpp.tsv and build/bench/bench.md: a table of mean, 99th
percentile and longest render per case and buffer size, Rust next to C++,
against the callback budget (the buffer's length at 48 kHz). Requires CMake,
a C++ compiler and the reference checkout from scripts/fetch_reference.py,
unless --rust-only.

The sources_* rows (the generated sources, feature `sources`) compare the
same voices on both sides. The streams_* rows do not compare the same work: the C++ reference decodes
inside mix, while Rust decodes on the stream worker's thread and renders at
the output's pace.
"""

from __future__ import annotations

import argparse
import csv
import os
from pathlib import Path
import subprocess
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
OUTPUT = REPOSITORY_ROOT / "build" / "bench"


def find(build_dir: Path, config: str, name: str) -> Path | None:
    for candidate in (build_dir / config / f"{name}.exe", build_dir / name, build_dir / f"{name}.exe"):
        if candidate.exists():
            return candidate
    return None


def run(command: list[str], output: Path) -> bool:
    print("+", " ".join(command), flush=True)
    result = subprocess.run(command, cwd=REPOSITORY_ROOT, capture_output=True, text=True)
    sys.stderr.write(result.stderr)
    if result.returncode != 0:
        return False
    output.write_text(result.stdout, encoding="utf-8")
    return True


def read(path: Path) -> dict[tuple[str, int], dict[str, str]]:
    if not path.exists():
        return {}
    with path.open(encoding="utf-8", newline="") as file:
        return {(row["case"], int(row["buffer"])): row for row in csv.DictReader(file, delimiter="\t")}


def table(rust: dict, cpp: dict) -> str:
    lines = [
        "| Case | Buffer | Budget µs | Rust mean | Rust p99 | Rust max | C++ mean | C++ p99 | C++ max "
        "| Rust/C++ mean | Worst max / budget | Note |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for key, row in rust.items():
        other = cpp.get(key)
        budget = float(row["budget_us"])
        maxima = [float(row["max_us"])] + ([float(other["max_us"])] if other else [])
        ratio = f"{float(row['mean_us']) / float(other['mean_us']):.2f}" if other else ""
        note = "; ".join(n for n in (row["note"], other["note"] if other else "") if n)
        cells = [key[0], str(key[1]), f"{budget:.0f}", row["mean_us"], row["p99_us"], row["max_us"],
                 other["mean_us"] if other else "", other["p99_us"] if other else "",
                 other["max_us"] if other else "", ratio, f"{100 * max(maxima) / budget:.1f}%", note]
        lines.append("| " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", default="2", help="seconds of output timed per case and buffer")
    parser.add_argument("--case", help="only cases whose name contains this")
    parser.add_argument("--buffers", help="comma-separated buffer sizes (default 128,256,480,512,1024)")
    parser.add_argument("--rust-only", action="store_true", help="skip the C++ reference")
    parser.add_argument("--reference", type=Path, default=REPOSITORY_ROOT / "reference" / "soloud")
    parser.add_argument("--build-dir", type=Path, default=REPOSITORY_ROOT / "build" / "reference")
    args = parser.parse_args(argv)
    options = ["--seconds", args.seconds]
    if args.case:
        options += ["--case", args.case]
    if args.buffers:
        options += ["--buffers", args.buffers]
    OUTPUT.mkdir(parents=True, exist_ok=True)

    build = ["cargo", "build", "--release", "-p", "solouder-bench", "--features", "sources"]
    if subprocess.run(build, cwd=REPOSITORY_ROOT).returncode != 0:
        return 1
    target = REPOSITORY_ROOT / os.environ.get("CARGO_TARGET_DIR", "target")
    rust_bench = target / "release" / "solouder-bench"
    if not run([str(rust_bench), *options], OUTPUT / "rust.tsv"):
        return 1

    (OUTPUT / "cpp.tsv").unlink(missing_ok=True)
    if not args.rust_only:
        reference = args.reference.expanduser().resolve()
        if not (reference / "contrib" / "CMakeLists.txt").exists():
            print(f"No SoLoud reference at {reference}. Run: python scripts/fetch_reference.py",
                  file=sys.stderr)
            return 1
        build_dir = args.build_dir.expanduser().resolve()
        configure = ["cmake", "--no-warn-unused-cli",
                     "-S", str(REPOSITORY_ROOT / "reference" / "tests"), "-B", str(build_dir),
                     f"-DSOLOUDER_REFERENCE_DIR={reference.as_posix()}", "-DCMAKE_BUILD_TYPE=Release"]
        compile_bench = ["cmake", "--build", str(build_dir), "--config", "Release",
                         "--target", "reference_bench"]
        for step in (configure, compile_bench):
            if subprocess.run(step, stdout=subprocess.DEVNULL).returncode != 0:
                return 1
        cpp_bench = find(build_dir, "Release", "reference_bench")
        if cpp_bench is None or not run([str(cpp_bench), *options], OUTPUT / "cpp.tsv"):
            print("reference_bench failed", file=sys.stderr)
            return 1

    report = table(read(OUTPUT / "rust.tsv"), read(OUTPUT / "cpp.tsv"))
    (OUTPUT / "bench.md").write_text(report, encoding="utf-8")
    print(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
