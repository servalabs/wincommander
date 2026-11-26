# Changelog — WinCommander Free

Public release notes are published with each versioned
[GitHub Release](https://github.com/servalabs/wincommander/releases).

Git tags run up to `v3.6.7` (2026-11-03). Notes for tagged versions are in the
GitHub Releases, not here. Everything under `Unreleased` is on `main` after
`v3.6.7` (1145 commits between `v3.6.7` and `dc5722ba`, counted 2026-11-17) and
lists only a selection of those changes.

## Unreleased

- Share Safely (Scrub Meta): each file in the results is now one full-width
  section with a single toggle, instead of a two-column grid of cards with a
  separate dropdown per kind of data. Collapsed, a file's header still shows
  its name, type icon, how many items will be removed and a per-kind summary
  ("Camera & device 2 · Dates & times 1 · Author & edits 1"). Expanded, it
  shows every removed property under every kind at once, with the value found
  in the original beside a property when the scan reported one. Up to three
  files start open; larger batches start collapsed (except a file that still
  holds data), with "Expand all" / "Collapse all" in the File details header,
  and list 50 files at a time. In the "files not cleaned" list a file's reason
  now sits on its own full-width line under the name, so a long reason no
  longer wraps into a narrow column.
- Fix RAM disks being writable only by the Windows account that created them:
  other users on the same PC saw the drive (for example "TEMP (R:)") but got
  "You don't have permission to save in this location". An NTFS RAM disk made
  from Secure Storage or by the Startup RAM disk switch now gives every
  signed-in account (Authenticated Users) Modify on the drive and everything
  saved in it, with full control for SYSTEM and Administrators. Nothing is
  granted to Everyone or anonymous logons. A RAM disk an older build already
  mounted gets the same permissions the next time WinCommander reuses it.
- Fix a drive letter (for example T:) staying "in use" after a Vault was
  closed. When a Vault is dismounted from another session, or the volume
  closes, Windows can keep the letter linked to a volume that no longer
  exists in the logon that mounted it. That logon may still be signed in or
  disconnected. Mounting to that letter then failed with "already in use",
  even though no drive showed in File Explorer. The service now frees any
  letter whose link points to a Vault or RAM disk device that no longer
  exists, in any account's logon, with no sign-out needed. It does this when
  the service starts, before choosing or mounting a letter, after a mount,
  and after every dismount, including failed, forced and logoff/disconnect
  dismounts. Dismount also removes the letter in the logon that mounted it.
  "Check and free unavailable Vault letters" does the same in one click and
  reports, for example, "Freed T: (left over from SERVAULT\King's closed
  vault)". A letter that stays blocked now says what holds it: a mounted
  encrypted volume, the WinCommander RAM disk, a Windows volume, a network
  drive or a subst folder. It also says which account holds it and whether
  that account is signed in, disconnected or signed out. Only administrators
  see other accounts' names. Live volumes, the RAM disk, Windows volumes,
  network drives and subst folders are never removed. Secure Storage also
  lists another account's Vault that is still held after that account signed
  out or the volume closed, with no path and no dismount button.
