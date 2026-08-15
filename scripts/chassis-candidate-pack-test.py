#!/usr/bin/env python3
"""Black-box proof for the exact Candidate-to-chassis pack adapter."""

from __future__ import annotations

import hashlib
import json
import os
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
MAX_LOADER_BYTES = 2 * 1024 * 1024


def run_pack(
    repo: Path,
    candidate: Path,
    output: Path,
    version: str,
    source_sha: str,
    env: dict[str, str],
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(repo / "scripts/chassis-candidate-pack.py"),
            "--candidate-input",
            str(candidate),
            "--out",
            str(output),
            "--version",
            version,
            "--source-sha",
            source_sha,
        ],
        cwd=repo,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )


def main() -> None:
    repo = Path(__file__).resolve().parents[1]
    version = "0.1.16"
    source_sha = "1" * 40
    with tempfile.TemporaryDirectory(prefix="chassis-candidate-pack-test-") as tmp_raw:
        tmp = Path(tmp_raw)
        candidate = tmp / "candidate-input"
        candidate.mkdir()
        (candidate / f"agenterm-{version}-sbom.spdx.json").write_text(
            '{"spdxVersion":"SPDX-2.3"}\n', encoding="utf-8"
        )
        expected = {}
        for index, cell in enumerate(CELLS):
            payload = f"THIN-CANDIDATE-L1:{cell}\n".encode() + bytes([index]) * 19
            source = tmp / f"loader-{cell}"
            source.write_bytes(payload)
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
                raise SystemExit(f"L1 stage failed\n{staged.stdout}\n{staged.stderr}")
            expected[cell] = hashlib.sha256(payload).hexdigest()

        blocker = tmp / "bin"
        blocker.mkdir()
        cargo = blocker / ("cargo.cmd" if os.name == "nt" else "cargo")
        cargo.write_text(
            "@exit /b 99\r\n" if os.name == "nt" else "#!/bin/sh\nexit 99\n",
            encoding="utf-8",
        )
        if os.name != "nt":
            cargo.chmod(cargo.stat().st_mode | stat.S_IEXEC)
        output = tmp / f"agenterm-{version}-chassis-product.tgz"
        env = os.environ.copy()
        env["PATH"] = str(blocker)
        env.pop("CARGO", None)
        proc = run_pack(repo, candidate, output, version, source_sha, env)
        if proc.returncode != 0:
            raise SystemExit(f"Candidate pack failed\n{proc.stdout}\n{proc.stderr}")
        report = json.loads(proc.stdout)
        if report["kind"] != "agenterm-chassis-candidate-pack":
            raise SystemExit("Candidate pack identity is missing")
        if report["invokes_cargo"] is not False:
            raise SystemExit("Candidate pack must not invoke cargo")
        if {cell: item["sha256"] for cell, item in report["l1"].items()} != expected:
            raise SystemExit("Candidate archive identity changed")
        checksum = output.with_name(output.name + ".sha256")
        provenance_path = output.with_name(output.name + ".provenance.json")
        if not checksum.is_file() or not provenance_path.is_file():
            raise SystemExit("Candidate chassis companions are missing")
        provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
        if (
            provenance["artifact"] != output.name
            or provenance["os"] != "chassis"
            or provenance["arch"] != "multi"
            or provenance["invokes_cargo"] is not False
        ):
            raise SystemExit("Candidate chassis provenance is invalid")
        with tarfile.open(output, "r:gz") as archive:
            identity = json.load(archive.extractfile("l3/product-identity.json"))
            if identity["version"] != version or identity["source_sha"] != source_sha:
                raise SystemExit("product identity does not bind version and source SHA")
            for cell, digest in expected.items():
                if archive.getmember(f"l1/{cell}/loader").mode != 0o755:
                    raise SystemExit(f"L1 mode changed for {cell}")
                payload = archive.extractfile(f"l1/{cell}/loader").read()
                if hashlib.sha256(payload).hexdigest() != digest:
                    raise SystemExit(f"L1 bytes changed for {cell}")

        fat_candidate = tmp / "fat-candidate-input"
        fat_candidate.mkdir()
        (fat_candidate / f"agenterm-{version}-sbom.spdx.json").write_text(
            '{"spdxVersion":"SPDX-2.3"}\n', encoding="utf-8"
        )
        for cell in CELLS:
            (fat_candidate / f"agenterm-{version}-{cell}.tar.gz").write_bytes(
                b"fat-platform-archive"
            )
        fat_rejected = run_pack(
            repo,
            fat_candidate,
            tmp / "fat-archive.tgz",
            version,
            source_sha,
            env,
        )
        if (
            fat_rejected.returncode == 0
            or "typed Candidate L1 loader is missing" not in fat_rejected.stderr
        ):
            raise SystemExit("fat platform archives were accepted as thin L1 loaders")

        descriptor_path = candidate / "chassis-l1/lnx-x86_64/loader.json"
        descriptor = json.loads(descriptor_path.read_text(encoding="utf-8"))
        original_descriptor = dict(descriptor)
        descriptor["sha256"] = "0" * 64
        descriptor_path.write_text(
            json.dumps(descriptor, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        sha_rejected = run_pack(
            repo,
            candidate,
            tmp / "bad-descriptor-sha.tgz",
            version,
            source_sha,
            env,
        )
        if sha_rejected.returncode == 0 or "descriptor mismatch" not in sha_rejected.stderr:
            raise SystemExit("tampered typed L1 descriptor SHA was not rejected")

        descriptor = dict(original_descriptor)
        descriptor["bytes"] += 1
        descriptor_path.write_text(
            json.dumps(descriptor, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        bytes_rejected = run_pack(
            repo,
            candidate,
            tmp / "bad-descriptor-bytes.tgz",
            version,
            source_sha,
            env,
        )
        if (
            bytes_rejected.returncode == 0
            or "descriptor mismatch" not in bytes_rejected.stderr
        ):
            raise SystemExit("tampered typed L1 descriptor byte count was not rejected")
        descriptor_path.write_text(
            json.dumps(original_descriptor, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

        (candidate / "chassis-l1/lnx-x86_64/loader").write_bytes(b"tampered")
        rejected = run_pack(
            repo,
            candidate,
            tmp / "tampered.tgz",
            version,
            source_sha,
            env,
        )
        if rejected.returncode == 0 or "descriptor mismatch" not in rejected.stderr:
            raise SystemExit("tampered typed L1 loader was not rejected")

        oversized = tmp / "oversized-loader"
        oversized.write_bytes(b"x" * (MAX_LOADER_BYTES + 1))
        oversized_rejected = subprocess.run(
            [
                sys.executable,
                str(repo / "scripts/chassis-stage-l1-loader.py"),
                "--loader",
                str(oversized),
                "--cell",
                "lnx-x86_64",
                "--version",
                version,
                "--source-sha",
                source_sha,
                "--out",
                str(tmp / "oversized-stage"),
            ],
            cwd=repo,
            check=False,
            capture_output=True,
            text=True,
        )
        if (
            oversized_rejected.returncode == 0
            or f"1..{MAX_LOADER_BYTES} bytes" not in oversized_rejected.stderr
        ):
            raise SystemExit(">2 MiB thin L1 loader was not rejected by staging")
        print("PASS: Candidate six-cell thin loaders compose into 0.1.16 chassis without cargo")


if __name__ == "__main__":
    main()
