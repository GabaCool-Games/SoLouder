#!/usr/bin/env python3
"""Generate or update docs/parity-ledger.csv from the reference C API.

The parity target is the whole SoLoud C API (include/soloud_c.h). Each ledger
row is one C++ behavior; C functions that reach the same behavior are grouped:
`Soloud_play` and `Soloud_playEx` share `Soloud::play`, and methods every
audio source (or filter) exposes, such as `Wav_setVolume`, share one
`AudioSource::setVolume` (or `Filter::...`) row.

Generated columns are rewritten on every run. The tracking columns (slice,
milestone, status, scenarios, notes) are preserved for existing rows; new
rows get a proposed slice and milestone and status `todo`.
Use --check in CI to fail when the ledger is out of date.
"""

from __future__ import annotations

import argparse
import csv
import io
from pathlib import Path
import re
import sys

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_HEADER = REPOSITORY_ROOT / "reference" / "soloud" / "include" / "soloud_c.h"
LEDGER_PATH = REPOSITORY_ROOT / "docs" / "parity-ledger.csv"

GENERATED = ["behavior", "kind", "c_functions"]
TRACKING = ["slice", "milestone", "status", "scenarios", "notes"]
STATUSES = {"todo", "in-progress", "ported", "excluded"}

DECLARATION = re.compile(r"^[A-Za-z_][\w\s\*]*?\b([A-Z]\w*?)_(\w+)\s*\((.*)\);\s*$")
GENERATORS = {"Sfxr", "Speech", "Monotone", "Vic", "TedSid", "Ay", "Noise", "Vizsn", "Openmpt"}

# Proposed engine slice/milestone by method name, first match wins.
ENGINE_RULES = [
    (r"3d|Speaker", "spatial", "E"),
    (r"Group", "routing", "D"),
    (r"Filter", "filters", "D"),
    (r"^(fade|oscillate|schedule)", "playback", "D"),
    (r"Clocked", "playback", "D"),
    (r"Visualization|Wave|FFT|ApproximateVolume", "visualization", "F"),
    (r"^(init|deinit|getBackend|setPostClipScaler|getPostClipScaler|setMainResampler|getMainResampler)",
     "output_lifecycle", "C"),
]


def parse_header(text: str) -> list[tuple[str, str, str]]:
    declarations = []
    for line in text.splitlines():
        match = DECLARATION.match(line.strip())
        if match:
            declarations.append(match.groups())
    return declarations


def behavior_key(cls: str, method: str, methods_of: dict[str, set[str]]) -> str:
    if method.endswith("Ex") and method[:-2] in methods_of[cls]:
        method = method[:-2]
    if method in ("create", "destroy"):
        return f"{cls}::lifecycle"
    return f"{cls}::{method}"


def build_rows(declarations: list[tuple[str, str, str]]) -> dict[str, dict[str, str]]:
    methods_of: dict[str, set[str]] = {}
    for cls, method, _ in declarations:
        methods_of.setdefault(cls, set()).add(method)

    def plain(cls: str) -> set[str]:
        return {behavior_key(cls, m, methods_of).split("::")[1] for m in methods_of[cls]} - {"lifecycle"}

    sources = sorted(c for c in methods_of if c != "Soloud" and not c.endswith("Filter"))
    filters = sorted(c for c in methods_of if c.endswith("Filter"))
    common_source = set.intersection(*(plain(c) for c in sources)) if sources else set()
    common_filter = set.intersection(*(plain(c) for c in filters)) if filters else set()

    rows: dict[str, dict[str, str]] = {}
    for cls, method, _ in declarations:
        key = behavior_key(cls, method, methods_of)
        name = key.split("::")[1]
        if cls in sources and name in common_source:
            key, kind = f"AudioSource::{name}", "source-common"
        elif cls in filters and name in common_filter:
            key, kind = f"Filter::{name}", "filter-common"
        elif cls == "Soloud":
            kind = "engine"
        elif cls in filters:
            kind = "filter"
        else:
            kind = "source"
        row = rows.setdefault(key, {"behavior": key, "kind": kind, "c_functions": []})
        row["c_functions"].append(f"{cls}_{method}")
    for row in rows.values():
        row["c_functions"] = " ".join(sorted(set(row["c_functions"])))
    return rows


