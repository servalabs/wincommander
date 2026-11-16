# Research — WinCommander Free

Explored ideas, measured findings, rejected options and conditional future
opportunities. Conclusions only: commitments are in [ROADMAP.md](ROADMAP.md),
deliberate exclusions in [NON-GOALS.md](NON-GOALS.md), and design decisions in
[ARCHITECTURE.md](ARCHITECTURE.md). Items marked "dev notes" come from design
notes that exist only on the `dev` branch and describe unimplemented, unshipped
work.

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
  too heavy for an always-on monitor (still open in ROADMAP.md).
- **Parent-directory pinning on Windows (dev notes).** In a restricted-token
  fixture test on one machine (2026-11-06), a directory pinned with read access
  and delete sharing denied could still be turned into a junction. This was not
  a standard-user test or a test against a real ProgramData ACL, so it neither
  proves a product weakness nor safety. Conclusion: do not rely on holding such
  a pin as the only substitution defence in the settings cutover design.

## Rejected or ruled-out options

- Rewriting the Everything query on the backend only: rejected in favour of
  pre-split tokens from the frontend plus backend validation of every token.
- Moving the Vault mount to SYSTEM context to simplify RDP dismount (see above).
- Automatic process kill without attribution in the Free ransomware monitor.
- A joined single-argument query string for `es.exe` (see above).

## Conditional future opportunities

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
- **Versioned encrypted settings generations (dev notes).** A cutover record
  stored beside the data it protects is not an anti-rollback anchor: restoring
  an older matching record and generation is undetectable. A usable design needs
  an independent anchor or an explicitly accepted recovery policy, installer
  provisioning against a held known-folder root, coordination of every writer
  (including early headless command paths that read settings before UI setup),
  and elevated, LocalSystem and standard-user acceptance runs.
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
