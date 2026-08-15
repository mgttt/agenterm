#!/usr/bin/env python3
"""Black-box proof: compose L1+L2+L3 without cargo, keep L1 bytes frozen."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tarfile
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


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_layout(root: Path) -> dict[str, str]:
    l1_sha = {}
    for index, cell in enumerate(CELLS):
        cell_dir = root / "l1" / cell
        cell_dir.mkdir(parents=True)
        payload = f"FROZEN-L1-{cell}\n".encode() + bytes([index]) * 32
        loader = cell_dir / "loader"
        loader.write_bytes(payload)
        l1_sha[cell] = hashlib.sha256(payload).hexdigest()
    (root / "l2").mkdir()
    (root / "l2" / "host-abi.json").write_text(
        json.dumps({"schema": 1, "capabilities": ["tab.list", "cu.focused_window"]}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    (root / "l3").mkdir()
    (root / "l3" / "app.txt").write_text("write-once app payload\n", encoding="utf-8")
    return l1_sha


def cargo_blocker(bin_dir: Path) -> None:
    bin_dir.mkdir()
    if os.name == "nt":
        script = bin_dir / "cargo.cmd"
        script.write_text("@echo cargo must not run>&2\r\n@exit /b 99\r\n", encoding="utf-8")
    else:
        script = bin_dir / "cargo"
        script.write_text("#!/bin/sh\necho cargo must not run >&2\nexit 99\n", encoding="utf-8")
        script.chmod(script.stat().st_mode | stat.S_IEXEC)


def run_compose(repo: Path, layout: Path, archive: Path, path_prefix: str) -> dict:
    env = os.environ.copy()
    env["PATH"] = path_prefix
    env.pop("CARGO", None)
    env.pop("CARGO_HOME", None)
    proc = subprocess.run(
        [
            sys.executable,
            str(repo / "scripts" / "chassis-compose-product.py"),
            "--from",
            str(layout),
            "--out",
            str(archive),
        ],
        check=False,
        capture_output=True,
        text=True,
        env=env,
        cwd=str(repo),
    )
    if proc.returncode != 0:
        raise SystemExit(
            f"compose failed ({proc.returncode})\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return json.loads(proc.stdout)


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="chassis-compose-") as tmp_raw:
        tmp = Path(tmp_raw)
        layout = tmp / "layout"
        layout.mkdir()
        expected_l1 = write_layout(layout)
        blocker = tmp / "nobin"
        cargo_blocker(blocker)
        first = tmp / "product-a.tgz"
        second = tmp / "product-b.tgz"
        report_a = run_compose(repo, layout, first, str(blocker))
        report_b = run_compose(repo, layout, second, str(blocker))
        if report_a["sha256"] != report_b["sha256"]:
            raise SystemExit("compose output is not deterministic")
        if report_a["invokes_cargo"] is not False:
            raise SystemExit("compose must declare invokes_cargo=false")
        if report_a["l1_sha256"] != expected_l1:
            raise SystemExit("compose report lost L1 hashes")
        with tarfile.open(first, "r:gz") as tar:
            names = set(tar.getnames())
            for cell in CELLS:
                member = f"l1/{cell}/loader"
                if member not in names:
                    raise SystemExit(f"missing {member}")
                extracted = tar.extractfile(member)
                assert extracted is not None
                digest = hashlib.sha256(extracted.read()).hexdigest()
                if digest != expected_l1[cell]:
                    raise SystemExit(f"L1 bytes changed for {cell}")
            if "l2/host-abi.json" not in names or "l3/app.txt" not in names:
                raise SystemExit("L2/L3 payload missing from archive")
            if "manifest.json" not in names:
                raise SystemExit("manifest.json missing")
        print("PASS: chassis-compose-product packs L1+L2+L3 without cargo")
        print(f"archive_sha256={report_a['sha256']}")


if __name__ == "__main__":
    main()