def proposed(row: dict[str, str]) -> tuple[str, str, str]:
    """Return (slice, milestone, note) proposals for a new row."""
    cls, name = row["behavior"].split("::")
    if row["kind"] == "engine":
        for pattern, slice_name, milestone in ENGINE_RULES:
            if re.search(pattern, name):
                return slice_name, milestone, ""
        return "playback", "B", ""
    if row["kind"] in ("filter", "filter-common"):
        return "filters", "D", ""
    if row["kind"] == "source-common":
        if "3d" in name:
            return "spatial", "E", ""
        if "Filter" in name:
            return "filters", "D", ""
        return "playback", "B", ""
    if cls == "Wav":
        return "decoded_audio", "B", ""
    if cls == "WavStream":
        return "streaming", "E", ""
    if cls in ("Bus", "Queue"):
        return "routing", "D", ""
    if cls == "Openmpt":
        return "sources", "F", "decide: wraps external libopenmpt"
    if cls in GENERATORS:
        return "sources", "F", ""
    return "", "", "unclassified"


def read_ledger(path: Path) -> dict[str, dict[str, str]]:
    if not path.exists():
        return {}
    with path.open(newline="", encoding="utf-8") as handle:
        return {row["behavior"]: row for row in csv.DictReader(handle)}


def render(rows: dict[str, dict[str, str]], previous: dict[str, dict[str, str]]) -> str:
    out = io.StringIO()
    writer = csv.DictWriter(out, fieldnames=GENERATED + TRACKING, lineterminator="\n")
    writer.writeheader()
    order = {"engine": 0, "source-common": 1, "source": 2, "filter-common": 3, "filter": 4}
    for key in sorted(rows, key=lambda k: (order[rows[k]["kind"]], k.lower())):
        row = dict(rows[key])
        if key in previous:
            row.update({column: previous[key].get(column, "") for column in TRACKING})
        else:
            slice_name, milestone, note = proposed(row)
            row.update(slice=slice_name, milestone=milestone, status="todo", scenarios="", notes=note)
        writer.writerow(row)
    return out.getvalue()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--header", type=Path, default=DEFAULT_HEADER)
    parser.add_argument("--check", action="store_true", help="Fail if the ledger is out of date")
    args = parser.parse_args(argv)

    if not args.header.exists():
        print(f"Missing {args.header}. Run: python scripts/fetch_reference.py", file=sys.stderr)
        return 2
    declarations = parse_header(args.header.read_text(encoding="utf-8", errors="replace"))
    rows = build_rows(declarations)
    previous = read_ledger(LEDGER_PATH)
    text = render(rows, previous)

    removed = sorted(set(previous) - set(rows))
    bad_status = sorted(k for k, r in previous.items() if r.get("status") not in STATUSES)
    if args.check:
        current = LEDGER_PATH.read_text(encoding="utf-8") if LEDGER_PATH.exists() else ""
        if current != text or bad_status:
            print("docs/parity-ledger.csv is out of date or has invalid statuses; "
                  "run scripts/gen_parity_ledger.py.", file=sys.stderr)
            return 1
        print("Parity ledger is up to date.")
        return 0

    LEDGER_PATH.parent.mkdir(parents=True, exist_ok=True)
    LEDGER_PATH.write_text(text, encoding="utf-8", newline="\n")
    counts: dict[str, int] = {}
    for row in csv.DictReader(io.StringIO(text)):
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    print(f"{len(declarations)} C declarations -> {len(rows)} behaviors in "
          f"{LEDGER_PATH.relative_to(REPOSITORY_ROOT).as_posix()}")
    print("Status: " + ", ".join(f"{s} {n}" for s, n in sorted(counts.items())))
    for key in removed:
        print(f"Removed from the C API (row dropped): {key}")
    for key in bad_status:
        print(f"Invalid status for {key}: {previous[key].get('status')!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
