#!/usr/bin/env python3
"""Black-box tests for fail-closed Chassis product installation."""

from __future__ import annotations

import gzip
import hashlib
import io
import json
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
NATIVE_CELL = "lnx-x86_64"


def file_member(name: str, data: bytes, mode: int = 0o644) -> tarfile.TarInfo:
    member = tarfile.TarInfo(name)
    member.size = len(data)
    member.mode = mode
    member.mtime = 0
    return member


def product_members() -> list[tuple[tarfile.TarInfo, bytes | None]]:
    loaders = {cell: f"thin-loader:{cell}\n".encode() for cell in CELLS}
    manifest = {
        "schema": 1,
        "compile": False,
        "invokes_cargo": False,
        "cells": list(CELLS),
        "native_cell": NATIVE_CELL,
        "l1_sha256": {cell: hashlib.sha256(data).hexdigest() for cell, data in loaders.items()},
    }
    abi = {
        "schema": 1,
        "version": 3,
        "capabilities": [{"id": "tabs.list", "kind": "host_call", "impl": "native"}],
    }
    app = {"schema": 1, "name": "install.test", "capabilities": ["tabs.list"]}
    entries = [(file_member("manifest.json", json.dumps(manifest).encode()), json.dumps(manifest).encode())]
    entries.extend((file_member(f"l1/{cell}/loader", data, 0o755), data) for cell, data in loaders.items())
    entries.append((file_member("l2/host-abi.json", json.dumps(abi).encode()), json.dumps(abi).encode()))
    # Candidate currently names the repository payload example-app.json; the
    # installer exposes the canonical app.json path consumed by workbench.
    entries.append(
        (
            file_member("l3/example-app.json", json.dumps(app).encode()),
            json.dumps(app).encode(),
        )
    )
    return entries


def write_archive(path: Path, entries: list[tuple[tarfile.TarInfo, bytes | None]]) -> Path:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as tar:
        for member, data in entries:
            tar.addfile(member, io.BytesIO(data) if data is not None else None)
    with path.open("wb") as output, gzip.GzipFile(fileobj=output, mode="wb", mtime=0) as compressed:
        compressed.write(raw.getvalue())
    checksum = path.with_name(path.name + ".sha256")
    checksum.write_text(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n", encoding="utf-8")
    return checksum


def write_candidate_product(repo: Path, root: Path) -> tuple[Path, Path]:
    candidate = root / "candidate-input"
    candidate.mkdir()
    version = "0.1.16"
    source_sha = "2" * 40
    (candidate / f"agenterm-{version}-sbom.spdx.json").write_text(
        '{"spdxVersion":"SPDX-2.3"}\n', encoding="utf-8"
    )
    for cell in CELLS:
        source = root / f"loader-{cell}"
        source.write_bytes(f"candidate-thin-loader:{cell}\n".encode())
        staged = subprocess.run(
            [
                sys.executable,
                str(repo / "scripts/chassis-stage-l1-loader.py"),
                "--loader",
                str(source),
                "--cell",
                cell,
                "--version",
                version,
                "--source-sha",
                source_sha,
                "--out",
                str(candidate),
            ],
            cwd=repo,
            check=False,
            capture_output=True,
            text=True,
        )
        if staged.returncode != 0:
            raise SystemExit(f"Candidate L1 stage failed\n{staged.stdout}\n{staged.stderr}")
    archive = root / f"agenterm-{version}-chassis-product.tgz"
    packed = subprocess.run(
        [
            sys.executable,
            str(repo / "scripts/chassis-candidate-pack.py"),
            "--candidate-input",
            str(candidate),
            "--out",
            str(archive),
            "--version",
            version,
            "--source-sha",
            source_sha,
        ],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
    )
    if packed.returncode != 0:
        raise SystemExit(f"Candidate product pack failed\n{packed.stdout}\n{packed.stderr}")
    return archive, archive.with_name(archive.name + ".sha256")


def run_install(
    repo: Path, archive: Path, checksum: Path, destination: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(repo / "scripts/chassis-install-product.py"),
            "--archive",
            str(archive),
            "--checksum",
            str(checksum),
            "--install-dir",
            str(destination),
            "--native-cell",
            NATIVE_CELL,
        ],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
    )


