# Changelog — WinCommander (Free)

All notable fixes and changes to this repo (`commander-free`, `wincmd-search`,
`wincmd-shared`, and the React frontend), in
[Keep a Changelog](https://keepachangelog.com/) style. Dates are UTC+5:30 (repo
commit timestamps). Shipped capabilities (not fixes) live in
[FEATURES.md](FEATURES.md); architecture and design invariants live in
[ARCHITECTURE.md](ARCHITECTURE.md); the lessons worth not relearning live in
[AGENTS.md](AGENTS.md) → *Gotchas*.

## [Unreleased]

### Added

- **Encrypted-volume driver repair and explicit decoy mounting**
  (2026-08-22) — Free now distinguishes standard, visible-decoy, and hidden
  mounts in the Secure Storage dialog and sends a bounded volume role to Pro.
  The SYSTEM service validates the fixed Microsoft-signed driver by path,
  ownership, ACL, Authenticode, and pinned SHA-256 before recreating and
  starting only WinCommander's driver service. The installer preserves the
  same system-start lifecycle, preventing an update from leaving ordinary
  users without a loaded encryption driver.

- **Vault access takes effect immediately and service shutdown is mount-safe**
  (2026-08-22) — explicit owner/user SIDs are now retained separately from the
  managed-group ACL, so an already-logged-in user does not need to sign out and
  back in after policy apply. Older persisted policies are migrated only after
  their identity, group membership, and exact ACL revalidate. The service now
  reports `StopPending` while its bounded dismount cleanup runs. The live
  Administrator/Partner1 matrix passed immediate projection, shared and decoy
  I/O boundaries, password non-persistence, active-mount service stop, policy
  restart revalidation, and complete cleanup. The source CLI catalog audit also
  stops treating intentionally desktop-only mutations as missing headless
  adapters.

- **Vault service hardening and developer-host multi-user validation**
  (2026-08-21) — the public
  service now has rollback-aware deterministic NetAPI group reconciliation,
  exact startup ACL/membership revalidation, durable mount-recovery records,
  and a cross-session collision guard. The installer source now preserves a
  correctly quoted Program Files service image path and restores the previous
  executable/configuration if replacement fails. Windows token membership now
  uses an explicit impersonation token, every local-only pipe instance receives
  the same ordinary-user DACL, and mounted-root grants inherit to new files and
  folders. With released Pro 3.2.19, the reusable SSH harness passed the exact
  three-entry developer-host matrix: Administrator saw three Vaults, Partner1
  saw and wrote only the shared Vault, both private decoys denied Partner1,
  Administrator wrote both decoys, and all mounts dismounted and cleaned up.
  This is not a packaged NSIS, separate interactive-desktop, unrelated-user,
  revocation/restart, or clean-machine acceptance claim.

- **Reactive unknown-keyboard approval** (2026-08-21) — Pro can hold a newly
  observed, unallowlisted keyboard disabled and surface a global approval dialog
  only after Windows containment read-back succeeds. Allow once is protected by
  one backend-bound randomized six-click human-presence challenge; permanent
  trust requires a stable serial identity and two independent challenges.
  Detach/replug observed by the poll, expiry, wrong responses, and ambiguous
  composite identities cancel or fail closed. Because the current three-second
  poll can miss a very fast detach/replug that reuses every Windows identifier,
  attachment-generation binding remains best-effort until native PnP
  notifications replace polling. The UI explicitly states that pointer input does not
  authenticate a separate trusted mouse and that user-level polling cannot
  guarantee first-keystroke or preboot prevention. Setup, recovery, managed
  device policy, Windows Server/RDS limits, and physical acceptance tests are
  documented in [the operator guide](docs/usb-keyboard-approval.md).

- **Commercial boundary for organisation-grade attack defenses** (2026-08-21)
  — the Free executable now retains the everyday safety baseline: a simple
  machine-wide USB attach/detach timeline and the local mass-file-change
  ransomware alarm. Filesystem decoy watching/read attribution and advanced
  USB transfer intelligence, trust scoring, HID timing anomalies, allow-list
  policy, and reactive auto-isolation execute only in the independently
  entitled Pro sidecar. Free contains typed paid IPC wrappers and the UI, not
  those detection/decision engines. Stop-only cleanup remains available after
  entitlement expiry so a monitor cannot be trapped on.

