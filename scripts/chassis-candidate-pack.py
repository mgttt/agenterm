#!/usr/bin/env python3
"""Compose one chassis product image from six already-built Candidate archives.

This adapter never builds a platform artifact. Each Candidate archive is copied
byte-for-byte into the matching frozen L1 cell before the ordinary chassis
composer attaches the repository L2 and L3 payloads.
"""

from __future__ import annotations

import argparse
import hashlib
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
MAX_LOADER_BYTES = 2 * 1024 * 1024


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def cargo_version(repo: Path) -> str:
    in_package = False
    for raw_line in (repo / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line.startswith("["):
            in_package = line == "[package]"
        elif in_package and line.startswith("version = "):
            return line.split('"', 2)[1]
    raise SystemExit("root Cargo.toml package version is missing")


def write_companions(
    repo: Path, candidate_input: Path, output: Path, version: str, source_sha: str
) -> None:
    archive_hash = sha256_file(output)
    checksum = output.with_name(output.name + ".sha256")
    checksum.write_text(f"{archive_hash}  {output.name}\n", encoding="utf-8")
    sbom = candidate_input / f"agenterm-{version}-sbom.spdx.json"
    if not sbom.is_file():
        raise SystemExit("Candidate SBOM is missing")
    provenance = {
        "schema_version": 1,
        "product": "AgenTerm",
        "version": version,
        "os": "chassis",
        "arch": "multi",
        "channel": "release",
        "signed": False,
        "notarized": False,
        "artifact": output.name,
        "sha256": archive_hash,
        "source_commit": source_sha,
        "source_tag": f"v{version}",
        "artifact_manifest_sha256": sha256_file(repo / "scripts/artifacts.json"),
        "cargo_lock_sha256": sha256_file(repo / "Cargo.lock"),
        "sbom_sha256": sha256_file(sbom),
        "compose": "chassis-l1-l2-l3",
        "invokes_cargo": False,
    }
    output.with_name(output.name + ".provenance.json").write_text(
        json.dumps(provenance, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def one_loader(candidate_input: Path, version: str, source_sha: str, cell: str) -> Path:
    root = candidate_input / "chassis-l1" / cell
    loader = root / "loader"
    descriptor_path = root / "loader.json"
    if not loader.is_file() or not descriptor_path.is_file():
        raise SystemExit(f"typed Candidate L1 loader is missing for {cell}")
    descriptor = json.loads(descriptor_path.read_text(encoding="utf-8"))
    size = loader.stat().st_size
    expected = {
        "schema": 1,
        "kind": "agenterm-chassis-l1-loader",
        "cell": cell,
        "version": version,
        "source_sha": source_sha,
        "bytes": size,
        "sha256": sha256_file(loader),
        "max_bytes": MAX_LOADER_BYTES,
    }
    if descriptor != expected:
        raise SystemExit(f"typed Candidate L1 descriptor mismatch for {cell}")
    if size == 0 or size > MAX_LOADER_BYTES:
        raise SystemExit(f"thin Candidate L1 loader size is invalid for {cell}")
    return loader


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-input", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()

    repo = Path(__file__).resolve().parents[1]
    version = args.version
    if cargo_version(repo) != version:
        raise SystemExit("requested chassis version does not match root Cargo.toml")
    source_sha = args.source_sha
    if len(source_sha) != 40 or any(ch not in "0123456789abcdef" for ch in source_sha):
        raise SystemExit("source SHA must be 40 lowercase hexadecimal characters")

    candidate_input = Path(args.candidate_input).resolve()
    output = Path(args.out).resolve()
    compose = repo / "scripts/chassis-compose-product.py"
    l2 = repo / "crates/agenterm-chassis/l2"
    l3 = repo / "crates/agenterm-chassis/l3"
    if not candidate_input.is_dir() or not l2.is_dir() or not l3.is_dir():
        raise SystemExit("candidate input or chassis L2/L3 tree is missing")

    with tempfile.TemporaryDirectory(prefix="chassis-candidate-pack-") as tmp_raw:
        layout = Path(tmp_raw) / "layout"
        frozen = {}
        for cell in CELLS:
            source = one_loader(candidate_input, version, source_sha, cell)
            loader = layout / "l1" / cell / "loader"
            loader.parent.mkdir(parents=True)
            shutil.copyfile(source, loader)
            frozen[cell] = {"asset": f"chassis-l1/{cell}/loader", "sha256": sha256_file(loader)}
        shutil.copytree(l2, layout / "l2")
        shutil.copytree(l3, layout / "l3")
        (layout / "l3" / "product-identity.json").write_text(
            json.dumps(
                {
                    "schema": 1,
                    "version": version,
                    "source_sha": source_sha,
                    "l1_archives": frozen,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        report = subprocess.run(
            [
                sys.executable,
                str(compose),
                "--from",
                str(layout),
                "--out",
                str(output),
            ],
            cwd=repo,
            check=False,
            capture_output=True,
            text=True,
        )
        if report.returncode != 0:
            raise SystemExit(
                f"chassis compose failed\nstdout:\n{report.stdout}\nstderr:\n{report.stderr}"
            )
        composed = json.loads(report.stdout)
        if composed.get("invokes_cargo") is not False:
            raise SystemExit("chassis Candidate pack must not invoke cargo")
        if composed.get("l1_sha256") != {
            cell: identity["sha256"] for cell, identity in frozen.items()
        }:
            raise SystemExit("composed L1 hashes do not match Candidate archives")
        write_companions(repo, candidate_input, output, version, source_sha)
        print(
            json.dumps(
                {
                    "kind": "agenterm-chassis-candidate-pack",
                    "version": version,
                    "source_sha": source_sha,
                    "archive": output.name,
                    "sha256": sha256_file(output),
                    "invokes_cargo": False,
                    "l1": frozen,
                },
                indent=2,
                sort_keys=True,
            )
        )


if __name__ == "__main__":
    main()
