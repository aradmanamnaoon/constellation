"""Negative checks for the read-only Windows installer payload provenance gate."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


SOURCE = Path(__file__).with_name("nsis-payload-compare.py")
SPEC = importlib.util.spec_from_file_location("nsis_payload_compare", SOURCE)
assert SPEC is not None and SPEC.loader is not None
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class PayloadComparisonTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.direct = Path(self.directory.name, "direct.exe")
        self.installed = Path(self.directory.name, "installed.exe")
        self.original = b"MZ" + bytes(53) + b"__TAURI_BUNDLE_TYPE_VAR_UNK" + bytes(75)
        self.direct.write_bytes(self.original)

    def test_identical_executables(self) -> None:
        self.installed.write_bytes(self.original)
        observed = module.inspect(self.direct, self.installed)
        self.assertEqual(observed["status"], "exact_match")
        self.assertEqual(observed["difference_count"], 0)

    def test_only_known_three_byte_tauri_nsis_marker_is_allowed(self) -> None:
        self.installed.write_bytes(
            self.original.replace(b"__TAURI_BUNDLE_TYPE_VAR_UNK", b"__TAURI_BUNDLE_TYPE_VAR_NSS")
        )
        observed = module.inspect(self.direct, self.installed)
        self.assertEqual(observed["status"], "tauri_marker_only")
        self.assertEqual(observed["difference_count"], 3)

    def test_even_one_extra_changed_byte_is_rejected(self) -> None:
        mutated = bytearray(
            self.original.replace(b"__TAURI_BUNDLE_TYPE_VAR_UNK", b"__TAURI_BUNDLE_TYPE_VAR_NSS")
        )
        mutated[-2] ^= 1
        self.installed.write_bytes(mutated)
        with self.assertRaisesRegex(ValueError, "Payload differs"):
            module.inspect(self.direct, self.installed)

    def test_truncation_and_unrelated_three_byte_patch_are_rejected(self) -> None:
        self.installed.write_bytes(self.original[:-1])
        with self.assertRaisesRegex(ValueError, "lengths differ"):
            module.inspect(self.direct, self.installed)
        altered = bytearray(self.original)
        altered[-3:] = b"NSS"
        self.installed.write_bytes(altered)
        with self.assertRaisesRegex(ValueError, "Payload differs"):
            module.inspect(self.direct, self.installed)


if __name__ == "__main__":
    unittest.main()