- **Machine-wide security-state completion** (2026-08-21) — USB timeline,
  Pro-disabled marker,
  F6 boot-verification recovery marker and security-event timeline now live in `%ProgramData%` with the
  main policy store, so Windows Server/RDS users see the same device-security
  posture. Pro's USB intelligence/auto-isolation policy uses a separate
  ACL-hardened ProgramData store. A validated current-user legacy file migrates
  only after its ProgramData replacement succeeds; session/private scratch
  remains per-user.

- **Operator-configurable security monitors** (2026-08-21) — Access & Session
  monitoring now persists working days/hours, local or UTC schedule basis,
  per-account failed-sign-in threshold/window, RDP/new-account/off-hours
  signals, repeat-alert delay, and Fleet reporting; off-hours context applies
  to RDP and the Fleet-wide reporting requirement cannot be locally bypassed.
  Wi-Fi Guard now persists and re-arms its bounded learning window, baseline,
  poll/debounce policy, and path-free Fleet alert preference. Ransomware policy
  adds repeat-alert/action cooldown, per-process evidence floor, ordered/retried
  startup, Fleet reporting, and honest detection-versus-containment health.
  Decoy read auditing now follows its actual saved switch. Screen-capture,
  USB/HID/auto-isolate, Wi-Fi, auth, and remote-access monitors re-arm or retry
  from saved intent; driver-watch cadence is selectable and applied immediately.

- **USB HID and device-control hardening** (2026-08-20) — composite HID
  functions reach Pro's timing-anomaly monitor; sub-8 ms intervals are retained
  and pre-attach timing is cleared. The Windows low-level hook cannot identify
  the source keyboard, so the alert is explicitly low-confidence and cannot
  directly enforce. USB serial/network functions remain outside HID/storage
  response. Sensitivity and auto-isolate mode/scope/exceptions are machine-wide;
  Enforce stays storage-only until HID scope is separately confirmed. Pro
  device control rejects non-USB/HID PnP targets and propagates Windows command
  failures instead of reporting false success. Physical BadUSB, Flipper Zero,
  Hak5 Rubber Ducky, O.MG cable, and composite-device verification remain open.

- **Signed-updater version binding** (2026-08-20) — staged installer bytes are
  reused only when a fresh signed manifest still names the same version. If the
  channel advances while WinCommander is open, installation downloads and
  verifies the new artifact instead of feeding stale bytes to the new update.

- **Routine cleaner safety and coverage refresh** (2026-08-20) — imported the
  compatible Kudu v2.1 cleaner improvements into WinCommander's native Rust
  cleaner: per-rule retention ages, bounded named-cache discovery below an
  explicit anchor, fail-closed timestamp checks, and cleanup-time file-identity,
  age, and handle-resolved containment revalidation. Unsupported rule fields are
  rejected instead of silently weakening a future catalog import. The catalog
  now covers newer Discord, Slack, VS Code, npm, pnpm, Docker, Cursor, Notion,
  and Logitech G HUB cache locations without traversing session or offline-data
  branches. Preview results are listed largest first within each category.
- **Signed recovery-wipe admission foundation** (2026-08-20) — the future
  no-USB recovery environment now verifies a canonical Ed25519-signed plan,
  binds it to the device wipe-token nonce and exact disk/partition identities,
  hashes the real staged WIM/tool files, and atomically consumes the nonce.
  This does not install a partition or erase a disk; WIM packaging, one-shot
  boot integration and destructive acceptance remain release gates.

- **No-path encrypted backup registration** (2026-08-20) — Secure Storage now
  exposes paid status, register, and clear controls for the pre-Lockdown backup
  binding. The WebView sends an empty object only; Pro derives the single
  mounted encrypted file-container identity, returns no path/hash, rejects raw
  or ambiguous targets, and clear removes no files. Focused UI, TypeScript,
  lint, tier, and Pro backup tests pass; signed/live acceptance remains open.

