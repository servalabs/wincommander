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

The opt-in `experimental-signer` library also exposes `release::sign_release`,
which checks release metadata and the exact artifact before calling either
signing callback. It does not provide a key store or publisher. Callers must
keep the verified artifact immutable through publication; this helper alone
does not establish a safe release process.

A separate opt-in `wincmd-pq-release-prepare` command rehearses detached
signing without loading private keys. Build with `--features experimental-signer
--bin wincmd-pq-release-prepare`. Given the same independently trusted key file:

```text
wincmd-pq-release-prepare prepare payload.json pro.exe trusted-keys.json 3.7.0 7 ed.msg pq.msg
wincmd-pq-release-prepare finish payload.json pro.exe trusted-keys.json 3.7.0 7 ed.sig pq.sig hybrid.json
```

The first step validates the file and emits the exact binary messages for the
Ed25519 and ML-DSA-65 signing providers. Each provider must review the release
and sign its respective bytes; the signatures are raw binary (64 and 3309
bytes). The second step rechecks the file and both signatures, and refuses to
overwrite existing outputs. These commands do not load or manage signing keys,
publish files, prevent later file replacement, or establish a production
signing ceremony. Keep the artifact immutable until publication.