def expect_rejected(
    repo: Path,
    root: Path,
    label: str,
    entries: list[tuple[tarfile.TarInfo, bytes | None]],
    message: str,
) -> None:
    archive = root / f"{label}.tgz"
    checksum = write_archive(archive, entries)
    destination = root / f"installed-{label}"
    result = run_install(repo, archive, checksum, destination)
    if result.returncode == 0 or message not in result.stderr:
        raise SystemExit(f"{label} was not rejected as {message!r}\n{result.stdout}\n{result.stderr}")
    if destination.exists():
        raise SystemExit(f"{label} left a partial install directory")


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="chassis-install-product-test-") as raw_tmp:
        root = Path(raw_tmp)
        archive, checksum = write_candidate_product(repo, root)
        destination = root / "installed"
        result = run_install(repo, archive, checksum, destination)
        if result.returncode != 0:
            raise SystemExit(f"valid product install failed\n{result.stdout}\n{result.stderr}")
        report = json.loads(result.stdout)
        if report["kind"] != "agenterm-chassis-installed-product" or report["native_cell"] != NATIVE_CELL:
            raise SystemExit("valid install report lost product identity")
        # This is the directory contract consumed by workbench --chassis-image.
        manifest = json.loads((destination / "manifest.json").read_text(encoding="utf-8"))
        app = json.loads((destination / "l3/app.json").read_text(encoding="utf-8"))
        if manifest["cells"] != list(CELLS) or app["name"] != "example.app":
            raise SystemExit("installed image does not satisfy workbench directory contract")
        loader = destination / f"l1/{NATIVE_CELL}/loader"
        if not loader.is_file() or loader.stat().st_mode & 0o777 != 0o755:
            raise SystemExit("installed native loader mode is not executable-only policy")
        overwrite = run_install(repo, archive, checksum, destination)
        if overwrite.returncode == 0 or "refusing to overwrite" not in overwrite.stderr:
            raise SystemExit("installer overwrote an existing directory")

        bad_checksum = root / "bad.sha256"
        bad_checksum.write_text(f"{'0' * 64}  {archive.name}\n", encoding="utf-8")
        tamper = run_install(repo, archive, bad_checksum, root / "installed-tamper")
        if tamper.returncode == 0 or "checksum mismatch" not in tamper.stderr:
            raise SystemExit("tampered product checksum was accepted")

        traversal = product_members()
        traversal.append((file_member("../escape", b"escape"), b"escape"))
        expect_rejected(repo, root, "traversal", traversal, "unsafe path")

        absolute = product_members()
        absolute.append((file_member("/escape", b"escape"), b"escape"))
        expect_rejected(repo, root, "absolute", absolute, "unsafe path")

        extra_root = product_members()
        extra_root.append((file_member("product/extra", b"extra"), b"extra"))
        expect_rejected(repo, root, "extra-root", extra_root, "extra root")

        fat = product_members()
        fat.append((file_member("l2/agenterm-linux.tar.gz", b"fat"), b"fat"))
        expect_rejected(repo, root, "fat", fat, "fat platform archive")

        oversized = product_members()
        oversized_payload = b"x" * (4 * 1024 * 1024 + 1)
        oversized.append(
            (file_member("l2/bomb.bin", oversized_payload), oversized_payload)
        )
        expect_rejected(repo, root, "oversized", oversized, "member size is invalid")

        symlink = product_members()
        link = tarfile.TarInfo("l2/link")
        link.type = tarfile.SYMTYPE
        link.linkname = "host-abi.json"
        symlink.append((link, None))
        expect_rejected(repo, root, "symlink", symlink, "forbidden entry type")

        hardlink = product_members()
        link = tarfile.TarInfo("l2/hardlink")
        link.type = tarfile.LNKTYPE
        link.linkname = "l2/host-abi.json"
        hardlink.append((link, None))
        expect_rejected(repo, root, "hardlink", hardlink, "forbidden entry type")

        device = product_members()
        node = tarfile.TarInfo("l2/device")
        node.type = tarfile.CHRTYPE
        device.append((node, None))
        expect_rejected(repo, root, "device", device, "forbidden entry type")

        duplicate = product_members()
        duplicate.append((file_member("manifest.json", b"{}"), b"{}"))
        expect_rejected(repo, root, "duplicate", duplicate, "duplicate entry")

        bad_mode = product_members()
        for member, _ in bad_mode:
            if member.name == f"l1/{NATIVE_CELL}/loader":
                member.mode = 0o644
        expect_rejected(repo, root, "mode", bad_mode, "loader mode is invalid")

        bad_loader_hash = product_members()
        for index, (member, _) in enumerate(bad_loader_hash):
            if member.name == f"l1/{NATIVE_CELL}/loader":
                payload = b"tampered-thin-loader"
                bad_loader_hash[index] = (file_member(member.name, payload, 0o755), payload)
        expect_rejected(repo, root, "loader-hash", bad_loader_hash, "loader hash mismatch")

        oversized_loader = product_members()
        for index, (member, _) in enumerate(oversized_loader):
            if member.name == f"l1/{NATIVE_CELL}/loader":
                payload = b"x" * (2 * 1024 * 1024 + 1)
                oversized_loader[index] = (file_member(member.name, payload, 0o755), payload)
        expect_rejected(repo, root, "loader-size", oversized_loader, "loader size is invalid")

        print("PASS: chassis product installer is atomic and fail-closed")
        print("PASS: installed directory satisfies workbench image contract")


if __name__ == "__main__":
    main()
