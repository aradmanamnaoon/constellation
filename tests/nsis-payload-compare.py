"""Read-only byte comparison of a Tauri NSIS payload and a direct Windows EXE.

The only accepted discrepancy is Tauri's documented three-byte bundle-type
marker (direct UNK versus installed NSS). Any other mutation fails the check.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


MAX_EXE_BYTES = 50 * 1024 * 1024
MARKER_PREFIX = b"__TAURI_BUNDLE_TYPE_VAR_"


def inspect(direct: Path, extracted: Path) -> dict[str, object]:
    if direct.resolve() == extracted.resolve():
        raise ValueError("Cannot compare the same executable against itself")
    for path in (direct, extracted):
        if not path.is_file() or path.is_symlink():
            raise ValueError("Expected regular executable files")
        if not 1 <= path.stat().st_size <= MAX_EXE_BYTES:
            raise ValueError("Executable outside bounded size")

    original = direct.read_bytes()
    packaged = extracted.read_bytes()
    if len(original) != len(packaged):
        raise ValueError("Executable lengths differ")
    # Count the full comparison without allocating one Python integer for
    # every differing byte in a malicious or incorrectly bundled executable.
    difference_count = 0
    first_offsets: list[int] = []
    for index, (a, b) in enumerate(zip(original, packaged)):
        if a != b:
            difference_count += 1
            if len(first_offsets) < 12:
                first_offsets.append(index)
    allowed = (
        difference_count == 3
        and first_offsets == list(range(first_offsets[0], first_offsets[0] + 3))
    )
    if allowed:
        start = first_offsets[0]
        allowed = (
            original[start - len(MARKER_PREFIX):start] == MARKER_PREFIX
            and original[start:start + 3] == b"UNK"
            and packaged[start:start + 3] == b"NSS"
        )
    if not allowed and difference_count:
        raise ValueError(
            "Payload differs beyond the expected Tauri marker: "
            f"first offsets {first_offsets}, total {difference_count}"
        )
    return {
        "status": "exact_match" if not difference_count else "tauri_marker_only",
        "bytes": len(original),
        "difference_count": difference_count,
        "difference_offset": first_offsets[0] if first_offsets else None,
        "direct_sha256": hashlib.sha256(original).hexdigest(),
        "payload_sha256": hashlib.sha256(packaged).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("direct", type=Path)
    parser.add_argument("extracted", type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.direct, args.extracted), indent=2))


if __name__ == "__main__":
    main()
