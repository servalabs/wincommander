# Changelog — WinCommander Free

Public release notes are published with each versioned
[GitHub Release](https://github.com/servalabs/wincommander/releases).

## Unreleased

- Secure Storage no longer asks users to identify a standard, outer, or hidden
  volume before mounting. The native engine selects the matching header from
  the supplied credentials, while personal and Quick Mount are locked
  read-only so an automatically selected outer volume cannot overwrite hidden
  data. This is source and automated-test evidence; a successful
  password-based mount still requires installed-Windows acceptance.

- Free, the Windows service, and Investigator now authenticate the connected
  Pro child by PID and executable bytes before sending the per-spawn secret;
  the secret no longer appears in the child command line. IPC v2 carries one
  strict destructive target identity through authorization, mutation, and a
  checked receipt. VeraCrypt headers use verified file/partition handles and
  BitLocker uses the resolved volume GUID. Explorer/search secure erase now
  overwrites, reads back, and deletes regular files through one handle while
  refusing reparse points, hard links, and folders. These are source and
  automated-test claims; signed installed-Windows attack acceptance is pending.

- Destructive desktop actions now require a native, target-specific confirmation
  and a short-lived, single-use capability bound to Rust-canonicalized arguments.
  Renderer-accessible generic and debug dispatch paths refuse Lockdown-only
  internals; trusted hotkey, watcher, and distress triggers remain Rust-owned and
  do not expose authorization secrets to WebView events. Lockdown capabilities
  are bound to the complete configuration snapshot that is subsequently
  executed; path mutations bind canonical paths and Windows file identities,
  selective BitLocker/VeraCrypt erasure uses the same native capability path,
  and repeated PIN failures are rate limited in Rust. Persisted flows now fail
  closed for Lockdown and protected erase actions. These controls are covered
  by source-level automated tests; signed installed-Windows acceptance remains
  pending.

- Developer and CI checks now run on trusted `main` pushes and pull requests
  with path selection and redundant-run cancellation. An explicit reversible
  pre-push hook runs the repository validation command. Debug builds load
  backend modules directly without regenerating release ciphertext; release
  builds fail when the salt or encrypted modules are missing, corrupt, or stale,
  and the final executable is scanned for protected plaintext.

- Vault service requests now fail closed for unknown operations, use a strict
  service-produced mount plan and shared bounded error vocabulary, preserve the
  original request identifier across the service and Pro broker, and redact
  credentials and transient security data from diagnostics. Personal Vault
  creation now uses a durable request-bound reservation with stable file
  identity checks; it remains administrator-only pending installed multi-user
  Windows acceptance.

- Paid Pro update installation now requires the signed
  `updates_entitled_until` entitlement in both the interface and the Rust
  installer. When coverage ends, the installed normal-Pro build keeps working
  but automatic, combined, dashboard, Settings, repair, and direct invoke paths
  cannot install a newer paid build. Checkout now sends the app version,
  explains manual-review offers, and can cancel an active subscription renewal
  at the end of its paid cycle through the saved recovery credential.
- The Incoming RDP access control in Privacy → Remote Access Monitor no
  longer surfaces a raw OS error when the WinCommander system service is
  stopped or unregistered. It now shows a plain-language message and, for
  an already-elevated session, a "Repair service" action that re-registers
  and starts the service from the app's own trusted binary — recovering a
  machine left in that state without a full reinstall.
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
