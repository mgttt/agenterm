#!/usr/bin/env python3
"""Stage one typed, size-bounded Chassis-L1 loader for Candidate compose."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path


CELLS = {
    "win-x86_64",
    "win-aarch64",
    "lnx-x86_64",
    "lnx-aarch64",
    "osx-x86_64",
    "osx-aarch64",
}
MAX_LOADER_BYTES = 2 * 1024 * 1024


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--loader", required=True)
    parser.add_argument("--cell", required=True, choices=sorted(CELLS))
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    source_sha = args.source_sha
    if len(source_sha) != 40 or any(ch not in "0123456789abcdef" for ch in source_sha):
        raise SystemExit("source SHA must be 40 lowercase hexadecimal characters")
    source = Path(args.loader)
    if not source.is_file():
        raise SystemExit("thin L1 loader is missing")
    size = source.stat().st_size
    if size == 0 or size > MAX_LOADER_BYTES:
        raise SystemExit(f"thin L1 loader size must be 1..{MAX_LOADER_BYTES} bytes")

    root = Path(args.out) / "chassis-l1" / args.cell
    root.mkdir(parents=True, exist_ok=True)
    loader = root / "loader"
    shutil.copyfile(source, loader)
    loader.chmod(0o755)
    descriptor = {
        "schema": 1,
        "kind": "agenterm-chassis-l1-loader",
        "cell": args.cell,
        "version": args.version,
        "source_sha": source_sha,
        "bytes": size,
        "sha256": sha256_file(loader),
        "max_bytes": MAX_LOADER_BYTES,
    }
    (root / "loader.json").write_text(
        json.dumps(descriptor, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(descriptor, sort_keys=True))


if __name__ == "__main__":
    main()