- Share Safely (Scrub Meta): the "files not cleaned" panel is open by default
  instead of hidden behind a small chevron (where it could also show an empty
  box). It names every file with its type icon, folder and reason
  (unsupported format, type mismatch, in use by another app, no permission
  and so on), grouped by reason, 50 at a time; files with nothing to remove
  are listed but not counted as "not cleaned". With more
  than one folder selected the results are shown per folder, each a
  collapsible section with its own status ("Ready — 36 files, 30 to clean,
  6 nothing to remove"). Each file card shows a file-type icon instead of a
  grey text badge, keeps its explanations behind an (i) button, and groups
  the removed properties by kind (GPS / location, Camera & device, Dates &
  times and so on). The engine line under the
  drop zone reads "ExifTool 13.59 · qpdf 12.4.2 · JPEG, PNG, …" instead of
  the garbled qpdf banner.
- Share Safely now reports every file in a scrub, one entry per file, each
  with its selected folder, extension, real type (read from the file's
  content) and outcome. A file that was not cleaned now says why: an
  unsupported format, a type mismatch (for example a WhatsApp ".jpg" that is
  really PNG), in use by another app, no permission, unreadable, unwritable,
  or a path that is too long. Plain text, CSV, JSON, Markdown, BMP and ICO
  files and empty files count as "nothing to remove", not as failures. A file
  or unreadable folder that the scrub engine skipped without naming it is
  still listed. Results are also totalled per selected folder.
- Fix the Hidden Data Remover version text ("vExifTool 13.59; qpdf version
  12.4.2Run qpdf --copyright to see..."). The version is now the ExifTool
  number, and the ExifTool and qpdf versions are also reported separately.
- Fix Stealth mode leaving no Calculator shortcut: with Hide WinCommander
  already on, turning the Calculator lock on restored the WinCommander
  shortcuts to the Hide snapshot instead of replacing them, so the Start menu
  and Desktop showed neither WinCommander nor Calculator. The lock now keeps
  its "calculator." shortcuts while Hide keeps the WinCommander ones hidden,
  and each comes off on its own: turning the lock off leaves WinCommander
  hidden, turning Hide off keeps the Calculator shortcuts, and turning both
  off brings the original shortcuts back byte for byte.
- Fix a crash ("thread 'main' has overflowed its stack") in development
  builds the moment Stealth mode, or the cover identity switch on its own, is
  turned on. Saving the cover name now runs off the window thread.
- Add a Stealth mode switch to Secret Settings that turns on the calculator
  lock, cover identity and hide backend apps together. Hide WinCommander is
  its own switch and is not turned on or off by Stealth mode; each of the four
  works alone. Turning on the lock needs a calculator PIN (set in the same
  card; it opens WinCommander from the calculator). Stealth mode stops at the
  first step that fails and offers "Turn on the rest". The card shows a status
  pill and the four protections as one compact row (two columns on a medium
  window, one on a narrow one), each with its own switch and a short
  description; the PIN and cover-name fields sit in the cards that need them,
  and the two hide cards are dimmed with a "Needs administrator" note when
  elevation is missing. A blank cover name uses "Calculator".
- Ask for administrator rights on every normal launch that is not already
  elevated: split-token administrators get the UAC consent prompt, standard
  accounts get the administrator-credential prompt. Cancelling or a failed
  prompt keeps the normal window; sign-in, PIN-cover and disabled-UAC launches
  never prompt. A slow settings read no longer keeps the old non-elevated copy.
- Fix large Vaults (a 50 GB hidden-volume Vault, a 10-15 GB hidden decoy)
  failing with the generic "could not create this Vault" message. WinCommander
  waited only 120 seconds for the service, but a hidden-volume Vault fills the
  whole outer volume with random data. Creating a Vault now waits 120 seconds
  plus 1 second per 10 MiB of outer size (at most 12 hours), and always two
  minutes longer than the service's own limit. A timed-out request is still
  never replayed. The Create step says when a Vault of 2 GB or more can take
  several minutes, and blocks a file Vault that clearly exceeds the free space
  on its drive. Not enough free space, a hidden volume too large for its outer
  volume, a Vault held open by antivirus or indexing, and a timeout now each
  get their own plain message instead of the generic one. The service waits
  the same size-scaled time for the Vault helper instead of a flat 120
  seconds, and a standard-account creation checks free space before it
  creates anything: one copy per drive when the Windows drive and the
  destination differ, two on one drive, instead of two on both.
- Fix an in-place update (`/UPDATE`, including the in-app updater) leaving
  the machine-wide Start Menu and Desktop shortcuts missing. Setup never
  created a shortcut in update mode, so an install whose shortcut was gone
  stayed without one. An update now restores a missing shortcut that no
  earlier setup offered; one the user removed afterwards stays removed, and
  a shortcut parked by hide or Calculator mode is not restored. The first
  update of an install made before this change restores a Desktop shortcut
  that was deliberately removed earlier, once. The release gate now checks
  both shortcuts after setup and after `/UPDATE`.
- Fix Privacy Shield reporting "Camera availability could not be confirmed" on
  a machine with no camera. The camera probe never answered "no camera" inside
  a Remote Desktop session or while Windows camera consent read Denied, which
  is the default on Windows Server; it now treats an empty device inventory
  plus an empty video-source list as a definite no, retries a failed query
  once, and the card then shows "No usable camera detected." only. A genuine
  query failure shows one plain sentence with the refresh icon. Starting Shield
  on a device that is not Fleet-managed no longer requires a single Windows
  session, and a Fleet-managed refusal is worded for people. Backend errors no
  longer show literal quotes. The AI runtime row is hidden while no camera is
  available.- Add an explicit classical release profile (`WINCMD_RELEASE_PROFILE=classical`).
  It builds without `experimental-*` features and skips the post-quantum trust
  gate, matching 3.6.x and the production licence worker; the default profile
  and its gate are unchanged. `release.yml` publishes only under classical, and
  `start-free-release.ps1` now starts that run by default.
- Show a Fleet command's effect in WinCommander within about two seconds. The
  app now re-reads settings and the open panel when settings change or the
  Pro agent reports a finished command, and the Fleet apply loop adopts that
  command immediately instead of at its next 60 second tick. The existing polls
  remain as the fallback.
- Fix the Fleet forensic System Cleanup projections. Shadow copies and NTFS
  journals send Yes/No text instead of an unsupported boolean column; empty
  sources and categories with nothing to report return a zero-row table
  (no phantom size-0 row); a device without mobile broadband returns an empty
  not-applicable Cellular table; a table cut at the 200-row limit reports its
  real total and `truncated`. Process review checks each executable once and,
  like ShellBags, stops at a row cap and a 30 s budget, and ShellBags no
  longer re-scans the whole Bags tree for every key.
- Fix Fleet process review and Amcache views failing on a live device. The Fleet
  collector read Windows PowerShell's stdout in the OEM code page, so one
  non-ASCII value (a signer such as `Baden-Württemberg`) made the whole table
  invalid UTF-8; it now forces UTF-8 like the regular backend runner. When
  Windows has not mounted the Amcache hive, the Amcache view lists its five
  categories as unavailable (counts left out, never zero) with a State column
  instead of failing.

- Let Privacy Shield start locally when the Windows service confirms the
  device is unbound, even if the profile still holds older Fleet settings.
- Request Windows administrator approval when a split-token administrator
  opens WinCommander. Standard users and administrators who decline can
  continue with their current Windows permissions.

- Start the verified Everything client when WinCommander opens and wait for
  its filename index during the first search if it is still loading. File
  Search shows filename and content index failures separately.
- Disable the dashboard camera button when no camera is detected. When a
  camera is detected and permissions are known, show Camera On or Camera Off.
  Privacy Shield shares the manual camera recheck with the dashboard.
- Retry Bluetooth policy checks silently on the dashboard. The control shows
  On or Off once Windows confirms the policy.
- Explain drive letter reservations, mobile broadband requirements, and
  incomplete process details in their respective controls.

- Fix Fleet Connect crashing the app with a stack overflow, and let the address
  be corrected or retried while connected, offline, enrolling or waiting for
  admin approval. Fleet addresses must use `https://`; the panel now says so.
  Windows v2 enrollment is aligned with current Fleet servers; this is covered
  by automated tests, and no live-server acceptance run is recorded here.
- Add a stale-trust repair to the Fleet panel. After a native confirmation that
  names the old and new server, organization and device, the machine's held
  Fleet trust can be replaced when the same device and signer are linked to a
  new server or key. Anything else is refused and reported with a fixed reason.
- Bind the Fleet policy-epoch floor to the enrollment generation, so a first
  epoch from a user-confirmed new generation is adopted instead of being
  refused as not newer. Every adoption now needs the protected service to
  confirm that the epoch's signed generation is the one it trusts; a replayed
  epoch from an older generation, and same-generation rollback, are refused.
  Refusals name the failing condition in the Fleet status. The service now
  persists a confirmed generation and returns it, with the trust generation,
  through its session-helper `get_policy` reply, so the app no longer needs the
  administrator-only trust read. The app and `commander-svc` must be updated
  together: against a service that does not return these fields, adoption fails
  closed.
- Add a remote-command dispatch toggle to the connected Fleet panel (no
  re-enrollment). Turning it off is always allowed; if the running agent cannot
  be reconfigured the panel says so.
- Report Shield status to Fleet read-only (single interactive session, no policy
  applied, nothing acknowledged) when the service or handoff check fails.
- Add pure Shannon-entropy and entropy-delta helpers to `wincmd-shared` for a
  future ransomware-monitor signal. They are not connected to the live monitor.

- Consolidate the two Switch components into `ui/switch.tsx` (Radix, with a
  `size` option for dense rows) and remove `shared/WCSwitch`. The compact
  switches now use the standard track colours.

- Bound the Everything search bar's result-icon cache with the existing
  `FileIconCache` (entry, 8 MiB and age limits) instead of an unbounded map.

- Add the typed service and Pro-gated UI integration for optional Vault cloud
  backup, with explicit consent and automatic uploads while mounted. Requires
  the paired Pro implementation; installed and real-account acceptance remain
  pending before release.

- Merge the `dev` branch into `main` (859 commits) and fix the clippy and unit-test
  failures that came with it. The standard-user evidence-storage regression and
  oversized files are recorded in
  [STATUS.md](STATUS.md#the-2026-11-16-merge-of-dev); gated work stays disabled by default.

- Report RDP idle protection as unverified when only saved settings are available.

- Refuse a service-managed Vault mount when its recorded filename or identity
  conflicts with another known mount, without disturbing the existing mount.

- Show App Launch Cache names and paths only from matching name/path fields;
  version and hash values remain available in local raw details instead of being
  mislabeled.

- Let the connected Fleet agent read the owning desktop session's current local
  arming setting through authenticated IPC, without retaining an armed snapshot.

- Show the last verified Fleet contact from the device status as a historical
  observation, without treating it as the current connection state.
- Show the calculator before any startup animation when its PIN gate is active,
  including when an open window is locked again.
- Handle temporary settings-service contention and concurrent settings migration
  more reliably, while preserving saved settings and startup security checks.
- Detect camera hardware separately from camera privacy permissions on the
  Dashboard, and show an unknown state when detection cannot be confirmed.
- Run independent Dashboard Fix All repairs three at a time while keeping
  installers and conflicting maintenance operations ordered.
- Simplify cleanup cards and detail tables, hide wholly empty columns, move
  pagination above results, and place disk-analysis item details in the selection
  bar. Distinguish disabled clipboard history and protected process details from
  scan failures, skip folder junctions during database-trace scans, and verify
  schedule changes without retrying unrelated upgrades.
- Expand local clipboard detection with scored secret and identifier checks,
  per-detector controls and content-free explanations. Optional contact checks
  start off. Paid clipboard masking preserves unrelated text and rechecks the
  current clipboard before replacing matched spans; alert cooldowns never
  suspend protection of a newly copied value.
- Share bounded content-rule contracts with managed devices while retaining
  source-specific reporting, signed service verification and compatible epochs
  for older clients. Detection results contain categories and counts, not text.

- Refresh saved Wi-Fi profiles after failed or partial cleanup and mark results
  stale when verification fails, instead of leaving an apparently current list.
- Improve cleanup account scoping, partial scan details, protected recurring
  schedules and independent post-cleanup read-back. Preserve individual schedule
  ownership and require explicit consent for sensitive recurring cleanup.
- Flow activity distinguishes failed, simulated and unknown outcomes; commands
  with failed or unfinished receipts no longer appear completed. Manual execution
  remains unavailable until actions can actually be run through that path.
- Verify adopted settings and saved application inventory, share process-elevation
  observation, and retain authenticated service-peer identity across requests.

- Extend neutral Fleet wire contracts with device-command metadata, detection
  timing, approved-id cleanup selection and explicit unknown browse status.
  Sensitive-finding wire support remains an optional, default-off feature.

- Add category/detail maintenance workspaces, developer and AI-cache groups,
  explicit project-folder cache review, duplicate selection preferences, and
  bounded local operation history. Refresh application and privacy review
  layouts while retaining their existing guarded operations.
- Add disk-analysis sunburst and treemap depth controls, navigation history,
  large-file filtering, scan progress/cancellation, and partial-result notices.
  Label logical sizes explicitly and require a fresh analysis after deletion.
- Privacy Shield now honors separate notification choices for look-away/no-face,
  multiple faces and phone detection. Changing blur or detector timing while a
  Fleet-managed session runs reapplies the local detector settings; device
  status still requires a current native observation to confirm the result.

- Hidden backend commands now rebuild Windows PowerShell's module search path,
  preventing inherited PowerShell 7 modules from breaking permission checks.

- Bluetooth now uses Windows policy only. Adapter disabling and its background
  worker are removed. The compact control verifies policy results, reports
  unsupported or managed systems explicitly and uses in-app notifications.
  Existing device-change records are preserved for manual administrator review.
  Server 2025 and later now check the installed policy provider instead of
  being rejected solely because they are Windows Server. Service changes
  preview affected dependencies and save settings before applying; a collapsed
  view compares snapshots, excluding driver settings from the service profile.

- Setup refreshes recognized Scrub metadata menus for signed-in Windows users,
  so a previous installation path does not keep launching an older scrubber.
  Disabled and unrelated menu commands are preserved; offline accounts finish
  refreshing when they start the installed app.
- The log viewer recognizes current encrypted records and startup migration
  preserves them without encrypting them a second time.

- Manually saved Wi-Fi SSID/BSSID identities now persist to local settings, so
  periodic policy reconciliation and app restart cannot discard them.

- Automatic app updates retain an in-session two-attempt limit even when a
  stale settings snapshot would otherwise reset the retry count.
- Port Guard groups repeated observations without discarding raw history,
  wraps narrow tables and keeps detailed guidance in info buttons. Firewall
  actions require verified Windows read-back from the matching Pro update.

- Trust Store Audit runs its read-only Windows scan without opening a console
  window and avoids module conflicts when launched from PowerShell 7.

- Wi-Fi Guard moves connection, alert-history and reporting explanations into
  info buttons while keeping status, alerts and errors visible.

- Port Guard presets no longer show outgoing-service reference buttons or empty
  categories. Existing saved watches and firewall rules are unchanged.

- USB observation matches Windows device identities correctly, associates volumes
  by their physical device, and refreshes watcher health and transfer totals.
  The timeline distinguishes current sessions, observed mounts, and verified
  manual policy actions. Pro must also be updated for verified block/allow receipts.

- Secure Storage wraps long Vault paths and errors within the card. Narrow cards
  place paths and actions on separate rows, keeping controls reachable without
  horizontal scrolling.
- Silent startup avoids a redundant Windows capture-setting call while retaining
  verified capture-protection changes in either direction.
- Machine-wide Vault cleanup no longer requires an interactive Explorer session.
  This prevents completed dismounts from leaving cleanup records that block
  later mounts. Matching Pro and service components are required; changed mount
  identities remain protected from stale recovery records.
- Vault components verify compatibility in both update orders before mounting,
  preventing a newer component from starting work an older component cannot clean up.
- Secure Storage preserves the specific mount-identity error when its drive
  list cannot be verified, instead of showing only a generic refresh failure.

- Secure Storage access errors distinguish Windows file permissions from Vault
  ownership and explain where the authorized owner can review access, including
  containers moved between PCs.

- Silent sign-in now enforces the native hidden window state before startup
  work begins. Opening from the tray still restores a maximized WinCommander
  window after the interface is ready.

- Personal Vault dismount skips Syncthing when the account's container is confirmed
  unbound. Bound or unresolved sync state still requires a confirmed pause; retained
  mounts and interrupted setup preserve that distinction across service recovery.
- RAM disk creation reuses a matching mounted ImDisk volume and removes stale
  copies without clearing the live drive letter. The service reconciles legacy
  768 MB TEMP disks on R: when it starts.

- Administrator sign-in uses the installed elevated startup route directly;
  background launches stay hidden and manual launches retain the elevation handoff.
  Installer reconciliation preserves startup opt-outs and removes only owned legacy
  routes. Early open requests wait for painted content, and a failed renderer gets
  one bounded recovery without displaying a blank window. The restored 3.6.4 splash
  animation is unchanged.
- Restored Stego video backup, container attachment, extraction, and snapshot
  refresh. Ordinary mounts retain existing Windows permissions and do not open
  the retired account-repair prompt. Fleet access restrictions remain unchanged.
- Stego backup replacement and refresh preserve explicit confirmation through
  the desktop-to-Pro interface; malformed confirmation values are rejected.
- Secure Storage reports recovery permission failures through the active mount
  dialog, without a competing raw-error toast or an account-repair retry.
  Existing Windows permissions may still block access.
  Post-mount drive and inventory checks have bounded waits and never turn an
  unavailable observation into a successful mount notification.
- Secure Storage refreshes visible mount status and prevents older responses from
  restoring stale rows. Refresh stays disabled while requests are running.
  Windows container-path prefixes are removed for display; unknown paths are
  labelled unavailable. Dismount errors wrap beneath the row instead of inside
  the action buttons, and attention dialogs fit smaller windows.
- Normal Vault workflows use service-authorized inventory and dismounts. Ordinary
  machine-wide mounts require an elevated administrator; shared Fleet mounts
  also require policy access. Private mounts remain owner/session-only. Unknown
  mount identities are preserved and reported, never guessed or silently removed.

- Standard-user startup retains strict service identity verification while
  allowing the connecting account to query the service's process identity.
  The bounded connection retry sends no request before verification; it grants
  neither administrator powers nor access to another account's saved settings.
- Vault failures distinguish a missing Pro engine, denied ownership and unlock
  failures, with visible inline notices and confirmed-operation notifications.
  Owner and drive-letter selectors have visible borders. Updates require native
  administrator authorization; standard-user licence activation remains available.
- Standard users can change their own app-capability consent without writing
  machine policy. Managed overrides and failed writes remain visible instead of
  reporting an applied permission; startup errors explain the failure category.

- Dashboard privacy fixes for Recall, background communication, Office logging
  and automatic device encryption now require Windows read-back before success.
  Failed or unverified changes stay visible with an explanation. Status checks
  work with Windows PowerShell 5.1 and reload the saved BitLocker policy correctly.

- Vault mount/dismount failures remain visible beside the operation, including
  unlock, drive-letter and Windows-access guidance. Authorized existing mounts
  can be opened without a disruptive remount. Fleet and Quick Mount selectors
  use service-checked letters, excluding occupied drives and saved reservations.
- The personal-data recovery notice can be dismissed for the current session;
  recovery details remain in Settings and protected features remain locked.

- Setup no longer mounts offline user registries during legacy startup cleanup,
  so leftover temporary hives from an earlier failed setup cannot block retries.
  Signed-in profiles are cleaned during setup; other profiles clean their own
  legacy startup entries when they next launch WinCommander.

- Network maintenance can inspect machine and current-user certificate stores,
  compare roots with the Windows AuthRoot reference, and save a local baseline.
  Differences are transparency signals, not malware findings; nothing is
  removed or changed.

- File Search exposes indexed-folder controls before searching and refreshes
  active results after indexing or synced file changes. Folder-operation and
  private-index failures are visible, and temporarily locked files no longer
  discard progress for other files in a private indexing batch.
  VeraCrypt mounts without a Windows canonical volume path can now create and
  read their search index using files kept on that same encrypted drive.

- Settings writes use separate temporary files for concurrent launches and
  briefly retry Windows file-sharing conflicts. First-time key creation keeps
  one committed key; unreadable keys and encrypted settings are preserved
  instead of automatically replacing the key and resetting preferences.

- Selected VeraCrypt folders now use an index inside the mounted volume.
  Search checks the native volume identity, updates incrementally while mounted,
  and reads existing indexes without writing on read-only mounts, including
  after the same container moves to another drive letter. Private
  results and previews are cleared after mount changes; private paths are
  excluded from persistent launch history and the ordinary search provider.
  Pending filename searches are cancelled when the mounted volume changes.

- Secure Storage and Quick Mount now request machine-wide writable drives,
  preserve existing file permissions, and retain explicit read-only mounting.
  The service checks the caller's access to containers and keyfiles before
  mounting; Fleet-managed containers keep their separate permission policy.
- Fleet Vault mounting uses current assigned principals rather than stale
  generated-group membership retained by an older Windows sign-in.
  Existing policies using the older generated-group file permissions show
  degraded health until an administrator saves the policy again.

- Manual NSIS upgrades and reinstalls preserve existing licence, preferences,
  Vault policies and the separately installed Pro runtime instead of invoking
  the previous NSIS uninstaller.
- Setup can repair an existing encryption driver's empty protected permission
  list, requiring the pinned file hash and a valid signature before keeping the
  repair. Vault diagnostics distinguish payload validation from driver startup
  failure; container permissions are unchanged.

- File-icon reuse now has entry and string-byte limits with least-recently-used
  eviction. Later lookups refresh aged icons and retry temporary failures while
  preserving visible-row priority and shared in-flight requests.

- Startup benchmark reports reject empty or malformed measurements, record
  per-phase sample counts, and distinguish partial diagnostics from complete
  supplied data. Strict reports require build/environment provenance and a
  successful protection outcome. Report files never overwrite existing files.

- Fleet reconnect preserves managed state. Connection changes validate keys,
  bind native approval to the complete request, and recheck the observed
  authority before saving. A failed settings read no longer implies a new device.

- Local settings changes and imports now enforce managed policy consistently.
  Native policy ingestion preserves its verification key and rejects stale
  epochs. The renderer and CLI no longer expose arbitrary policy ingestion.
- ActivityWatch's local API transport rejects redirects, ambient proxies,
  namespace escapes and oversized streamed responses.
- Restore native module and watcher build integrity, update the CLI catalog,
  and retain content-free diagnostics when USB actions fail.

- Windows service machine-setting changes now report success only when the
  read-back state matches the request. RDP lock verification distinguishes an
  unavailable firewall query from an unlocked state and checks each matching
  active rule independently.

- Updated the document-search XML readers for quick-xml 0.42, so the merged
  dependency update builds correctly while preserving EPUB, Office, and ODF
  extraction behavior.

- Full development startup now checks the Windows service and matching Pro
  helper before exposing the dev server, including direct `dev:server` and
  Tauri launches. Missing services enter installation instead of failing the
  running-state check; failed synchronization stops startup.

- The frontend development watcher ignores runtime staging files, preventing
  locked Vault test containers from crashing the dev server on Windows.

- Development service updates now wait at most 150 seconds for a graceful stop,
  allowing Vault cleanup to finish and reporting a timeout before replacing files.

- Vault permission saves now clear outdated access results, and both Mount
  buttons respect permission-verification failures. Personal Vault lookup uses
  consistent Windows file paths while retaining owner and file-identity checks.

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
  refusing reparse points and hard links. Folders are erased only after a full
  pre-inspection, entry by entry through pinned handles, and refuse a target or
  ancestor replaced after selection. These are source and
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
