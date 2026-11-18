# Research — WinCommander Free

Explored ideas, measured findings, rejected options and conditional future
opportunities. Conclusions only: commitments are in the private open-work ledger,
deliberate exclusions in [NON-GOALS.md](NON-GOALS.md), and design decisions in
[ARCHITECTURE.md](ARCHITECTURE.md). Items marked "dev notes" come from design
notes that were kept on the `dev` branch before it merged into `main` and describe
unimplemented, unshipped work.

## Measured findings

- **Everything CLI argument handling.** `es.exe` (v1.1.0.37) joins its non-flag
  arguments with spaces, but a single argument that already contains a space is
  read as a quoted phrase. Passing a whole multi-term query as one argument
  returned zero rows with no error (one argument: 0 rows; the same terms as two
  arguments: 3 rows). This had made the app's own app-priority query never match
  and shipped before it was found. Conclusion: send one token per argument and
  never a joined string (rule in ARCHITECTURE.md).
- **RDP idle dismount and the mount namespace.** A VeraCrypt volume mounted by
  the elevated logged-in user lands in the global object namespace, so a SYSTEM
  session-0 task can see and force-dismount it (empirically verified).
  Conclusion: dismount from a SYSTEM scheduled task, and do not move the mount
  itself to SYSTEM context, which would hide the drive from the user.
- **RDS session limits have no single source of truth.** Windows applies them
  through a four-layer precedence cascade; the policy editor can read "Not
  configured" while the registry still enforces a one-minute logoff.
  WinCommander therefore writes its own idle-timeout setting and does not trust
  a single policy view.
- **Hardware secure erase is not universal.** A drive may advertise ATA
  Security/Sanitize or NVMe Sanitize/Format, but a USB bridge or an ATA
  security-frozen state can block it, on a live Windows install it is generally
  reachable only from a pre-boot environment, and crypto-erase is the only
  drive-agnostic guarantee. Conclusion: ship read-only detection first
  (`storage_probe.rs`), and keep any erase feature held.
- **DNS-over-HTTPS bootstrap.** A DoH-only resolver stalled on some networks even
  when a direct request to the resolver worked. Conclusion: system DNS first,
  then a hostname-pinned Cloudflare DoH fallback with a five-second timeout
  (`net.rs`); artifact verification does not depend on DNS.
- **Ransomware monitor, version 1.** File-watch events do not identify the
  modifying process, so the Free monitor counts modify events only and tells the
  user what to do; it does not suspend or kill automatically, because acting
  without process attribution risks stopping the wrong process. Reading file
  contents before and after every write to compute an entropy delta was judged
  too heavy for an always-on monitor; pure entropy helpers exist in
  `wincmd-shared` but the monitor does not use them (still open).
- **Parent-directory pinning on Windows (dev notes).** In a restricted-token
  fixture test on one machine (2026-11-06), a directory pinned with read access
  and delete sharing denied could still be turned into a junction. This was not
  a standard-user test or a test against a real ProgramData ACL, so it neither
  proves a product weakness nor safety. Conclusion: do not rely on holding such
  a pin as the only substitution defence in the settings cutover design.
- **Pinned parent directory can still become a junction (probe, 2026-11-06).**
  A directory pinned with the settings migration's Windows read/share flags
  (a read handle denying delete sharing, plus a second handle requesting
  `0x116`) could still be turned into a junction in a disposable fixture, using
  a restricted LUA token (maximum privileges disabled, Administrators not
  enabled, Users enabled) against a protected fixture ACL (Users W,RX; SYSTEM
  and Administrators full control). With `--deny-write` the mutation handle
  failed with sharing violation 32; while that pin was held an existing child
  could be read and a new child could still be created, and a pre-existing
  directory write handle prevented acquiring the pin. This is not a separately
  signed-in standard-user test, not run against the real ProgramData ACL, and
  not proof the product path is exploitable; a failed attempt may reflect token
  privileges or filesystem policy, and holding such a pin may conflict with
  unrelated software. Reproduce with `tools/settings-parent-reparse-probe.rs`
  (run instructions are in its header comment).

## Rejected or ruled-out options

