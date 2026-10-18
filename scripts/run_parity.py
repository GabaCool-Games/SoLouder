#!/usr/bin/env python3
"""Render every parity scenario with the C++ reference and compare with Rust.

Builds reference/tests (including parity_runner) into build/reference, renders
tests/scenarios/*.scn into build/parity/cpp, then runs the solouder-parity
tests with SOLOUDER_REQUIRE_PARITY=1 so a scenario without C++ output fails.
Requires CMake, a C++ compiler, and the reference checkout from
scripts/fetch_reference.py.

With --cpp-dir, nothing is built: Rust is compared with C++ output rendered
elsewhere, such as the Windows runner's output saved by CI. --profile names
a profile in tests/scenarios/tolerances.txt whose declared tolerances apply
(`gcc` for a GCC/glibc reference build, `cross` for Rust on another platform
against the Windows output); other scenarios must still match exactly.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def find_runner(build_dir: Path, config: str) -> Path | None:
    for candidate in (build_dir / config / "parity_runner.exe", build_dir / "parity_runner",
                      build_dir / "parity_runner.exe"):
        if candidate.exists():
            return candidate
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, default=REPOSITORY_ROOT / "reference" / "soloud",
                        help="SoLoud checkout (default: reference/soloud)")
    parser.add_argument("--build-dir", type=Path, default=REPOSITORY_ROOT / "build" / "reference")
    parser.add_argument("--config", default="Release")
    parser.add_argument("--cpp-dir", type=Path,
                        help="compare with C++ output already in this directory instead of rendering it")
    parser.add_argument("--profile", help="tolerance profile from tests/scenarios/tolerances.txt")
    args = parser.parse_args(argv)
    if args.cpp_dir is not None:
        return compare(args.cpp_dir.expanduser().resolve(), args.profile)

    reference = args.reference.expanduser().resolve()
    if not (reference / "contrib" / "CMakeLists.txt").exists():
        print(f"No SoLoud reference at {reference}. Run: python scripts/fetch_reference.py",
              file=sys.stderr)
        return 1
    build_dir = args.build_dir.expanduser().resolve()
    # CMAKE_BUILD_TYPE matters for single-config generators only.
    configure = ["cmake", "--no-warn-unused-cli",
                 "-S", str(REPOSITORY_ROOT / "reference" / "tests"), "-B", str(build_dir),
                 f"-DSOLOUDER_REFERENCE_DIR={reference.as_posix()}",
                 f"-DCMAKE_BUILD_TYPE={args.config}"]
    build = ["cmake", "--build", str(build_dir), "--config", args.config, "--target", "parity_runner"]
    for step in (configure, build):
        if subprocess.run(step).returncode != 0:
            return 1

    runner = find_runner(build_dir, args.config)
    if runner is None:
        print(f"parity_runner not found under {build_dir}", file=sys.stderr)
        return 1
    output = REPOSITORY_ROOT / "build" / "parity" / "cpp"
    shutil.rmtree(output, ignore_errors=True)
    output.mkdir(parents=True)
    scenarios = sorted((REPOSITORY_ROOT / "tests" / "scenarios").glob("*.scn"))
    if not scenarios:
        print("No scenarios in tests/scenarios", file=sys.stderr)
        return 1
    if subprocess.run([str(runner), str(output), *map(str, scenarios)]).returncode != 0:
        print("The C++ runner rejected a scenario (see above).", file=sys.stderr)
        return 1

    return compare(output, args.profile)


def compare(output: Path, profile: str | None) -> int:
    if not any(output.glob("*.f32")):
        print(f"No C++ output in {output}", file=sys.stderr)
        return 1
    env = dict(os.environ, SOLOUDER_PARITY_DIR=str(output), SOLOUDER_REQUIRE_PARITY="1")
    if profile:
        env["SOLOUDER_PARITY_PROFILE"] = profile
    test = ["cargo", "test", "-p", "solouder-parity", "--test", "parity", "--", "--nocapture"]
    return subprocess.run(test, cwd=REPOSITORY_ROOT, env=env).returncode


if __name__ == "__main__":
    raise SystemExit(main())
