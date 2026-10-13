#!/usr/bin/env python3
"""Run every repository check, in the order CI runs them.

Always: Rust file sizes (and the checker's own tests), the parity ledger,
the generated WAV fixtures, cargo fmt, cargo clippy and cargo test. With
--reference: the C++ reference regression tests. With --parity: C++ versus
Rust scenario comparison. Both need CMake, a C++ compiler and
`python scripts/fetch_reference.py`.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", action="store_true", help="also run the C++ regression tests")
    parser.add_argument("--parity", action="store_true", help="also compare scenarios with C++")
    args = parser.parse_args(argv)

    python = sys.executable
    checks = [
        [python, "scripts/check_rust_file_lines.py"],
        [python, "scripts/test_check_rust_file_lines.py"],
        [python, "scripts/gen_parity_ledger.py", "--check"],
        [python, "scripts/gen_wav_fixtures.py", "--check"],
        [python, "scripts/gen_flac_fixtures.py", "--check"],
        ["cargo", "fmt", "--all", "--", "--check"],
        ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--workspace"],
        # Decoders parse untrusted bytes; also test them without overflow checks.
        ["cargo", "test", "--release", "-p", "solouder-decoders"],
    ]
    if args.reference:
        checks.append([python, "scripts/test_reference.py"])
    if args.parity:
        checks.append([python, "scripts/run_parity.py"])

    failed = []
    for check in checks:
        print(f"\n==> {' '.join(check)}", flush=True)
        if subprocess.run(check, cwd=REPOSITORY_ROOT).returncode != 0:
            failed.append(" ".join(check))
    if failed:
        print("\nFailed checks:\n  " + "\n  ".join(failed), file=sys.stderr)
        return 1
    print("\nAll checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