- Rewriting the Everything query on the backend only: rejected in favour of
  pre-split tokens from the frontend plus backend validation of every token.
- Moving the Vault mount to SYSTEM context to simplify RDP dismount (see above).
- Automatic process kill without attribution in the Free ransomware monitor.
- A joined single-argument query string for `es.exe` (see above).

## Conditional future opportunities

- **Service-mediated evidence ledger (design only, not implemented).** Fixes the
  standard-user regression in [WEAKNESSES.md](WEAKNESSES.md#known-regressions-on-main)
  without widening the machine-state ACL, which would let any user alter or delete
  device-wide evidence and its key.
  - *Boundary:* the service (`commander-svc`, SYSTEM) becomes the only reader and
    writer of the ledger file, failure marker and key; the desktop calls it over
    the existing pipe protocol (`svc_client::call`). The machine-state ACL stays as
    it is.
  - *Operations:* `evidence.record` (any interactive user through the signed
    desktop or sidecar; the server stamps time and accepts only `source`,
    `severity`, `summary` and `detail`, with allow-listed `source`/`severity`,
    length caps and a per-caller rate limit), `evidence.read` (paged and bounded;
    whether a standard user sees other users' entries is an open privacy
    decision), `evidence.clear` (Administrators only, verified from the elevated
    token on the server) and `evidence.failure_marker` get/set (service-owned, so a
    user cannot erase it).
  - *Identity:* taken from the pipe peer (`peer_auth::SessionHelperGate`,
    `PeerIdentitySnapshot`, `TrustOrigin`), never from arguments; the service
    appends the verified SID hash and session id as server-controlled fields. This
    is an additive schema change, so confirm older readers tolerate unknown fields.
  - *Integrity:* the key never leaves the service-owned location; the existing hash
    chain and authenticated encryption stay, and only the service can extend the
    chain. The evidence lock and the failure-seen flag move into the service, and
    the client reports "service unavailable" as a distinct, visible error.
  - *Existing records:* the service runs the current `read_with_machine_source`
    merge of the current and legacy machine ledgers. The per-user legacy ledger is
    not read on the service's own authority: the calling user's client sends it in
    bounded chunks, and the service verifies and appends it as
    `imported-from-user`, once. No source file is deleted or truncated until the
    import is verified by re-reading, and a byte-for-byte backup of the machine
    ledger and key is taken first.
  - *Service down:* never write the device ledger as the user. Queue events in an
    append-only, per-user, size-capped file labelled unverified, and replay through
    `evidence.record` later, keeping the original client time as a separate field.
  - *Verification before it can ship:* service unit tests with a fake peer
    (caller-supplied identity ignored, non-admin `clear` rejected, oversize and
    rate-limit and invalid-field rejection, idempotent import and replay, a crash
    between append and acknowledge); a disposable Administrator-owned Windows
    fixture exercised by two standard accounts and SYSTEM (record, read and the
    failure marker; direct file open, key read, rename and delete all fail; the
    machine-state ACL byte-identical before and after; migration from a real-shaped
    legacy ledger keeps every record and the chain verifies; upgrade ordering
    between old and new client and service; restart; elevated clear). Real ledgers
    are preserved and never used for migration experiments.
  - *Open decisions:* visibility of other users' events, who may clear, retention
    and `MAX_ENTRIES` trimming now that one process owns the file, and whether the
    sidecar calls the service directly or through the desktop process.
- **Encrypted content index (dev notes).** If pursued: a random 256-bit key per
  Windows user and index identity protected with user-scoped DPAPI, never
  machine-scoped install material; rebuild from the original documents into a
  separate secured generation instead of migrating plaintext segments in place;
  publish with a durable atomic commit and fail closed on missing keys or
  ambiguous state; stop older writers first, since the held Tantivy writer lock
  does not fence legacy cleanup; treat the index as an eventually consistent
  cache. Open validation: large reads against the prototype range limit, crash
  and disk-full recovery, cross-user isolation and installed-service behaviour.
  Deleting files does not guarantee erasure on SSDs, backups or snapshots.
  The prototype's experimental file format v2 derives a separate key per
  random 256-bit file ID, uses distinct counter nonces for chunks and the
  footer, and caps each file at 65,536 chunks. It depends on fresh OS
  randomness across crashes, needs native streaming validation and
  cryptographic review, and cannot reopen v1 files (rebuild into a fresh
  unselected generation). Fixed test keys and the experimental `Directory`
  are not production key custody, and the portable chunk tests establish none of
  the production properties above. The index is currently plaintext and holds
  potentially sensitive extracted content; the design is disabled by default.
- **Versioned encrypted settings generations (dev notes).** A cutover record
  stored beside the data it protects is not an anti-rollback anchor: restoring
  an older matching record and generation is undetectable. A usable design needs
  an independent anchor or an explicitly accepted recovery policy, installer
  provisioning against a held known-folder root, coordination of every writer
  (including early headless command paths that read settings before UI setup),
  and elevated, LocalSystem and standard-user acceptance runs.
  Further constraints found while building the gated experiment:
  - Routing and scope: normal builds keep the current format and guarded-key
    writes. The non-default `experimental-settings-generation` feature (plus
    `WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION=1` in debug builds) routes
    section reads and writes through the experimental lock and anchor; nothing
    provisions the lock or starts a generation in normal startup or the
    installer. Per-user installs use user-scoped DPAPI for a new generation,
    fail with an account/profile recovery message if Windows cannot unlock it,
    and never retry with machine scope; SSH or noninteractive access may need
    the profile loaded and is not accepted until tested.
  - Installer: the post-install hook must not invoke the provisioner until a
    verified known-folder root and parent ACL are held against substitution,
    the lock file's exact ACL is checked on the held handle, and elevated
    Administrator, LocalSystem and standard-user tests pass. Stopping the named
    desktop image and service is not proof every older writer has stopped, and
    process scans neither stop a later launch nor identify renamed copies.
  - Selection gate: it must run before every settings read or write, including
    paths that execute before Tauri `setup`. Stable data uses short-lived shared
    read ownership; pending state needs authorised exclusive recovery from a
    fresh held source snapshot; the old store and signing key stay recoverable
    and a corrupt selected generation is never silently bypassed. An existing
    store with a missing cutover anchor fails closed and is not reclassified as
    legacy. Deleting or restoring all cutover artifacts is indistinguishable
    from no prior migration, so the first-rollout assumption holds until an
    independent anchor exists.
  - Lab commands: `--settings-generation-lab status`, `fresh --offline-confirmed`
    and `migrate --offline-confirmed` can modify the real settings root, have
    not been run, and the offline flag is an operator statement rather than a
    fence. They must not reach a release installer until enforced maintenance
    mode, protected root and lock ACL acceptance, source preservation and
    crash and cross-user tests pass. The staged encrypted candidate is retained
    and the original settings and signing key stay in place.
  - Acceptance: the machine-lock fixture tests are `#[ignore]` (they need an
    elevated token). Run `cargo test -p commander-free elevated_provision_is_idempotent -- --ignored`
    and `cargo test -p commander-free settings_rotation_trial::tests -- --ignored`
    in both elevated Administrator and LocalSystem contexts, then separate
    standard-user versus privileged cross-account tests; an ignored test is not
    evidence. Also required: interruption-safe executable and service recovery,
    binding migration to the original user's identity for per-user installs, and
    source and signing-identity checks at activation.
- **Remote file search and semantic or forensic search backends.** Seams are
  reserved in `fleet-proto` (`SearchMode`) and `wincmd-search`
  (`SemanticBackend`, `ForensicBackend`); a handler and catalog entry would be
  needed. Any remote content collection must respect the Fleet boundaries in
  NON-GOALS.md.
- **Tunnel-style Fleet transport.** Only light traffic shaping (randomized
  check-in cadence, constant-size padding, optional decoy check-ins) exists in
  `fleet-agent-core`; a tunnel tier was explicitly set aside.
- **DNS canary tokens.** Only HTTP/TCP beacon canaries exist; DNS canaries
  were left for a later version.
- **Post-quantum migration.** Hybrid Ed25519 + ML-DSA-65 rehearsal tooling
  exists in `wincmd-pq-experiment`; its limits and the claim boundary are in
  that crate's docs and SECURITY.md.
