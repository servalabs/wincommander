# Roadmap — WinCommander Free

This file is the single place for planned, postponed, in-progress and
partially implemented work in this public repository. It states public product
intent only; it is not a delivery commitment and carries no dates.

- Shipped work is recorded in [CHANGELOG.md](CHANGELOG.md) and Git history, not
  here. An item is removed from this file once `main` implements it (verified
  against code and tests, not against documentation claims).
- Deliberate exclusions are owned by [NON-GOALS.md](NON-GOALS.md); this file
  does not repeat them.
- Current capabilities are in [FEATURES.md](FEATURES.md); security limits and
  the post-quantum claim boundary are in [SECURITY.md](SECURITY.md).
- Current state of `main` (which now includes the former `dev`) is in
  [STATUS.md](STATUS.md). Gated, disabled or unaccepted work that arrived with
  the 2026-11-16 merge is listed under WIP and is **not** part of any release.
- Commercial (Pro/Fleet) planning and internal limitation ledgers are not
  tracked in this repository.
- Known limits are in [WEAKNESSES.md](WEAKNESSES.md); manual and live
  acceptance work, and owner decisions, are in [USER-TASKS.md](USER-TASKS.md);
  explored ideas and conclusions are in [RESEARCH.md](RESEARCH.md). "Verification"
  lines below name checks that have not been performed.

## 1. Deferred

Consciously postponed, or blocked on an external dependency or decision.

- **Migration of legacy plaintext application logs to encrypted records.**
  New records are encrypted; older entries stay readable on disk. Blocked on a
  stronger directory-ownership boundary for safe path replacement
  (`migrate_plaintext_logs` in `src-tauri/commander-free/src/log.rs` is
  deliberately not invoked automatically). Also missing: a visible migration
  status flow. Verification: installed multi-profile Windows test.
