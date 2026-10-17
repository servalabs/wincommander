# SPDX-License-Identifier: AGPL-3.0-or-later
"""Behavior tests for the offline source inventory; uses disposable Git repos."""

import contextlib
import hashlib
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import crypto_inventory as inventory


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.git("init", "-q")
        self.git(
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        )

    def git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.root), *args], check=True, capture_output=True
        ).stdout

    def track(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        self.git("add", "--", name)
        return path

    def test_reports_indicators_and_lines_without_source_or_secret_values(self):
        path = self.track(
            "src/keys.rs",
            '// Ed25519\nlet key = "never-output-this-value";\nuse aes_gcm::Aes256Gcm;\n',
        )
        result = inventory.collect_inventory(self.root)
        self.assertEqual(result["kind"], "lexical-source-inventory")
        self.assertTrue(result["has_tracked_changes"])
        self.assertEqual(
            result["files"],
            [
                {
                    "path": "src/keys.rs",
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                    "occurrences": [
                        {"line": 1, "indicator": "Ed25519"},
                        {"line": 3, "indicator": "AES-GCM"},
                    ],
                }
            ],
        )
        self.assertNotIn("never-output-this-value", json.dumps(result))

    def test_scans_working_copy_and_does_not_modify_source_or_index(self):
        path = self.track("code.rs", "// Ed25519\n")
        path.write_text("// ML-KEM-768\n", encoding="utf-8")
        index = (self.root / ".git/index").read_bytes()
        before = path.read_bytes()
        first = inventory.collect_inventory(self.root)
        self.assertEqual(first, inventory.collect_inventory(self.root))
        self.assertEqual(
            first["files"][0]["occurrences"], [{"line": 1, "indicator": "ML-KEM"}]
        )
        self.assertEqual(index, (self.root / ".git/index").read_bytes())
        self.assertEqual(before, path.read_bytes())

    def test_ignores_untracked_ignored_and_out_of_scope_data(self):
        self.track(".gitignore", "ignored.rs\n")
        self.track("keys.pem", "Ed25519 secret\n")
        self.track("state.dat", "AES-GCM\n")
        for name in ("untracked.rs", "ignored.rs"):
            (self.root / name).write_text("Ed25519", encoding="utf-8")
        result = inventory.collect_inventory(self.root)
        self.assertEqual(result["files"], [])
        self.assertEqual(result["skipped"], {"outside_suffix_scope": 3})

    def test_reports_missing_binary_undecodable_and_oversized_files(self):
        self.track("deleted.rs", "Ed25519").unlink()
        for name, data in (("binary.rs", b"\0Ed25519"), ("invalid.rs", b"\x80Ed25519")):
            path = self.track(name, "")
            path.write_bytes(data)
        self.track("large.rs", "Ed25519 " * 10)
        with patch.object(inventory, "MAX_FILE_BYTES", 40):
            result = inventory.collect_inventory(self.root)
        self.assertEqual(result["files"], [])
        self.assertEqual(
            result["skipped"],
            {
                "binary_or_undecodable": 2,
                "missing_working_file": 1,
                "oversized_file": 1,
            },
        )

    def test_handles_utf8_bom_utf16_and_space_in_filename(self):
        for name, encoding in (("space name.rs", "utf-8-sig"), ("wide.ps1", "utf-16")):
            path = self.track(name, "")
            path.write_bytes("// DPAPI\n".encode(encoding))
        result = inventory.collect_inventory(self.root)
        self.assertEqual(len(result["files"]), 2)
        self.assertTrue(
            all(
                item["occurrences"] == [{"line": 1, "indicator": "DPAPI"}]
                for item in result["files"]
            )
        )

    def test_never_follows_tracked_symlink_or_submodule_entries(self):
        for mode in ("120000", "160000"):
            with (
                self.subTest(mode=mode),
                patch.object(Path, "open", side_effect=AssertionError("read")),
            ):
                self.assertEqual(
                    inventory.read_source(self.root, "link.rs", mode),
                    (None, "non_regular_index_entry"),
                )

    def test_rejects_parent_escape_even_for_regular_index_mode(self):
        self.assertEqual(
            inventory.read_source(self.root, "../outside.rs", "100644"),
            (None, "linked_or_external_path"),
        )

    def test_fails_on_nested_root_and_unmerged_index(self):
        nested = self.root / "nested"
        nested.mkdir()
        with self.assertRaisesRegex(ValueError, "worktree root"):
            inventory.collect_inventory(nested)
        original = inventory.run_git

        def unmerged(root, *args):
            if args[0] == "ls-files":
                return b"100644 abc 1\tconflict.rs\0"
            return original(root, *args)

        with (
            patch.object(inventory, "run_git", side_effect=unmerged),
            self.assertRaisesRegex(ValueError, "unmerged"),
        ):
            inventory.collect_inventory(self.root)

    def test_cli_errors_do_not_echo_subprocess_output(self):
        error = subprocess.CalledProcessError(1, "git", stderr=b"private-source-value")
        stderr = io.StringIO()
        with (
            patch("sys.argv", ["crypto_inventory.py"]),
            patch.object(inventory, "collect_inventory", side_effect=error),
            contextlib.redirect_stderr(stderr),
        ):
            self.assertEqual(inventory.main(), 2)
        self.assertNotIn("private-source-value", stderr.getvalue())

    def test_recognizes_hybrid_provider_names_without_claiming_negotiation(self):
        indicators = inventory.scan_text(
            "X25519MLKEM768 ML-DSA-65 ECDSA_P256 enc:v2:\n"
        )
        self.assertEqual(
            {item["indicator"] for item in indicators},
            {"ML-KEM", "ML-DSA", "ECDSA", "encrypted-envelope"},
        )

    def test_recognizes_packaging_cipher_and_manifest_dependencies(self):
        self.assertEqual(
            inventory.scan_text('createCipheriv("aes-256-gcm", key, iv)'),
            [{"line": 1, "indicator": "AES-GCM"}],
        )
        self.track("Cargo.lock", 'name = "ed25519-dalek"\nversion = "2.2.0"\n')
        result = inventory.collect_inventory(self.root)
        self.assertEqual(result["files"][0]["path"], "Cargo.lock")
        self.assertEqual(
            result["files"][0]["occurrences"], [{"line": 1, "indicator": "Ed25519"}]
        )
        self.assertEqual(result["included_suffixes"], sorted(inventory.SOURCE_SUFFIXES))


if __name__ == "__main__":
    unittest.main()