- **Windows Server tweak section** (2026-08-15) — twelve Server-SKU settings in
  a new `tweaks/server` module: no Ctrl+Alt+Del at logon, hide last signed-in
  user, console inactivity lock, no Shutdown Event Tracker, no Server Manager
  at logon, disable IE ESC, block WDigest cleartext credentials, LSA
  Protection (RunAsPPL), refuse LM/NTLMv1, require SMB signing, disable SMBv1,
  and disable Remote Registry. The section only renders when a Server SKU is
  detected (`Win32_OperatingSystem.ProductType` 2 or 3, with `osName` as the
  pre-probe fallback), and the three settings Windows ignores on client SKUs
  guard with `Assert-IsServerSku` rather than writing a dead key. Verified
  against a live Windows Server 2025 Standard (build 26100) RDS/Hyper-V host.

- **Registry-only exemption to tier invariant 5** (2026-08-15) — `No
  Ctrl+Alt+Del At Logon` and `Disable IE Enhanced Security` genuinely set
  `reducesSecurity`, but are plain HKLM DWORD writes with nothing for AV to
  flag, so they stay `tier: "free"` via an explicit allowlist in
  `tools/check-tier-invariants.ts`. The stale note in `src/types/toggles.ts`
  claiming `reducesSecurity` was unbound from tier has been corrected — the
  rule is real and enforced; this is a narrow, documented carve-out.

- **Fleet device summaries now carry `device_kind`** (2026-08-13), allowing
  the server and console to distinguish Windows, Linux, and Android agents
  without inferring from host metadata. The additive field defaults safely
  when an older cached response is decoded.

- **Shared fleet protocol contracts for governed security collection**
  (2026-08-12). The catalog now includes the bounded, parameter-free
  `endpoint.security_snapshot` action and the fixed
  `velociraptor.collect.client_info` provider action. A strict typed snapshot
  result contract caps every row domain and the aggregate payload, and the
  generated TypeScript bindings expose the same wire shapes to the Pro fleet
  console.

### Fixed

- **Encrypted-volume mounts no longer disappear at the two-minute boundary**
  (2026-08-22) — Free now gives the exact `Mount-EncryptionVolume` request up
  to 20 minutes while all other Pro requests retain their two-minute bound.
  This covers legitimate long-running PIM and hidden-volume key derivation.
  The mount dialog clears both passwords as soon as dispatch begins, preserves
  the non-secret path/PIM/options, shows an in-place progress explanation, and
  retains a bounded in-place error instead of closing with no useful result.
  Request-shape tests prove outer and hidden PIM values remain distinct across
  the Free-to-Pro boundary.

- **Fleet administrator tabs recover from transient service startup failures**
  (2026-08-21) — the Fleet panel now retries its service-derived permission
  check, refreshes it when the window regains focus, and exposes a manual retry
  instead of permanently presenting an Administrator as a Vault-only member.
  Administrative tabs remain hidden when the service positively identifies the
  caller as a non-admin account.

- **Backend CLI deadlines now cover runtime startup** (2026-08-20) — a live
  `3.2.17` read-only probe remained alive beyond its explicit 10-second limit
  because the timer was created only after Tauri setup began. The deadline now
  runs on an independent process thread before runtime initialization, so a
  wedged startup also emits the bounded timeout result and exits `10`. This is
  source-level remediation; a source-matched release still needs VM acceptance.

- **Investigator release-shell fail-closed validation** (2026-08-19) — signed
  two-artifact manifests now reject unknown fields, reordered app/sidecar
  entries, and noncanonical uppercase SHA-256 values. Entitlement matching has
  a regression test proving ordinary Pro's `paid` feature cannot satisfy the
  explicit Investigator `advanced` gate. Clean-machine release acceptance
  remains checkpoint-gated in the private release record.

- **Fresh Windows developer bootstrap now installs rustup reliably**
  (2026-08-18). The installer keeps rustup's required `rustup-init.exe`
  executable name and sends its informational output to the host instead of
  accidentally capturing it as the rustup command path. A clean sandbox now
  installs and selects the repository-pinned Rust 1.97.1 toolchain headlessly.

