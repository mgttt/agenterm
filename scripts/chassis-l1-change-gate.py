#!/usr/bin/env python3
"""Classify a diff against the frozen Chassis-L1 six-cell surface."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path, PurePosixPath


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def normalize_path(raw: str) -> str:
    value = raw.strip().replace("\\", "/")
    while value.startswith("./"):
        value = value[2:]
    path = PurePosixPath(value)
    if not value or path.is_absolute() or ".." in path.parts:
        raise ValueError(f"path must be repository-relative: {raw!r}")
    return path.as_posix()


def matches(path: str, entries: list[str]) -> bool:
    for entry in entries:
        if entry.endswith("/"):
            if path.startswith(entry):
                return True
        elif path == entry:
            return True
    return False


def patterns_overlap(left: str, right: str) -> bool:
    if left.endswith("/") and right.endswith("/"):
        return left.startswith(right) or right.startswith(left)
    if left.endswith("/"):
        return right.startswith(left)
    if right.endswith("/"):
        return left.startswith(right)
    return left == right


def load_surface(path: Path) -> dict:
    surface = json.loads(path.read_text(encoding="utf-8"))
    if surface.get("schema") != 2:
        raise ValueError("surface schema must be 2")
    reasons = surface.get("l1_reasons")
    if not isinstance(reasons, dict) or not reasons:
        raise ValueError("surface must contain non-empty l1_reasons")
    for reason, rules in reasons.items():
        if not isinstance(reason, str) or not reason:
            raise ValueError("L1 reason names must be non-empty strings")
        for key in ("path_prefixes", "exact_paths"):
            entries = rules.get(key)
            if not isinstance(entries, list) or not all(
                isinstance(item, str) for item in entries
            ):
                raise ValueError(f"{reason}.{key} must be a string list")
    excluded = surface.get("explicitly_not_l1")
    if not isinstance(excluded, list) or not all(
        isinstance(item, str) for item in excluded
    ):
        raise ValueError("explicitly_not_l1 must be a string list")
    for reason, rules in reasons.items():
        l1_entries = rules["path_prefixes"] + rules["exact_paths"]
        for l1_entry in l1_entries:
            for excluded_entry in excluded:
                if patterns_overlap(l1_entry, excluded_entry):
                    raise ValueError(
                        f"surface overlap would hide L1 reason {reason}: "
                        f"{l1_entry!r} vs {excluded_entry!r}"
                    )
    return surface


def git_changed_paths(root: Path, base: str, head: str) -> list[str]:
    proc = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", base, head, "--"],
        cwd=root,
        check=False,
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.decode("utf-8", errors="replace").strip())
    return [part.decode("utf-8") for part in proc.stdout.split(b"\0") if part]


def classify(surface: dict, raw_paths: list[str]) -> dict:
    paths = sorted({normalize_path(path) for path in raw_paths})
    excluded_rules = surface["explicitly_not_l1"]
    reasons: dict[str, list[str]] = {}
    excluded: list[str] = []
    unmatched: list[str] = []

    for path in paths:
        if matches(path, excluded_rules):
            excluded.append(path)
            continue
        path_reasons = []
        for reason, rules in surface["l1_reasons"].items():
            exact = rules["exact_paths"]
            prefixes = rules["path_prefixes"]
            if path in exact or any(path.startswith(prefix) for prefix in prefixes):
                path_reasons.append(reason)
        if path_reasons:
            for reason in path_reasons:
                reasons.setdefault(reason, []).append(path)
        else:
            unmatched.append(path)

    return {
        "schema": 1,
        "requires_l1_candidate": bool(reasons),
        "l1_reasons": reasons,
        "explicitly_not_l1": excluded,
        "unmatched_not_l1": unmatched,
        "changed_paths": paths,
    }


def write_github_output(path: Path, report: dict) -> None:
    reason_names = ",".join(sorted(report["l1_reasons"]))
    with path.open("a", encoding="utf-8") as handle:
        required = str(report["requires_l1_candidate"]).lower()
        handle.write(f"requires_l1_candidate={required}\n")
        handle.write(f"l1_reasons={reason_names}\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--surface",
        type=Path,
        default=repo_root() / "plan" / "chassis-l1-surface.json",
    )
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--paths", nargs="*", metavar="PATH")
    source.add_argument("--base", metavar="GIT_REF")
    parser.add_argument("--head", default="HEAD", metavar="GIT_REF")
    parser.add_argument("--github-output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        surface = load_surface(args.surface)
        raw_paths = args.paths
        if raw_paths is None:
            raw_paths = git_changed_paths(repo_root(), args.base, args.head)
        report = classify(surface, raw_paths)
        if args.github_output is not None:
            write_github_output(args.github_output, report)
        json.dump(report, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
        return 0
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        print(f"chassis-l1-change-gate: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
