#!/usr/bin/env python3
"""Black-box tests for the Chassis-L1 diff classifier."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GATE = ROOT / "scripts" / "chassis-l1-change-gate.py"


def run(
    *paths: str,
    github_output: Path | None = None,
    surface: Path | None = None,
) -> dict:
    command = [sys.executable, str(GATE), "--paths", *paths]
    if surface is not None:
        command.extend(["--surface", str(surface)])
    if github_output is not None:
        command.extend(["--github-output", str(github_output)])
    proc = subprocess.run(command, cwd=ROOT, check=False, capture_output=True, text=True)
    if proc.returncode != 0:
        raise AssertionError(f"gate failed ({proc.returncode}): {proc.stderr}")
    return json.loads(proc.stdout)


def assert_reason(path: str, reason: str) -> None:
    report = run(path)
    assert report["requires_l1_candidate"] is True, path
    assert report["l1_reasons"] == {reason: [path]}, report


def main() -> None:
    assert_reason("src/bin/agenterm.rs", "loader")
    assert_reason("crates/agenterm-platform/src/window_host.rs", "window")
    assert_reason("crates/agenterm-platform/src/window_op.rs", "window")
    assert_reason("src/pty/mod.rs", "pty")
    assert_reason("src/ipc_transport.rs", "ipc")

    non_l1 = [
        "crates/agenterm-chassis/l2/host-abi.json",
        "crates/agenterm-chassis/l3/example-app.json",
        "crates/agenterm-chassis/src/bytecode.rs",
        "crates/agenterm-chassis/src/vm.rs",
        "crates/agenterm-cu/src/lib.rs",
        "crates/agenterm-platform/src/clipboard.rs",
        "src/frontend/window.rs",
        "src/platform/adapters/unix/frontend/render.rs",
        "scripts/chassis-compose-product.py",
        "packaging/windows/manifest.json",
        "plan/refactor-chassis-l1-l2-l3.md",
        ".github/workflows/ci-chassis.yml",
    ]
    report = run(*non_l1)
    assert report["requires_l1_candidate"] is False, report
    assert report["l1_reasons"] == {}, report
    classified_non_l1 = sorted(
        report["explicitly_not_l1"] + report["unmatched_not_l1"]
    )
    assert classified_non_l1 == sorted(non_l1), report

    mixed = run("src/frontend/action.rs", "src/protocol.rs", "README.md")
    assert mixed["requires_l1_candidate"] is True, mixed
    assert mixed["l1_reasons"] == {"ipc": ["src/protocol.rs"]}, mixed
    assert mixed["unmatched_not_l1"] == ["README.md"], mixed

    with tempfile.TemporaryDirectory(prefix="chassis-l1-gate-") as raw:
        temp = Path(raw)
        output = temp / "github-output"
        report = run("src/operations.rs", github_output=output)
        assert report["requires_l1_candidate"] is False
        assert output.read_text(encoding="utf-8") == (
            "requires_l1_candidate=false\nl1_reasons=\n"
        )

        overlap = temp / "overlap.json"
        overlap.write_text(
            json.dumps(
                {
                    "schema": 2,
                    "l1_reasons": {
                        "loader": {
                            "path_prefixes": ["src/loader/"],
                            "exact_paths": [],
                        }
                    },
                    "explicitly_not_l1": ["src/"],
                }
            ),
            encoding="utf-8",
        )
        proc = subprocess.run(
            [
                sys.executable,
                str(GATE),
                "--paths",
                "src/loader/main.rs",
                "--surface",
                str(overlap),
            ],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        assert proc.returncode == 2, proc
        assert "surface overlap would hide L1 reason" in proc.stderr, proc.stderr

    print("PASS: chassis L1 gate limits six-cell reasons to loader/window/PTY/IPC")
    print("PASS: L2/L3/CU/frontend/scripts/docs/workflow diffs do not trigger L1")


if __name__ == "__main__":
    main()
