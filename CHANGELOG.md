# Changelog — WinCommander Free

Public release notes are published with each versioned
[GitHub Release](https://github.com/servalabs/wincommander/releases).

## Unreleased

- Added a Monitor Operations Center that combines the local and Pro monitor
  families into one privacy-safe snapshot. It shows useful counts, check
  cadence, stale/degraded/unavailable states, Pro locks, coverage filters, and
  manual refresh without exposing event contents, paths, usernames, or peers.
  Stateful Pro monitor commands require one durable sidecar session so a
  running monitor and its status/history do not drift between workers; if that
  authoritative session is unavailable, the dashboard reports it instead of
  querying a different worker.
- A slow cold settings read now recovers in the background after the bounded
  startup-cache deadline instead of leaving the dashboard shell at 0% with an
  empty navigation rail.
- Fleet Vault policy now supports standard and outer+hidden file containers,
  including group-shared dual entries. Each dual mount selects outer or hidden
  for that request only; writable outer mounts require a second transient
  hidden-volume password so the service can force hidden-region protection.
  Both passwords are cleared after the authenticated service-to-Pro request,
  and older standard-container requests and drafts remain compatible.
- Normal cached launches can show the dashboard without waiting for the
  decorative splash. Panel loading and Disk Cleanup warming now follow user
  intent or a bounded idle budget instead of competing with first interaction.
- Startup background work now shares one coordinator: equivalent probes reuse
  in-flight results, expensive launch jobs are serialized, Downloads discovery
  leaves the native setup path, unchanged default Flows avoid settings writes,
  and optional Pro/ActivityWatch checks run only when applicable.
- Added a local, bounded startup trace for comparing launch phases without
  recording paths, settings values, command arguments, licence data, device
  identifiers, or arbitrary error text.
- Vault access hardening now keeps availability queries read-only, serializes
  policy apply/mount/unmount work, avoids implicit shared-user grants, permits
  an explicit revoke-all policy, rejects duplicate principals, keeps automatic
  mounts per-user unless machine scope is explicit, and reports unknown volume
  ciphers honestly. Repeated driver and Pro-binary validation is cached by file
  identity for the service process. Recovery-write failures are isolated per
  entry; boot cleanup uses a bounded SYSTEM/internal-slot request; and a shared
  live mount can be unmounted only by its original SID/session. Legacy drafts
  resolve only known current principals, preserve distinct SID identities, mark
  missing accounts unavailable, and support a conflict-safe three-way rebase.
  Vault mutation calls now use per-verb deadlines, unique request IDs, and an
  explicit unknown-outcome result on timeout. Driver readiness/SCM checks are
  cached by driver identity and run off async service workers. Mount/unmount
  patch returned UI state, focus refresh is coalesced, apply reuses returned
  status, and dismount performs one owner refresh. The unsupported vault-panel
  system-encryption PowerShell spawn was removed.
- Large asset maps are split by their consuming feature. The initial registry
  uses a small explicit feature-logo allowlist while Apps, browser/privacy,
  Network, Mesh, cloud, and product media stay with their lazy surface.
- Frequent live metrics share a bounded native disk snapshot while CPU and RAM
  remain live; settings, portable state, device-hash/licence inputs, and
  mutations remain outside this cache.
- Public documentation was reduced to user-facing product, security, and
  contributor information. Internal plans, runbooks, mockups, acceptance
  evidence, and development ledgers are maintained privately.

This file intentionally excludes unshipped implementation detail and internal
readiness status. A shipped change should be added here only when it is suitable
for public users and matches the released artifact.
