"""Tamper and overwrite safety for the offline lockfile-bound cache bridge."""

from __future__ import annotations

import hashlib
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


bridge = None
if sys.version_info >= (3, 11):
    source = Path(__file__).with_name("offline-cargo-registry-bridge.py")
    spec = importlib.util.spec_from_file_location("offline_cargo_bridge", source)
    assert spec is not None and spec.loader is not None
    bridge = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bridge)


@unittest.skipIf(bridge is None, "Cargo cache bridge requires stdlib tomllib (Python 3.11+)")
class OfflineBridgeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.origin = Path(self.temporary.name, "windows.crate")
        self.destination = Path(self.temporary.name, "linux", "crate.crate")
        self.data = b"test-only exact source archive bytes"
        self.origin.write_bytes(self.data)
        self.digest = hashlib.sha256(self.data).hexdigest()

    def test_verified_copy_and_no_overwrite(self) -> None:
        self.assertTrue(bridge.copy_missing(self.origin, self.destination, checksum=self.digest))
        self.assertEqual(self.destination.read_bytes(), self.data)
        self.assertFalse(bridge.copy_missing(self.origin, self.destination, checksum=self.digest))

    def test_modified_source_and_destination_are_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "differs from Cargo.lock"):
            bridge.copy_missing(self.origin, self.destination, checksum="0" * 64)
        self.assertFalse(self.destination.exists())
        self.destination.parent.mkdir(parents=True, exist_ok=True)
        self.destination.write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "invalid lockfile digest"):
            bridge.copy_missing(self.origin, self.destination, checksum=self.digest)

    def test_correct_crates_io_sparse_index_layout(self) -> None:
        self.assertEqual(str(bridge.index_key("a")), "1/a")
        self.assertEqual(str(bridge.index_key("ab")), "2/ab")
        self.assertEqual(str(bridge.index_key("abc")), "3/a/abc")
        self.assertEqual(str(bridge.index_key("nom_locate")), "no/m_/nom_locate")


if __name__ == "__main__":
    unittest.main()
