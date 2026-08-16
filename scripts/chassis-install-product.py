#!/usr/bin/env python3
"""Safely install one sealed Chassis product archive into a new directory."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import platform
import shutil
import stat
import tarfile
import tempfile
from pathlib import Path, PurePosixPath


CELLS = (
    "win-x86_64",
    "win-aarch64",
    "lnx-x86_64",
    "lnx-aarch64",
    "osx-x86_64",
    "osx-aarch64",
)
MAX_ARCHIVE_BYTES = 32 * 1024 * 1024
MAX_EXPANDED_BYTES = 24 * 1024 * 1024
MAX_MEMBER_BYTES = 4 * 1024 * 1024
MAX_LOADER_BYTES = 2 * 1024 * 1024
MAX_MEMBERS = 512
COPY_CHUNK = 64 * 1024
FAT_SUFFIXES = (
    ".tar.gz",
    ".tar.zst",
    ".tgz",
    ".tar",
    ".zip",
    ".7z",
    ".dmg",
    ".pkg",
    ".msi",
    ".deb",
    ".rpm",
    ".appimage",
)


class InstallError(ValueError):
    """A fail-closed product validation error."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(COPY_CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def native_cell() -> str:
    os_name = {"Windows": "win", "Linux": "lnx", "Darwin": "osx"}.get(platform.system())
    machine = platform.machine().lower()
    if machine in {"arm64", "aarch64"}:
        arch = "aarch64"
    elif machine in {"amd64", "x86_64"}:
        arch = "x86_64"
    else:
        arch = None
    if os_name is None or arch is None:
        raise InstallError("this OS/ISA has no Chassis-L1 loader cell")
    return f"{os_name}-{arch}"


def verify_checksum(archive: Path, checksum: Path) -> str:
    if not archive.is_file() or archive.is_symlink():
        raise InstallError("product archive must be a regular file")
    if not checksum.is_file() or checksum.is_symlink():
        raise InstallError("product checksum must be a regular file")
    if archive.stat().st_size == 0 or archive.stat().st_size > MAX_ARCHIVE_BYTES:
        raise InstallError("product archive size is invalid")
    lines = checksum.read_text(encoding="utf-8").splitlines()
    if len(lines) != 1:
        raise InstallError("product checksum must contain exactly one record")
    fields = lines[0].split()
    if len(fields) != 2 or fields[1].lstrip("*") != archive.name:
        raise InstallError("product checksum names a different archive")
    expected = fields[0]
    if len(expected) != 64 or any(ch not in "0123456789abcdef" for ch in expected):
        raise InstallError("product checksum is not lowercase SHA-256")
    actual = sha256_file(archive)
    if actual != expected:
        raise InstallError("product archive checksum mismatch")
    return actual


def inflate_bounded(archive: Path, raw_tar: Path) -> None:
    total = 0
    try:
        with gzip.open(archive, "rb") as source, raw_tar.open("xb") as target:
            while chunk := source.read(COPY_CHUNK):
                total += len(chunk)
                if total > MAX_EXPANDED_BYTES:
                    raise InstallError("expanded product archive exceeds size limit")
                target.write(chunk)
    except (gzip.BadGzipFile, EOFError, OSError) as error:
        raise InstallError(f"product archive is not valid gzip: {error}") from None


def safe_member_name(name: str) -> PurePosixPath:
    if not name or "\\" in name or "\x00" in name:
        raise InstallError("product archive contains a non-portable path")
    path = PurePosixPath(name)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise InstallError(f"product archive contains unsafe path: {name}")
    if path.parts[0] not in {"manifest.json", "l1", "l2", "l3"}:
        raise InstallError(f"product archive contains extra root: {path.parts[0]}")
    if path.parts[0] == "manifest.json" and len(path.parts) != 1:
        raise InstallError("manifest.json must be a root file")
    lowered = name.lower()
    if lowered.endswith(FAT_SUFFIXES):
        raise InstallError("fat platform archive is forbidden inside chassis product")
    return path


def validate_members(tar: tarfile.TarFile) -> tuple[list[tarfile.TarInfo], dict[str, tarfile.TarInfo]]:
    members = tar.getmembers()
    if not members or len(members) > MAX_MEMBERS:
        raise InstallError("product archive member count is invalid")
    seen: dict[str, tarfile.TarInfo] = {}
    total = 0
    for member in members:
        path = safe_member_name(member.name)
        canonical = path.as_posix()
        if canonical in seen:
            raise InstallError(f"product archive contains duplicate entry: {canonical}")
        seen[canonical] = member
        if not (member.isfile() or member.isdir()):
            raise InstallError(f"product archive contains forbidden entry type: {canonical}")
        if member.isfile():
            if member.size < 0 or member.size > MAX_MEMBER_BYTES:
                raise InstallError(f"product member size is invalid: {canonical}")
            total += member.size
            if total > MAX_EXPANDED_BYTES:
                raise InstallError("product members exceed expanded size limit")
        if member.mode & (stat.S_ISUID | stat.S_ISGID | stat.S_ISVTX):
            raise InstallError(f"product member has privileged mode: {canonical}")
    required = {"manifest.json", "l2/host-abi.json"}
    required.update(f"l1/{cell}/loader" for cell in CELLS)
    missing = sorted(required - seen.keys())
    if missing:
        raise InstallError(f"product archive is incomplete: {missing[0]}")
    for name in required:
        if not seen[name].isfile():
            raise InstallError(f"required product member is not a file: {name}")
    app_names = {"l3/app.json", "l3/example-app.json"} & seen.keys()
    if len(app_names) != 1:
        raise InstallError("product archive must contain exactly one canonical L3 app")
    app_name = next(iter(app_names))
    if not seen[app_name].isfile():
        raise InstallError("canonical L3 app is not a file")
    for name in seen:
        if name.startswith("l1/") and name not in required:
            raise InstallError(f"unexpected L1 payload: {name}")
    return members, seen


