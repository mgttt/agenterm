#!/usr/bin/env python3
"""CI pack: six-cell L1 stubs + crate L2 ABI + L3 app, no cargo."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

CELLS = (
    "win-x86_64",
    "win-aarch64",
    "lnx-x86_64",
    "lnx-aarch64",
    "osx-x86_64",
    "osx-aarch64",
)


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    abi = repo / "crates/agenterm-chassis/l2/host-abi.json"
    app = repo / "crates/agenterm-chassis/l3/example-app.json"
    compose = repo / "scripts/chassis-compose-product.py"
    if not abi.is_file() or not app.is_file() or not compose.is_file():
        raise SystemExit("missing chassis L2/L3 or compose script")

    with tempfile.TemporaryDirectory(prefix="chassis-ci-pack-") as tmp_raw:
        tmp = Path(tmp_raw)
        layout = tmp / "layout"
        for cell in CELLS:
            cell_dir = layout / "l1" / cell
            cell_dir.mkdir(parents=True)
            (cell_dir / "loader").write_bytes(f"CHASSIS-L1-STUB:{cell}\n".encode())
        (layout / "l2").mkdir()
        shutil.copyfile(abi, layout / "l2" / "host-abi.json")
        (layout / "l3").mkdir()
        shutil.copyfile(app, layout / "l3" / "app.json")
        archive = tmp / "product.tgz"
        report = subprocess.run(
            [sys.executable, str(compose), "--from", str(layout), "--out", str(archive)],
            check=False,
            capture_output=True,
            text=True,
            cwd=str(repo),
        )
        if report.returncode != 0:
            raise SystemExit(
                f"compose failed\nstdout:\n{report.stdout}\nstderr:\n{report.stderr}"
            )
        data = json.loads(report.stdout)
        if data.get("invokes_cargo") is not False:
            raise SystemExit("compose must not invoke cargo")
        missing = [cell for cell in CELLS if cell not in data.get("l1_sha256", {})]
        if missing:
            raise SystemExit(f"pack missing L1 cells: {missing}")
        print("PASS: chassis CI packed six L1 cells + L2 ABI + L3 app without cargo")
        print(json.dumps({"sha256": data["sha256"], "cells": list(CELLS)}, indent=2))


if __name__ == "__main__":
    main()