- **Production post-quantum signing and any quantum-resistant claim.**
  Blocked on key custody and a signing ceremony, independent review, shared
  known-answer/mixed-version tests, and an application-level release
  verification path (full list in
  [SECURITY.md](SECURITY.md#cryptography-and-post-quantum-status)). Free
  release publication with a hybrid record is additionally blocked on the
  record being published and the installed update path being qualified
  (`src-tauri/wincmd-pq-experiment/OFFLINE-RELEASE-REHEARSAL.md`).
  Experimental code is listed under WIP.
- **Secure Storage copy-check and backed-up-file recovery.** The paired
  app/component change is release-gated: availability starts with a future
  release that ships both sides ([FEATURES.md](FEATURES.md), Pro section).
  Verification: matched-pair install on Windows.
- **Flow "Run now".** The button is disabled in `src/panels/flows/index.tsx`
  until actions can genuinely be run through that path; enabled rules still run
  from their triggers.
- **Canary tokens over DNS.** Only the self-hosted HTTP/TCP beacon exists
  (`src-tauri/commander-free/src/canary_tokens.rs`, "DNS canaries deferred").
- **Hardware secure-erase feature built on the drive probe.**
  `probe_drive_capabilities` (`storage_probe.rs`) is detection-only by design;
  the erase feature it would feed is held pending a separate gating decision.
- **Obfuscated tunnel-style transport for Fleet check-in.** Only light traffic
  shaping exists in `src-tauri/fleet-agent-core/src/config.rs`; the tunnel tier
  is explicitly deferred.
- **Reproducible-build CI gate (build twice, compare SHA-256).** A job is
  drafted but commented out in `.github/workflows/invariants.yml`; it cannot
  pass while release builds embed a per-build random salt (see
  [WEAKNESSES.md](WEAKNESSES.md#build-and-release)).

## 2. WIP

- **ActivityWatch classic/Tauri compatibility.** The candidate handles the
  classic API's canonical bucket route and Tauri's managed server/watchers.
  Verification: installed Windows startup, status, stop and maintenance with
  both distributions. API-key management remains outside the local viewer.

Actively in progress, or present only as an experiment, an unwired seam, or
gated and disabled by default.

On `main`, opt-in and not part of default builds:

- **Post-quantum experiment crate `wincmd-pq-experiment`.** Hybrid
  Ed25519 + ML-DSA-65 sign/verify rehearsal tools for Pro and Free release
  records, licence-device proof and evidence re-attestation, behind Cargo
  features `experimental-pq-licence`, `experimental-pq-pro-release`,
  `experimental-pq-free-release` (`src-tauri/commander-free/Cargo.toml`) and
  `experimental-offline-verifier` / `experimental-signer`. Not enabled in
  releases, no production keys, no key store or ceremony; see
  `EVIDENCE-REHEARSAL.md` and `OFFLINE-RELEASE-REHEARSAL.md` in that crate.
  CI runs its tests (`invariants.yml`). Verification: independent review and a
  qualified installed update path before any claim.

Arrived on `main` with the 2026-11-16 merge of `dev` (see [STATUS.md](STATUS.md#the-2026-11-16-merge-of-dev)); gated, disabled by default or unaccepted, and not shipped.

- **Evidence-history storage migration.** Moves evidence storage to a location
  that standard (non-administrator) users cannot write. Needs a
  service-mediated write path before it can be merged; the design exists but
  is not implemented. Verification: standard-user and administrator sessions on
  installed Windows.
- **Recursive folder deletion in the Maintenance analyzer and duplicate
  cleanup.** Gated and disabled by default; the default build offers no recursive
  folder deletion there. Verification: Windows, elevated and standard, with
  shared handles and junctions.
- **Experimental settings-generation cutover** (versioned encrypted settings
  generations with a protected cutover record). Disconnected from installers
  and normal startup; needs an independent anti-rollback anchor, installer
  provisioning and cross-account tests (constraints and acceptance runs are
  recorded in RESEARCH.md).
- **Encrypted content-search index cutover.** On `main` the ordinary per-user
  index is not encrypted (see Full pending). The tree holds a disabled research
  design and prototype (design conclusions in RESEARCH.md).
- **Fleet service-owned recovery/re-admission authority.** Test-only fixtures
  and gated source; no production enablement.
- **Experimental per-generation search leases.**

## 3. Partial

Partly implemented. What exists, and exactly what is missing.

- **Service-owned endpoint control plane.** Exists: SYSTEM service
  (`src-tauri/commander-svc`) with signed-policy store, personal settings,
  Vault authorization, and optional Fleet reachability
  (`fleet_transport.rs`, which never polls commands or acts on silence).
  Missing: `reconciler.rs` and `command_worker.rs` are idle placeholders
  (30-second sleep loops started from `main.rs`); drift repair and command
  execution still run in the session-bound app (`useAutoHeal`, Pro sidecar), so
  sign-out, quit or crash interrupts them. Verification: installed service
  across sign-out and user switch.
- **CameraTrigger flows.** Exists: Privacy Shield now emits
  `privacy-shield-event` for gaze triggers (`backend.rs`), and the listener is
  in `flow_engine.rs`. Missing: `CameraTrigger` is still rejected in
  `flow_engine.rs` validation and `flow_capabilities.rs` still reports the bus
  as failed; no camera-specific event is emitted.
- **Selective VeraCrypt header destruction.** Overwrite is issued and flushed
  but not independently re-mounted to confirm (`selective_erase.rs`).
  Verification: live container test.
- **Resource Usage History cleanup.** Reports SRUM database metadata only; it
  does not decode historical records ([FEATURES.md](FEATURES.md)).
- **Session Assurance identity.** UI uses personal "self" identity defaults; the
  Fleet-managed organization/subject identity is not wired
  (`src/panels/privacy/SessionAssuranceSection.tsx`).
- **Remote file search and extensible search backends.** Reserved only:
  `SearchMode` (keyword/semantic) in `fleet-proto` has no catalog entry or
  handler, and `SemanticBackend`/`ForensicBackend` traits in
  `wincmd-search/src/backend.rs` have test stubs only.

## 4. Full pending

Not started.

- **Encrypted-at-rest ordinary content index on `main`.** The index under the
  user's `file-search\fts` data folder stores extracted text unencrypted; no
  encryption exists in the index path. Until then, rely on disk encryption
  (see [SECURITY.md](SECURITY.md#current-operational-limits)).
- **Entropy-delta signal for the Free ransomware monitor.** Only write-rate
  detection exists (`ransomware_monitor.rs`); process attribution is a paid
  capability.
- **Audit record for confirmed irreversible actions.** The confirmation dialog
  only logs to the console (`ConfirmIrreversibleDialog.tsx`).
- **UI polish.** Consolidate the two Switch components (`ui/switch.tsx`,
  `shared/WCSwitch.tsx`); sliding tab indicator (`ui/bp.tsx`).
