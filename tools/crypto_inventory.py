#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Print a read-only lexical crypto inventory: python tools/crypto_inventory.py --root .

Scans Git-tracked working-copy text, including tests and comments. Occurrences
are review leads, not proof of execution, security, or negotiated algorithms.
No source excerpts, key values, application data, or network access are used.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

MAX_FILE_BYTES = 4 * 1024 * 1024
SOURCE_SUFFIXES = {
    ".rs",
    ".ts",
    ".tsx",
    ".js",
    ".mjs",
    ".c",
    ".cpp",
    ".h",
    ".hpp",
    ".ps1",
    ".sh",
    ".nsi",
    ".nsh",
    ".toml",
    ".json",
    ".yaml",
    ".yml",
    ".lock",
}
PATTERNS = {
    "AES-GCM": r"\b(?:Aes256Gcm|aes[-_](?:(?:128|192|256)[-_])?gcm)\b",
    "AES-XTS": r"\b(?:XTS|AES[-_]XTS)\b",
    "Argon2": r"\bargon2(?:id)?\b",
    "XChaCha20-Poly1305": r"\b(?:XChaCha20Poly1305|XChaCha20-Poly1305)\b",
    "HMAC": r"\b(?:Hmac(?:Sha256)?|HMAC[-_]SHA[-_]?256)\b",
    "SHA-256": r"\bsha[-_]?256\b",
    "SHA-384": r"\bsha[-_]?384\b",
    "SHA-512": r"\bsha[-_]?512\b",
    "SHA-2-family": r"\bsha2\b",
    "Ed25519": r"\bed25519(?:[-_]dalek)?\b",
    "ECDSA": r"\b(?:ECDSA(?:_P256)?|ES256|p256)\b",
    "RSA": r"\b(?:RSA|RS256|RS384|RS512|PS256)\b",
    "DH": r"\b(?:DH|Diffie[- ]Hellman)\b",
    "X25519": r"\bx25519\b",
    "ML-KEM": r"\b(?:ML[-_]?KEM(?:[-_]?(?:512|768|1024))?|X25519MLKEM768|SECP256R1MLKEM768)\b",
    "ML-DSA": r"\bML[-_]?DSA(?:[-_]?(?:44|65|87))?\b",
    "SLH-DSA": r"\bSLH[-_]?DSA\b",
    "DPAPI": r"\b(?:DPAPI|CryptProtectData|CryptUnprotectData)\b",
    "Authenticode": r"\b(?:Authenticode|WinVerifyTrust|signtool)\b",
    "minisign": r"\bminisign(?:[-_]verify)?\b",
    "rustls": r"\brustls\b",
    "aws-lc": r"\baws[-_]lc(?:[-_](?:rs|sys))?\b",
    "ring": r"\bring\b",
    "OS-randomness": r"\b(?:OsRng|BCryptGenRandom|getrandom|crypto\.getRandomValues)\b",
    "encrypted-envelope": r"\benc:v[123]:",
}
RULES = {name: re.compile(pattern, re.IGNORECASE) for name, pattern in PATTERNS.items()}


def run_git(root: Path, *args: str) -> bytes:
    return subprocess.run(
        [
            "git",
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-C",
            str(root),
            *args,
        ],
        check=True,
        capture_output=True,
    ).stdout


def scan_text(text: str) -> list[dict]:
    occurrences = []
    for number, line in enumerate(text.splitlines(), 1):
        for algorithm, pattern in sorted(RULES.items()):
            if pattern.search(line):
                occurrences.append({"line": number, "indicator": algorithm})
    return occurrences


def read_source(root: Path, relative: str, mode: str) -> tuple[bytes | None, str]:
    if mode not in {"100644", "100755"}:
        return None, "non_regular_index_entry"
    path = root / relative
    if path.suffix.lower() not in SOURCE_SUFFIXES:
        return None, "outside_suffix_scope"
    # Tracked links and parent-directory junctions must never lead into user data.
    if path.is_symlink() or not path.resolve().is_relative_to(root):
        return None, "linked_or_external_path"
    if not path.is_file():
        return None, "missing_working_file"
    with path.open("rb") as source:
        data = source.read(MAX_FILE_BYTES + 1)
    if len(data) > MAX_FILE_BYTES:
        return None, "oversized_file"
    return data, ""


def decode_source(data: bytes) -> str:
    if data.startswith((b"\xff\xfe", b"\xfe\xff")):
        return data.decode("utf-16")
    if b"\0" in data:
        raise UnicodeError("binary source")
    return data.decode("utf-8-sig")


def collect_inventory(root: Path) -> dict:
    root = root.resolve(strict=True)
    actual_root = Path(
        run_git(root, "rev-parse", "--show-toplevel").decode().strip()
    ).resolve()
    if root != actual_root:
        raise ValueError("--root must be a Git worktree root")
    entries = run_git(root, "ls-files", "--stage", "-z").split(b"\0")
    files = []
    skipped: Counter = Counter()
    scanned = 0
    for entry in sorted(filter(None, entries)):
        metadata, raw_path = entry.split(b"\t", 1)
        mode, _, stage = metadata.decode("ascii").split()
        if stage != "0":
            raise ValueError(
                "resolve unmerged index entries before generating an inventory"
            )
        relative = raw_path.decode("utf-8")
        data, reason = read_source(root, relative, mode)
        if data is None:
            skipped[reason] += 1
            continue
        try:
            source = decode_source(data)
        except UnicodeError:
            skipped["binary_or_undecodable"] += 1
            continue
        scanned += 1
        occurrences = scan_text(source)
        if occurrences:
            files.append(
                {
                    "path": relative,
                    "sha256": hashlib.sha256(data).hexdigest(),
                    "occurrences": occurrences,
                }
            )
    return {
        "schema_version": 1,
        "kind": "lexical-source-inventory",
        "head": run_git(root, "rev-parse", "HEAD").decode("ascii").strip(),
        "has_tracked_changes": bool(
            run_git(root, "status", "--porcelain", "--untracked-files=no")
        ),
        "scope": "Git-tracked working-copy source/config/lock text; includes comments and tests",
        "included_suffixes": sorted(SOURCE_SUFFIXES),
        "limitations": [
            "Partial lexical indicators only; not a complete cryptographic bill of materials.",
            "Untracked files, submodules, dependencies, binaries and deployment state are not scanned.",
            "No call-graph, feature, purpose, key-lifecycle, PQ-security or TLS-negotiation proof.",
        ],
        "scanned_files": scanned,
        "skipped": dict(sorted(skipped.items())),
        "files": sorted(files, key=lambda item: item["path"]),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    try:
        inventory = collect_inventory(args.root)
    except (OSError, ValueError, UnicodeError, subprocess.CalledProcessError):
        # Raw subprocess/decoder errors can include source bytes or local details.
        print(
            "Crypto inventory failed: check the Git root, index and readable source files.",
            file=sys.stderr,
        )
        return 2
    print(json.dumps(inventory, indent=2, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