- **Headless automation now fails closed for every mutating and destructive
  command** (2026-08-15). The CLI exposes only read-only handlers until its
  native confirmation and cross-process locking controls cover the mutation
  surface, preventing CLI-triggered destructive actions, settings write races,
  and index-rebuild/read races.

- **Fleet device requests now bind the complete payload to HMAC v2**
  (2026-08-13). The shared agent core signs the HTTP method, exact route path,
  and recursively canonicalized JSON body (excluding only `hmac`), preventing
  authenticated envelopes from being reused with mutated decoy, telemetry,
  search-result, or command-ack fields. Enrollment advertises
  `hmac_body_v2`; cross-language golden vectors pin string escaping and exact
  plain-decimal number normalization.

- **Restored the commander-free workspace manifest** (2026-08-12) by removing
  a stray delimiter that prevented Cargo from parsing the workspace during
  protocol type generation and validation.

### Changed

- **Desktop search now has bounded render and native-icon work** (2026-08-20).
  Filename results render through a 48-row window; content results remain
  backend-limited to 50, keeping the covered 2,000-result fixture below the
  100-row DOM budget. Native file-icon work is shared, prioritizes selected and
  nearby rows, allows at most eight active requests, and discards obsolete
  queued work. Per-row entrance animation was removed while keyboard selection,
  screen-reader semantics, and reduced-motion behavior remain covered. No live
  latency or device measurement is claimed.

- **Startup, live dashboard data, and occasional shell controls are now more
  isolated** (2026-08-20). Persisted settings hydrate before background probes;
  the shell exposes cached, refreshing, ready, and stale states rather than
  blocking on those probes. Two-second CPU/RAM/disk samples and motion
  preference have narrow providers, so their updates do not redraw the global
  application state. The RDP quick action and metadata-scrubber dialog load on
  demand. This is structural work, not a measured startup-latency result.

- **The first typed seams now sit beside the legacy backend facade**
  (2026-08-20). Search-maintenance commands moved into a dependency-free typed
  client, and cleanup-category adapters centralize typed preview handling.
  `useBackend` remains the compatibility facade and its wider extraction, along
  with existing oversized-file debt, remains open.

- **Changed-file quality controls now run in CI** (2026-08-20). `lint:quality`
  checks added TypeScript/Rust/PowerShell/CSS lines for new explicit `any` and
  flags excessive new source-file growth while allowing pre-existing debt to be
  addressed incrementally. It complements existing lint/type checks; it does
  not certify a full repository cleanup.

- Corrected stale Fleet/monitoring documentation that described retired
  consent handlers and a nonexistent 403 disclosure-version gate, and marked
  the now-complete cross-device content-search path as shipped.

- **Lockdown can now target VeraCrypt whole-partition volumes safely**
  (2026-08-11). Secret Settings lists eligible non-system partitions alongside
  file containers and persists a bound disk/partition identity. Every lockdown
  trigger and the reboot-to-USB stage forwards that complete identity to Pro,
  where it is revalidated before header destruction. Empty target lists still
  skip cleanly. The full dispatch was physically verified on both designated
  5 GB partitions, including successful pre-destroy mounts and rejected
  post-destroy credentials.

- **Vault creation now exposes the complete guarded VeraCrypt volume workflow**
  (2026-08-11). Users can create standard or decoy+hidden file containers and
  whole non-system partitions, select every bundled cipher/cascade and hash/KDF,
  use independent keyfiles/PIMs, quick or dynamic file creation, custom
  filesystems, and mount read-only/removable or with hidden-volume protection.
  The partition picker surfaces exact disk identity and requires a partition-
  bound erase phrase; Pro re-probes it before any write. Dismount now tries the
  non-forcing path first and presents a separate force confirmation only when
  needed. The bundled engine was physically exercised on the two designated
  5 GB test partitions; signed-release clean-machine acceptance remains open.

