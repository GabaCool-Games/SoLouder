#!/usr/bin/env python3
"""Create the patched C++ SoLoud reference checkout (git-ignored).

Clones upstream SoLoud at the pinned commit from reference/upstream.json and
applies reference/patches/*.patch in order with `git am`. The result is the
same tree for every developer and agent, without depending on a private fork.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import stat
import subprocess
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = REPOSITORY_ROOT / "reference" / "upstream.json"
STAMP_NAME = ".solouder-reference"
# Identity for the commits `git am` creates; authorship comes from each patch.
COMMITTER = ["-c", "user.name=SoLouder reference", "-c", "user.email=reference@solouder.invalid"]


def load_config() -> dict:
    return json.loads(CONFIG_PATH.read_text(encoding="utf-8"))


def patch_files(config: dict) -> list[Path]:
    return sorted((REPOSITORY_ROOT / config["patches"]).glob("*.patch"))


def expected_stamp(config: dict, patches: list[Path]) -> str:
    digest = hashlib.sha256()
    digest.update(config["commit"].encode())
    for patch in patches:
        digest.update(patch.name.encode())
        digest.update(patch.read_bytes())
    return digest.hexdigest()


def git(*args: str, cwd: Path | None = None) -> None:
    subprocess.run(["git", *args], cwd=cwd, check=True)


def remove_tree(path: Path) -> None:
    # Git marks object files read-only, which blocks deletion on Windows.
    def make_writable(function, target, _error):
        Path(target).chmod(stat.S_IWRITE)
        function(target)

    if sys.version_info >= (3, 12):
        shutil.rmtree(path, onexc=make_writable)
    else:
        shutil.rmtree(path, onerror=make_writable)


def fetch(source: str, destination: Path, config: dict, patches: list[Path]) -> None:
    git("clone", "--no-checkout", "-c", "core.autocrlf=false", source, str(destination))
    git("checkout", "--detach", config["commit"], cwd=destination)
    if patches:
        # Patches are byte-exact against upstream blobs, some of which use CRLF.
        git(*COMMITTER, "am", "--keep-cr", *[str(p) for p in patches], cwd=destination)


def main(argv: list[str] | None = None) -> int:
    config = load_config()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", default=config["url"],
                        help="Git URL or local path to clone (default: upstream SoLoud)")
    parser.add_argument("--force", action="store_true",
                        help="Delete an existing checkout and recreate it")
    args = parser.parse_args(argv)

    destination = REPOSITORY_ROOT / config["checkout"]
    patches = patch_files(config)
    stamp = expected_stamp(config, patches)
    stamp_path = destination / STAMP_NAME

    if destination.exists():
        if not args.force:
            if stamp_path.exists() and stamp_path.read_text(encoding="utf-8").strip() == stamp:
                print(f"Reference is up to date: {destination}")
                return 0
            print(f"{destination} exists but does not match the pinned commit and patches.\n"
                  "Rerun with --force to recreate it (local edits there will be lost).",
                  file=sys.stderr)
            return 1
        remove_tree(destination)

    try:
        fetch(args.source, destination, config, patches)
    except subprocess.CalledProcessError as error:
        print(f"Reference fetch failed: {error}", file=sys.stderr)
        return 1
    stamp_path.write_text(stamp + "\n", encoding="utf-8")
    print(f"Reference ready: {destination} (upstream {config['commit'][:7]} + {len(patches)} patches)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
