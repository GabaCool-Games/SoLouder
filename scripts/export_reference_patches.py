#!/usr/bin/env python3
"""Regenerate reference/patches/ from a patched SoLoud git branch.

Use this after committing a C++ reference fix in a SoLoud fork (for example
the GoreGrind fork's `goregrind` branch). Every commit between the pinned
upstream commit and --ref becomes one patch file. Afterwards, run
`python scripts/fetch_reference.py --force` and the reference tests.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = REPOSITORY_ROOT / "reference" / "upstream.json"


def main(argv: list[str] | None = None) -> int:
    config = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fork", type=Path, help="Path to the patched SoLoud git checkout")
    parser.add_argument("--ref", default="HEAD", help="Last commit to export (default: HEAD)")
    args = parser.parse_args(argv)

    fork = args.fork.expanduser().resolve()
    out_dir = REPOSITORY_ROOT / config["patches"]
    base = config["commit"]
    try:
        subprocess.run(["git", "-C", str(fork), "merge-base", "--is-ancestor", base, args.ref],
                       check=True)
    except subprocess.CalledProcessError:
        print(f"{args.ref} in {fork} does not contain upstream {base[:7]}.", file=sys.stderr)
        return 1

    out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob("*.patch"):
        old.unlink()
    # --zero-commit, --no-signature and --no-numbered keep existing files
    # unchanged when later patches are added.
    subprocess.run(
        ["git", "-C", str(fork), "format-patch", "--zero-commit", "--no-signature",
         "--no-numbered", "--output-directory", str(out_dir), f"{base}..{args.ref}"],
        check=True,
    )
    count = len(list(out_dir.glob("*.patch")))
    print(f"Exported {count} patches to {out_dir.relative_to(REPOSITORY_ROOT).as_posix()}.")
    print("Next: python scripts/fetch_reference.py --force && python scripts/test_reference.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