- **Productivity panel no longer embeds ActivityWatch's web UI** (2026-08-04).
  The panel used to host ActivityWatch's own Vue app in a WebView2 pointed at
  `http://localhost:5600` and inject an `AW_HIDE_CSS` string to hide its navbar,
  header and footer. That did not theme with the app, broke whenever upstream
  changed its markup, and dragged in webview lifecycle workarounds
  (`hide_all_server_apps` on unmount, a remount `key` keyed on hostname). It now
  reads ActivityWatch's REST API directly and renders native components from
  `src/components/activity/` — an independent AGPL-3.0 viewer layer, deliberately
  not shared with the Pro fleet console (see [AGENTS.md](AGENTS.md) → *Gotchas*).
  Adds a date picker: ActivityWatch keeps months of local history, but the embed
  only ever showed today. Reads the web and VS Code watcher buckets too, which
  the panel had advertised installing for a long time without ever reading.

### Removed

- **Unused frontend dependencies** (2026-08-20). `@xyflow/react` and
  `thinking-orbs` had no source imports and were removed from `package.json` and
  the Bun lockfile.

- **ReFS encrypted-volume creation option** (2026-08-12). The VeraCrypt
  creation wizard now offers NTFS, FAT32, and exFAT only; ReFS availability is
  too dependent on Windows edition and target-device support for a reliable
  WinCommander workflow.

- **Employee-facing monitor kill-switches** (2026-08-04). The "What my employer
  sees" section's three switches (Session Assurance, Access & Session, App Usage)
  are gone; the cards are now read-only status. Monitoring on a fleet-enrolled
  device is unconditional — the lawful basis is the employment agreement, not a
  per-device opt-in. The switches were also misleading: the fleet agent's
  productivity-detail collector reads ActivityWatch directly and never consulted
  them, so switching "App Usage Monitor" off never stopped the upload.

### Fixed

- **Desktop UUID generation now supports older WebView2 engines** (2026-08-06).
  Shared app initialization no longer calls `crypto.randomUUID()` directly;
  it uses the secure `getRandomValues()` UUIDv4 fallback when the convenience
  method is unavailable. This prevents the device-side Fleet/search/download
  surfaces from failing before their request can be sent.

- **Removed the employee-facing “What my employer sees” card** (2026-08-06).
  Fleet monitor lifecycle is now owned entirely by the Fleet agent instead of
  a one-shot frontend timer. On Fleet enrollment it starts Session Assurance,
  Access & Session, and all Argus collectors with their default settings; on
  disconnect it stops them together. UI remounts can no longer flip Session
  Assurance between on and off.

- **Fleet now repairs legacy device IDs before enrollment** (2026-08-06).
  A blank or non-UUID settings identity is replaced and persisted as one stable
  UUID during settings load. This prevents Fleet's Postgres-backed search and
  file-transfer paths from rejecting an enrolled device, and the Fleet panel
  now shows the pending device ID while enrollment is in progress.

- **The Monitoring Mirror understated what leaves the machine** (2026-08-04).
  Its header declared a privacy invariant that "window titles, exe paths, URLs,
  filenames... and usernames NEVER leave the device", and the panel subtitle read
  "Data stays on this device. Never uploaded." Both were false on a fleet-enrolled
  device: application names, window titles, URLs, file paths and the username are
  reported to the fleet. The copy now states what is actually transmitted, and
  what genuinely still is not (keystroke content — input is counts only —
  screenshots, webcam frames, clipboard contents, and file contents as distinct
  from file paths). `AGENTS.md` carried the same false blanket claim; it is now
  scoped to the `ArgusSignal` wire, where it does hold, and it no longer documents
  a `consent_store.rs` module that never existed in any commit.

- **Commands that erase data asked for the weaker confirmation** (2026-08-03).
  The headless CLI graded every command from its *name*, and the destructive name
  lists were anchored to the start of the name — so the whole `Invoke-*Erase`
  family (`Invoke-7Erase`, `Invoke-UnallocatedSpaceErase`, `Invoke-CrashDumpErase`,
  `Invoke-PreviousWindowsInstallErase`) accepted `RUN:` instead of `DESTROY:`.
  Worse, the guess disagreed with `authz::DESTRUCTIVE_COMMANDS` — the repo's own
  CI-enforced catastrophic-command registry — so `fleet_connect` (re-pinning to a
  different fleet server and signing key) and `internet_kill_switch_set` were
  graded ordinary mutations. `classify_risk` now consults that registry **first**,
  then a table of handlers hand-verified against their bodies, then the name
  rules, then any name containing `erase`/`shred`/`wipe`/`destroy` — the last
  applied only after the read-only allowlist, so `Get-AutoEraseSchedules` stays a
  read. **17 commands were regraded, every one stricter.** Adding a command to the
  registry now hardens the CLI automatically; a test enforces it.