def read_json_member(tar: tarfile.TarFile, member: tarfile.TarInfo) -> dict:
    source = tar.extractfile(member)
    if source is None:
        raise InstallError(f"cannot read product member: {member.name}")
    try:
        value = json.load(source)
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise InstallError(f"invalid JSON in {member.name}: {error}") from None
    if not isinstance(value, dict):
        raise InstallError(f"product member must contain a JSON object: {member.name}")
    return value


def validate_manifest(tar: tarfile.TarFile, seen: dict[str, tarfile.TarInfo], selected_cell: str) -> dict:
    manifest = read_json_member(tar, seen["manifest.json"])
    if (
        manifest.get("schema") != 1
        or manifest.get("compile") is not False
        or manifest.get("invokes_cargo") is not False
    ):
        raise InstallError("product manifest compile policy is invalid")
    if manifest.get("cells") != list(CELLS):
        raise InstallError("product manifest does not name canonical six cells")
    declared_cell = manifest.get("native_cell")
    if declared_cell is not None and declared_cell != selected_cell:
        raise InstallError("product manifest native cell does not match this host")
    hashes = manifest.get("l1_sha256")
    if not isinstance(hashes, dict) or set(hashes) != set(CELLS):
        raise InstallError("product manifest L1 hash catalog is invalid")
    for cell in CELLS:
        member = seen[f"l1/{cell}/loader"]
        digest = hashes.get(cell)
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or any(ch not in "0123456789abcdef" for ch in digest)
        ):
            raise InstallError(f"product manifest has invalid loader SHA-256 for {cell}")
        if member.size == 0 or member.size > MAX_LOADER_BYTES:
            raise InstallError(f"thin L1 loader size is invalid for {cell}")
        if member.mode & 0o777 != 0o755:
            raise InstallError(f"thin L1 loader mode is invalid for {cell}")
        source = tar.extractfile(member)
        assert source is not None
        if hashlib.sha256(source.read()).hexdigest() != digest:
            raise InstallError(f"thin L1 loader hash mismatch for {cell}")
    return manifest


def extract_regular_files(tar: tarfile.TarFile, members: list[tarfile.TarInfo], root: Path) -> None:
    for member in members:
        path = safe_member_name(member.name)
        destination = root.joinpath(*path.parts)
        if member.isdir():
            destination.mkdir(parents=True, exist_ok=True)
            destination.chmod(member.mode & 0o777)
            continue
        destination.parent.mkdir(parents=True, exist_ok=True)
        source = tar.extractfile(member)
        if source is None:
            raise InstallError(f"cannot extract product member: {member.name}")
        with destination.open("xb") as target:
            shutil.copyfileobj(source, target, COPY_CHUNK)
        destination.chmod(member.mode & 0o777)


def install(archive: Path, checksum: Path, destination: Path, selected_cell: str) -> dict:
    archive_digest = verify_checksum(archive, checksum)
    if selected_cell not in CELLS:
        raise InstallError("selected native cell is not canonical")
    if destination.exists() or destination.is_symlink():
        raise InstallError("install directory already exists; refusing to overwrite")
    parent = destination.parent
    parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{destination.name}.tmp-", dir=parent))
    raw_tar = temporary / ".product.tar"
    image = temporary / "image"
    try:
        inflate_bounded(archive, raw_tar)
        image.mkdir()
        with tarfile.open(raw_tar, "r:") as tar:
            members, seen = validate_members(tar)
            manifest = validate_manifest(tar, seen, selected_cell)
            extract_regular_files(tar, members, image)
        example_app = image / "l3/example-app.json"
        installed_app = image / "l3/app.json"
        if example_app.is_file():
            shutil.copyfile(example_app, installed_app)
            installed_app.chmod(example_app.stat().st_mode & 0o777)
        raw_tar.unlink()
        os.replace(image, destination)
        temporary.rmdir()
    except (InstallError, tarfile.TarError, OSError) as error:
        shutil.rmtree(temporary, ignore_errors=True)
        if isinstance(error, InstallError):
            raise
        raise InstallError(f"product installation failed: {error}") from None
    return {
        "schema": 1,
        "kind": "agenterm-chassis-installed-product",
        "archive_sha256": archive_digest,
        "native_cell": selected_cell,
        "install_dir": str(destination),
        "manifest_schema": manifest["schema"],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True)
    parser.add_argument("--checksum", required=True)
    parser.add_argument("--install-dir", required=True)
    parser.add_argument("--native-cell", choices=CELLS, help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        result = install(
            Path(args.archive),
            Path(args.checksum),
            Path(args.install_dir),
            args.native_cell or native_cell(),
        )
    except InstallError as error:
        raise SystemExit(f"chassis install rejected: {error}") from None
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
