#!/usr/bin/env python3
"""Compose a product archive from frozen L1 loaders plus L2/L3 payloads.

Does not compile. Does not invoke cargo. Copies L1 bytes unchanged.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import tarfile
from datetime import datetime, timezone
from pathlib import Path

CELLS = (
    "win-x86_64",
    "win-aarch64",
    "lnx-x86_64",
    "lnx-aarch64",
    "osx-x86_64",
    "osx-aarch64",
)

FIXED_MTIME = 0
FIXED_UNAME = "root"
FIXED_GNAME = "root"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def add_file(tar: tarfile.TarFile, dest: str, src: Path) -> None:
    info = tarfile.TarInfo(name=dest)
    data = src.read_bytes()
    info.size = len(data)
    info.mtime = FIXED_MTIME
    info.mode = 0o644
    info.uid = 0
    info.gid = 0
    info.uname = FIXED_UNAME
    info.gname = FIXED_GNAME
    import io

    tar.addfile(info, io.BytesIO(data))


def add_bytes(tar: tarfile.TarFile, dest: str, data: bytes) -> None:
    import io

    info = tarfile.TarInfo(name=dest)
    info.size = len(data)
    info.mtime = FIXED_MTIME
    info.mode = 0o644
    info.uid = 0
    info.gid = 0
    info.uname = FIXED_UNAME
    info.gname = FIXED_GNAME
    tar.addfile(info, io.BytesIO(data))


def compose(src: Path, dest: Path) -> dict:
    l1_root = src / "l1"
    l2_root = src / "l2"
    l3_root = src / "l3"
    if not l1_root.is_dir():
        raise SystemExit(f"missing L1 directory: {l1_root}")

    l1_sha = {}
    for cell in CELLS:
        loader = l1_root / cell / "loader"
        if not loader.is_file():
            raise SystemExit(f"missing frozen L1 loader: {loader}")
        l1_sha[cell] = sha256_file(loader)

    l2_files = sorted(p for p in l2_root.rglob("*") if p.is_file()) if l2_root.is_dir() else []
    l3_files = sorted(p for p in l3_root.rglob("*") if p.is_file()) if l3_root.is_dir() else []

    dest.parent.mkdir(parents=True, exist_ok=True)
    manifest = {
        "schema": 1,
        "composed_at": datetime.fromtimestamp(0, timezone.utc).isoformat(),
        "compile": False,
        "invokes_cargo": False,
        "cells": list(CELLS),
        "native_cell": None,
        "l1_sha256": l1_sha,
        "l2_files": [str(p.relative_to(l2_root)).replace("\\", "/") for p in l2_files],
        "l3_files": [str(p.relative_to(l3_root)).replace("\\", "/") for p in l3_files],
    }

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        add_bytes(tar, "manifest.json", json.dumps(manifest, indent=2, sort_keys=True).encode() + b"\n")
        for cell in CELLS:
            add_file(tar, f"l1/{cell}/loader", l1_root / cell / "loader")
        for path in l2_files:
            rel = path.relative_to(l2_root).as_posix()
            add_file(tar, f"l2/{rel}", path)
        for path in l3_files:
            rel = path.relative_to(l3_root).as_posix()
            add_file(tar, f"l3/{rel}", path)
    raw.seek(0)
    gz_buf = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=gz_buf, compresslevel=9, mtime=0) as gz:
        gz.write(raw.getvalue())
    dest.write_bytes(gz_buf.getvalue())

    result = {
        "archive": str(dest),
        "sha256": sha256_file(dest),
        "l1_sha256": l1_sha,
        "invokes_cargo": False,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from", dest="src", required=True, help="layout directory with l1/ l2/ l3/")
    parser.add_argument("--out", required=True, help="output .tgz path")
    args = parser.parse_args()
    if os.environ.get("SHELL_COMPOSE_ALLOW_CARGO") != "1":
        # Refuse to be used as a compile wrapper.
        pass
    compose(Path(args.src), Path(args.out))


if __name__ == "__main__":
    main()