- **Three commands that write were graded read-only — no confirmation at all**
  (2026-08-03). `search_rename_file` renames a file on disk but matched the
  `search_` read prefix; the four `export_*` commands write an artefact yet were
  eligible for the read-only wait deadline, so an export could be killed
  part-written; `fleet_update_posture_snapshot` pushes posture to the fleet server
  but matched the `_snapshot` read suffix.

- **`--dry-run` could silently execute for real** (2026-08-03). The parser took
  whatever token followed an option as its value, so `--confirm --dry-run`
  consumed `--dry-run` as the confirmation string and left dry-run off. Read-only
  commands never check the confirmation, so the preview ran live. The same
  swallow smuggled `--safe-copy` past the parser into `lib.rs::run`'s argv scan,
  hijacking the requested command and exiting `0` without running it. Flag-shaped
  option values are now rejected, and that argv scan is skipped in CLI mode.

- **A wedged backend command hung forever** (2026-08-03). `--timeout-ms` was
  accepted on read-only backend-script commands and then ignored — the documented
  behaviour, but a trap for unattended automation. Both transports now honour the
  deadline (default 300,000 ms) and exit `10`. It remains a wait limit, not
  transactional cancellation.

- **An unanswerable confirmation dialog hung the process and blocked every other
  job** (2026-08-03). `authz::native_confirm` awaits a dialog with no timeout. In
  CLI mode the only window is invisible and unattended runs have nobody to click
  it, so the process waited forever *while holding the cross-process execution
  lock* — failing every other mutating CLI run with `cli_busy`. It now fails
  closed when the CLI runtime is active: an unanswerable confirmation is a denied
  confirmation.

- **Two paths produced no JSON at all** (2026-08-03), breaking the
  one-document-per-invocation contract. A CLI-runtime setup failure reached
  Tauri's own `panic!`, exiting `101` with empty stdout; it now reports
  `runtime_error` and exit `9`. And the `lockdown`/`full_lockdown` detached
  acknowledgement sat *after* `crate::run()`, which never returns because Tauri's
  `App::run` exits the process directly — so terminal lockdown printed nothing.
  It is now emitted before dispatch, with a guard so no second document can
  follow it. It still acknowledges only the dispatch, never the outcome.

- **The debug-only command list could drift silently** (2026-08-03). Four
  dev-panel handlers are `#[cfg(debug_assertions)]`-gated and must be refused by
  release builds, but the CLI matched them against a hardcoded name list while the
  catalog generator could not see `cfg` attributes at all. A fifth gated handler
  would have been reported as executable in release, and every test would still
  have passed. The generator now derives `debugOnly` from the attribute itself and
  fails if the parse stops working; the catalog carries the flag (schema v2).

- **A `__proto__` parameter key silently vanished** (2026-08-03). Parameters were
  spliced into the runner script as JavaScript object syntax, so that key set the
  prototype instead of becoming an ordinary parameter and the handler received
  something other than what was asked for. They are now `JSON.parse`d from a
  string literal.

- **Multi-token searches returned nothing at all** (2026-07-28).
  `search_everything` passed the entire query to `es.exe` as a single argv entry,
  which `es.exe` reads as a quoted phrase — so any query combining a term with a
  filter (`ext:md dm:thisyear`) returned **zero rows with no error**. The app's own
  app-priority query (`brave* ext:exe;lnk;msi;appx;msix`) had therefore never
  matched anything, so typing an app name ranked apps only by luck.
  `backend.rs::tokenize_es_query` now splits into one argv entry per token
  (preserving quoted runs), and the frontend sends a pre-split `tokens` array.

