"""Populate missing Ubuntu Cargo cache entries from an existing Windows cache.

Run from WSL with Python 3.11+: ``python3 tests/offline-cargo-registry-bridge.py
apps/desktop/src-tauri/Cargo.lock /mnt/c/Users/saba/.cargo/registry
/home/saba/.cargo/registry``. Does not access the network. Source archives must
match the exact SHA-256 recorded in the supplied Cargo.lock. Never overwrites.
The sparse registry index is a cache hint only; package bytes are lock-pinned.
"""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import sys
import tomllib
from pathlib import Path


REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
REGISTRY_ID = "index.crates.io-1949cf8c6b5b557f"
NAME = re.compile(r"^[a-zA-Z0-9_-]{1,128}$")
VERSION = re.compile(r"^[0-9][a-zA-Z0-9.+_-]{0,127}$")


def index_key(name: str) -> Path:
    if len(name) == 1:
        return Path("1") / name
    if len(name) == 2:
        return Path("2") / name
    if len(name) == 3:
        return Path("3") / name[0] / name
    return Path(name[:2]) / name[2:4] / name


def sha256(path: Path) -> str:
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def copy_missing(source: Path, target: Path, *, checksum: str | None = None) -> bool:
    if target.exists() or target.is_symlink():
        if checksum is not None and sha256(target) != checksum:
            raise ValueError(f"Existing cache file has invalid lockfile digest: {target}")
        return False
    if not source.is_file() or source.is_symlink():
        return False
    if checksum is not None and sha256(source) != checksum:
        raise ValueError(f"Windows source archive differs from Cargo.lock: {source}")
    target.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive destination; never replace a worker's existing registry entry.
    with source.open("rb") as reader, target.open("xb") as writer:
        shutil.copyfileobj(reader, writer, length=1 << 20)
        writer.flush()
        os.fsync(writer.fileno())
    if checksum is not None and sha256(target) != checksum:
        raise ValueError(f"Copied archive digest mismatch: {target}")
    return True


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    lock, windows, linux = map(lambda p: Path(p).resolve(), sys.argv[1:])
    if windows == linux or not lock.is_file() or not windows.is_dir() or not linux.is_dir():
        raise ValueError("Expected an existing lockfile and two distinct registry roots")
    data = tomllib.loads(lock.read_text(encoding="utf-8"))
    index_added = archives_added = unavailable = 0
    for package in data["package"]:
        if package.get("source") != REGISTRY:
            continue
        name, version, checksum = package["name"], package["version"], package["checksum"]
        if not NAME.fullmatch(name) or not VERSION.fullmatch(version) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise ValueError("Invalid locked registry identity")
        archive = f"{name}-{version}.crate"
        origin = windows / "cache" / REGISTRY_ID / archive
        destination = linux / "cache" / REGISTRY_ID / archive
        if copy_missing(origin, destination, checksum=checksum):
            archives_added += 1
        elif not destination.exists():
            unavailable += 1
        key = index_key(name)
        if copy_missing(
            windows / "index" / REGISTRY_ID / ".cache" / key,
            linux / "index" / REGISTRY_ID / ".cache" / key,
        ):
            index_added += 1
    print(f"OFFLINE_LOCKED_ARCHIVES_ADDED={archives_added}")
    print(f"OFFLINE_SPARSE_INDEX_ENTRIES_ADDED={index_added}")
    print(f"LOCKED_ARCHIVES_UNAVAILABLE_IN_BOTH_CACHES={unavailable}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
