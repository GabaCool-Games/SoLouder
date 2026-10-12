#!/usr/bin/env python3
"""Check the 400-physical-line limit for first-party Rust source files."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import sys

MAX_LINES = 400
REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
SKIPPED_DIRECTORY_NAMES = {".git", "target"}
# vendor/ holds external crates; reference/soloud is the git-ignored C++ checkout.
EXCLUDED_ROOTS = {("vendor",), ("reference", "soloud")}


def physical_line_count(path: Path) -> int:
    data = path.read_bytes()
    if not data:
        return 0
    return data.count(b"\n") + int(not data.endswith(b"\n"))


def rust_files(root: Path) -> list[Path]:
    files = []

    def raise_walk_error(error: OSError) -> None:
        raise error

    for directory, names, filenames in os.walk(root, onerror=raise_walk_error):
        current = Path(directory)
        names[:] = sorted(
            name
            for name in names
            if name not in SKIPPED_DIRECTORY_NAMES
            and (current / name).relative_to(root).parts not in EXCLUDED_ROOTS
        )
        for name in sorted(filenames):
            path = current / name
            if path.suffix == ".rs":
                files.append(path)
    return sorted(files)


def violations(root: Path) -> tuple[int, list[tuple[Path, int]]]:
    paths = rust_files(root)
    over_limit = []
    for path in paths:
        count = physical_line_count(path)
        if count > MAX_LINES:
            over_limit.append((path.relative_to(root), count))
    return len(paths), over_limit


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=REPOSITORY_ROOT, help="Repository root to inspect"
    )
    args = parser.parse_args(argv)
    try:
        root = args.root.expanduser().resolve(strict=True)
        if not root.is_dir():
            raise ValueError(f"Not a directory: {root}")
        count, over_limit = violations(root)
    except (OSError, ValueError) as error:
        print(f"Rust file-size check error: {error}", file=sys.stderr)
        return 2
    for path, lines in over_limit:
        print(f"FAIL {path.as_posix()}: {lines} physical lines (maximum {MAX_LINES})")
    if over_limit:
        print(f"Found {len(over_limit)} oversized Rust file(s) among {count} checked.")
        return 1
    if count:
        print(f"PASS: {count} Rust file(s), each at or below {MAX_LINES} physical lines.")
    else:
        print("PASS: no first-party Rust files found; no size violations to check.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
