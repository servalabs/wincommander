# Experimental offline Pro release check

This is a local rehearsal, not a production verifier. It is not enabled in
WinCommander and has no production keys. Do not use it to certify a release.
It verifies both signatures, the expected version, the fixed Pro release URL,
and the actual file size and SHA-256 without a network connection. It cannot
check key revocation, persistent rollback history, Windows signatures, channel,
architecture, or provenance. Old classical releases are not accepted.

Build from `src-tauri` with:

```text
cargo build --locked -p wincmd-pq-experiment --features experimental-offline-verifier --bin wincmd-pq-release-check
```

Run with a signed record, downloaded executable, independently trusted key
configuration, and expected version:

```text
wincmd-pq-release-check hybrid.json wincommander-pro.exe trusted-keys.json 3.7.0
```

The trust file is a JSON object with `ed_key_id`, `ed_public`, `pq_key_id`,
`pq_public`, and positive `minimum_epoch`. Public keys use unpadded base64url;
Ed25519 keys are 32 bytes and ML-DSA-65 keys are 1952 bytes. The key file and
minimum epoch must come from a separately trusted source, never the download
or record being checked. The caller must obtain fresh revocation and rollback
policy independently; this tool cannot enforce those requirements.
