# Privacy hygiene and Nyx-family coverage

WinCommander's Windows privacy-hygiene surface is a clean-room, native capability
port inspired by the target families documented by
[`evilsocket/nyx`](https://github.com/evilsocket/nyx). WinCommander does not ship,
invoke, download, or copy Nyx's PowerShell script. Coverage is expressed through
WinCommander's existing typed cleanup-category registry and authorized Rust /
PowerShell backends so entitlement checks, path ownership, reconciliation, and
product safety rules remain in one place.

This document defines what “Nyx-family parity” means for WinCommander. It means
reviewable coverage of the useful Windows trace families with stronger preview,
selection, and result semantics. It does **not** mean reproducing a one-click
anti-forensics script or making every destructive upstream action part of a
routine cleanup.

## Guided Routine Hygiene

The Cleanup panel contains a guided Routine Hygiene card for low-risk cache
maintenance. Its allowlist is intentionally exact:

- `dnsCache`
- `thumbnailDb`
- `spotlightCache`
- `fontCache`
- `legacyIconCache`
- `photosCache`
- `xboxCache`
- `branchCache`
- `p2pUpdateCache`
- `geolocationCache`

A new cleanup category is never admitted automatically. Changing a display name,
category order, registry group, or backend availability cannot expand the
allowlist.

The guided flow is:

1. Preview every allowlisted category through its real backend getter.
2. Refuse the bulk action when a category is missing, still loading, unscanned,
   busy, or failed.
3. Show the discovered item total and require one aggregate confirmation.
4. Clear ready categories sequentially through their existing backend clearers.
5. Re-scan each category authoritatively and publish the actual remaining count.

The frontend never assumes that a successful command means the category is now
empty. Partial reductions, no-op results, backend errors, and post-clear scan
failures remain visible. Investigator mode disables the guided mutation so an
acquisition workflow is not silently altered.

## Windows target-family mapping

The exact category registry and backend implementations are authoritative. This
mapping explains how Nyx's Windows families translate into WinCommander's product
model; it is not an alternate dispatcher or an independent list of paths.

| Nyx Windows family | Native WinCommander coverage | Policy |
| --- | --- | --- |
| Events | Existing event-log, servicing, setup, error-reporting, notification, crash-report and diagnostic-log cards | Explicit preview/clear surfaces; never part of Routine Hygiene |
| History | PowerShell history, Recent Files, jump lists, Office MRU, RDP history, browser footprints, search/activity stores and other application interaction caches | Explicit categories because clearing may remove useful user context or investigation material |
| Registry | USB/device association, execution cache, shell bags, network-location, capability-access, profile/usage and registry-hygiene categories | Backend-owned keys and identifiers; no caller-supplied registry path |
| Filesystem | Prefetch, application-launch/compatibility traces, dumps, crash reports, development artifacts, WSL, VM and container data | Separated by consequence; owner data and runtime state are never hidden inside a cache action |
| Temporary data | DNS, thumbnail/icon/font, Photos/Xbox, Delivery Optimization/branch, geolocation, internet/web and application caches | Only the ten named low-risk categories are admitted to the guided routine allowlist |
| Security-related state | Credential, logon-security, remediation, quarantine, firewall and protection-related categories already modeled by the product | Never admitted to Routine Hygiene; entitlement, confirmation, product mode and backend policy continue to apply |
| Advanced/destructive | Recovery artifacts, memory dumps, old installations, large application data, VM/WSL/Docker state and secure-erasure workflows | Deliberately separate, consequence-labelled operations; no generic “wipe all traces” entry point |

Unsupported, unavailable, or renamed categories do not silently degrade to a
filesystem glob or a generic command. They remain unavailable and make the guided
plan fail closed.

## Deliberate safety boundary

This port does not introduce a bulk operation for deleting Windows Security,
audit, Microsoft Defender, EDR, authentication, or incident-response evidence. It
also does not add commands that disable auditing, tamper with security controls,
flush firewall policy, remove restore/recovery material, delete credentials, or
destroy VM/WSL/container data under the label of routine privacy hygiene.

Some of those data families already have narrowly scoped, explicit product cards
or separate recovery/secure-erasure workflows. Their existing backend checks and
confirmations remain authoritative. They are not inherited into Routine Hygiene,
Fix Everything, or any future cache group merely because an upstream script calls
them “cleanup.”

## Execution invariants

- The UI submits registered category IDs, not arbitrary paths, registry keys,
  commands, or script text.
- Every category continues through its existing entitlement and backend
  authorization path.
- Preview is a real backend scan, not a static estimate.
- Clear results are reconciled by a post-action scan; optimistic zeroing is
  forbidden.
- Bulk routine execution is sequential to keep attribution and partial-failure
  reporting deterministic.
- The explicit allowlist, protected-category guardrails, unknown-category failure,
  incomplete-preview failure, and backend-error failure are unit tested.
- This feature adds no Tauri capability, general-purpose command runner, new
  deletion backend, or plaintext PowerShell payload.

## Relationship to deeper cleanup

WinCommander can expose deeper cleanup and secure-erasure capabilities outside the
routine card when the build, entitlement, operating mode, Administrator rights,
and explicit confirmation permit them. Those operations must keep their own
consequence labels, previews, receipts, and recovery warnings. Privacy hygiene is
not evidence destruction, and ordinary file deletion is not guaranteed media
sanitization on SSD/NVMe, snapshots, backups, replicas, or copy-on-write storage.
