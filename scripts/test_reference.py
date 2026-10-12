#!/usr/bin/env python3
"""Build the C++ reference regression tests and run them with CTest.

Requires CMake and a C++ compiler (Visual Studio 2022 on Windows). Builds into
build/reference, using reference/soloud unless --reference points elsewhere.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, default=REPOSITORY_ROOT / "reference" / "soloud",
                        help="SoLoud checkout to test (default: reference/soloud)")
    parser.add_argument("--build-dir", type=Path, default=REPOSITORY_ROOT / "build" / "reference")
    parser.add_argument("--config", default="Release")
    args = parser.parse_args(argv)

    reference = args.reference.expanduser().resolve()
    if not (reference / "contrib" / "CMakeLists.txt").exists():
        print(f"No SoLoud reference at {reference}. Run: python scripts/fetch_reference.py",
              file=sys.stderr)
        return 1
    build_dir = args.build_dir.expanduser().resolve()
    steps = [
        ["cmake", "-S", str(REPOSITORY_ROOT / "reference" / "tests"), "-B", str(build_dir),
         f"-DSOLOUDER_REFERENCE_DIR={reference.as_posix()}"],
        ["cmake", "--build", str(build_dir), "--config", args.config],
        ["ctest", "--test-dir", str(build_dir), "-C", args.config, "--output-on-failure"],
    ]
    for step in steps:
        if subprocess.run(step).returncode != 0:
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
