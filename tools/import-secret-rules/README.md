# Pinned content rule import

Run `python tools/import-secret-rules/import.py` from the public repository root.
Use `--check` to verify the committed table. Regeneration performs no network I/O.

The input is Gitleaks v8.24.3, SHA-256
`8f393899dbdc67e4eae580909e496acab9cf3a75aacfb6fac56b6eb16bb9e051`.
The generator imports 207 text patterns, keywords, entropy and content allowlists.
It escapes literal opening braces accepted by Go but rejected by Rust. The one
path-only PKCS12 rule is excluded; rules requiring file paths ship disabled.
File-path allowlists do not apply to clipboard content. No live secret validation
is performed. Attribution and full MIT notices are in the crate's `NOTICE`.

The `generate-secret-fixtures` crate example reproduces synthetic golden fixtures
for every imported detector. These are format fixtures, never valid credentials.
The separate PII/legacy fixtures and 10,000 benign examples exercise checksum,
context, scoring and false-positive behavior. They establish a synthetic baseline;
representative organizational data and installed-device acceptance remain separate.

Run the `scan-benchmark` example with `cargo run --release -p wincmd-clip-rules
--example scan-benchmark` from `src-tauri` for the 64 KiB timing sample.