- **Files whose names contain `" - "` could not be found by typing their name**
  (2026-07-28). The argv-injection guard correctly rejects any token starting with
  `-`, but a plain whitespace split turned the common `"Name - Description"`
  pattern — including Windows' own WinX shortcuts — into a lone `-` token, erroring
  the entire search. Measured: **198 of 4,014 real files (4.9%)**, now 0.
  `searchQueryPlan.ts::toEverythingToken` also neutralises a leading `!`, which
  Everything was parsing as its NOT operator and silently answering with ~1.4M
  wrong rows.

- **Wrong-separator searches found nothing** (2026-07-28). Typing
  `Docker-Desktop` or `Docker_Desktop` for `Docker Desktop.exe` had **0% recall**
  across 268 real cases. Separator runs are now interchangeable: 0% → **97.3%**
  found within the fetch window.

- **The user's own files were ranked behind system folders** (2026-07-28).
  Everything's native order is by path, so `C:\Windows` and `C:\Program Files` came
  first: the developer's own `D:\GitHub\assets` ranked **1193rd of 1239** folders
  named `assets*` and fell outside the fetched window entirely. Searches now sort
  by recency, fetch a wider window, and prefer the user's own space over program
  and build/cache trees. An exact- or prefix-name match now promotes **any** result
  type, not only apps — folders were previously near-invisible (recall@10 8.6%).

- **Search Files reported "no results" while thousands existed** (2026-07-28).
  When a text query and filter chips were combined, the panel sent the bare query,
  over-fetched, and filtered in JS — so "folders named assets" kept the folders
  among 50 arbitrary rows. Filters now reach the backend, and the client-side
  re-filter (whose date window disagreed with Everything's) is gone.

- **Icon extraction stalled every keystroke by ~10 seconds** (2026-07-28).
  `parse_es_output` spawned one PowerShell per app-like result row (**measured
  ~866 ms each**), serially, on a tokio worker thread and *outside* the search
  timeout. Removed: the frontend already lazy-loads icons per visible row and
  caches them.

- **An un-indexed filter could hang the search box indefinitely** (2026-07-28).
  `es.exe` calls now run under a hard timeout (6 s search / 4 s count) with a plain
  explanation instead of a freeze. `attrib:` forces a live disk scan and ran >100 s
  in testing.

- **Content search ignored the folder scope** (2026-07-28).
  `ContentQuery.roots` was accepted and never applied, and `search_content` had no
  scope parameter — so an "in this folder" filter narrowed the file-name list while
  "Inside files" below it kept showing hits from the whole disk. Now scoped end to
  end, component-anchored so `…\wincommander` cannot leak `…\wincommander-pro`.

- **Frecency could permanently forget your most-used files** (2026-07-28). A
  heavily-used file that went idle past 90 days scored below a brand-new single
  open, so a burst of one-off opens evicted its history at the store cap. Eviction
  no longer discards durable open history, and more path spellings of one file now
  share an entry.

### Changed

- **`Ctrl+Space` gains a chip grammar** (2026-07-28). Typing a word like `folder`
  or `images` offers a filter chip promoted with `Tab`; `Backspace` at the start of
  the text demotes the last chip back to the exact word typed. Chips cover type,
  folders/files, time, size, and folder scope. Time chips *prefer* recent by
  default and become a hard date filter on a second click, because a hard filter
  turns a slightly-wrong memory into an empty list. An empty box now lists recent
  files instead of nothing, `>name` jumps into a folder, and the folder you were
  last viewing in Explorer is offered as a scope.

- **Two chips relabelled to match what the engine does** (2026-07-28). "Empty" →
  **"Empty folders"**: Everything's `empty:` matches empty folders only and can
  never surface a 0-byte file (10,373 exist on the test machine); pairing it with
  "Files" returned zero forever and is now mutually excluded. "Duplicates" →
  **"Same name & size"**: `dupe:` matches filename and size, not content.

- **File-type chips cover many more real formats** (2026-07-28). Notably `.c`,
  `.h`, `.hpp` for Code (~35,000 files previously invisible), `.avif`/`.tif` for
  Images, `.apk` for Apps. A missing extension reads to the user as "your file does
  not exist".

- **Very short queries no longer hit the engine** (2026-07-28). 1–2 character
  terms timed out 9 of 16 runs under load versus 0 of 8 for a long term, and are
  virtually always mid-keystroke.
