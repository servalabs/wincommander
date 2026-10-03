# Features — WinCommander

This is the public, user-visible capability and entitlement summary. The public
code is authoritative for Free behavior. Private implementation details,
internal plans, and acceptance evidence are intentionally omitted.

## Editions

| Edition | Intended use | Source / access |
| :-- | :-- | :-- |
| Free | Local Windows visibility, baseline privacy and hardening controls, alerts, maintenance, and search | Public AGPL source in this repository |
| Pro | Advanced local automation, containment, secure cleanup, vault workflows, and premium safeguards | Proprietary; paid entitlement |
| Investigator | Isolated forensic acquisition, analysis, verification, and reporting workflow | Proprietary; separate Investigator entitlement |
| Fleet | Organization-managed policy, remote operations, reporting, integrations, and retention | Proprietary service entitlement |

An on-screen lock is not the security boundary. Paid operations are checked by
the backend and private component before execution.

## Free

### Windows privacy and posture

- Visible Windows privacy, telemetry, application, network, and security-posture
  controls where the public backend supports the operation.
- Current-versus-intended state reporting so the UI can show whether Windows
  actually applied a requested setting.
- Fix All keeps supported Explorer preferences in the current Windows account,
  even when machine-wide fixes are selected. Machine protections still require
  elevation; mixed or application-defined actions retain their actual scope.
  Failed or pending changes remain actionable. Kernel DMA Protection requires
  confirmed Windows hardware status, not just a saved policy preference.
- Safety metadata for administrative, security-reducing, and destructive
  operations.
- Export/import of non-secret application settings through validated backend
  paths.

### Local alerts and visibility

- Local ransomware-style mass-change alerting.
- Basic USB attach/detach timeline and device visibility.
- Local clipboard-risk warnings and user-controlled clear/snooze behavior.
- Monitor Operations Center: one content-free view of Free and Pro monitor
  coverage, armed state, recent-event counts, cadence, stale/degraded health,
  unavailable services, and entitlement-locked capabilities.
- Local security and application status surfaces.
- Read-only trust-store audit for machine and current-user certificates, with a
  locally saved baseline and Windows AuthRoot comparison. Differences are
  review signals; they do not identify a certificate as malicious.
- Local productivity view backed by the user's local ActivityWatch instance
  when installed and enabled.

### Maintenance

- Scheduled cleanup uses compact Task Scheduler names: `SL-SW-<code>` for
  system tasks and `SL-UW-<code>-<SID>` for per-user tasks. App updates migrate
  recognized legacy tasks while preserving their schedules and disabled state.
- Turning auto-start off removes the app's logon and elevated-launch tasks;
  turning it on recreates them as `SL-AS` and `SL-EL`. Updates preserve OFF.
- Preview-first maintenance flows for supported cleanup categories.
- Service Profile recommendations use current Windows service settings, not an
  expiring run timestamp. Already-applied settings stay resolved across restarts;
  changes and per-service failures remain actionable.
- Guided Routine Hygiene previews an exact, test-locked cache allowlist and
  refuses bulk clear when any category is missing, busy, failed, or unscanned.
  Clears run sequentially and publish an authoritative post-action rescan.
- Duplicate, empty-folder, broken-shortcut, package, registry, and firewall
  review surfaces where available in the current build.
- Backend-owned candidate identifiers and live revalidation before mutations;
  the frontend does not submit arbitrary filesystem targets.
- Explorer and search-result secure erase for regular files uses one verified
  Windows handle for overwrite, read-back, and deletion. Folder shredding is
  refused until handle-safe recursive traversal is implemented.

### Search and automation surface

- Fast local filename search through the supported Everything integration.
- Instant Search installation checks both Everything and its separate search CLI,
  repairs missing components, and verifies their presence before reporting success.
- Local keyword search inside supported document and text formats.
- Selected VeraCrypt folders keep filename/content indexes inside their mounted
  volume. Existing indexes support read-only access; writable mounts reconcile
  changes. Private results disappear after locking. See the
  [security boundary](SECURITY.md#private-volume-search).
- Read-only production CLI for catalog and audit workflows.
- Public UI and typed bridges for paid automation without publishing the paid
  engine.

### Read-only paid-feature support

- Upgrade discovery and entitlement status.
- Signed installation/launch surface for separately entitled components.
- Read-only status or verification where exposing it does not perform a paid
  mutation.

## Pro

Pro is for advanced capability that has substantial local value, higher
operational risk, or proprietary enforcement logic.

- Advanced ransomware attribution and configured process response.
- USB/HID policy, device intelligence, transfer metering, quarantine, and
  reactive unknown-keyboard approval.
- Advanced automation rules and event-driven actions.
- Encrypted-volume create, mount, dismount, recovery, and secure-erasure
  workflows. Personal and Quick Mount use the supplied password, PIM, and
  keyfile to select a standard, outer, or hidden volume automatically. Secure
  Storage and Quick Mount use machine-wide, writable mounts by default;
  Secure Storage also offers explicit read-only mounting. Existing file
  permissions remain unchanged. Automatic or prompted account-permission repair
  is unavailable; an inaccessible container reports an error without changing
  its permissions. Stego video backups can create, attach, restore, and refresh
  encrypted-container snapshots with explicit replacement confirmation; restoring
  a snapshot does not grant new Windows account permissions. Fleet-managed
  containers retain their separate selected-user and group permission checks.
- Personal Vault folder sync through Syncthing. Explicit setup installs a
  verified per-user engine if needed, retains existing sync identities, and
  follows the same container when its drive letter changes. After mounting,
  stopped folders get an automatic rescan; a remaining sync problem opens a
  dialog without dismounting the container. Manually paused folders stay paused.
  Shared Vaults are
  excluded; connecting another device still requires Syncthing device pairing.
- Deep cleanup, secure deletion, metadata/privacy-clean operations, and
  evidence-grade receipts where supported.
- Signed evidence-vault export and advanced verification/reporting options.
- Deception and tripwire capabilities such as canaries or honeypots.
- VM/sandbox, backup, recovery, and premium monitoring capabilities.

Some Pro operations are intentionally unavailable without Administrator rights,
supported Windows features, or explicit destructive confirmation.

## Investigator

Investigator is a separate product boundary, not a hidden Pro panel.

- Free may install and launch a signed Investigator artifact only when the
  licence contains the Investigator entitlement.
- The acquisition, analysis, case, evidence, and report workflow is private and
  runs in the separate application.
- A normal Pro entitlement or trial does not imply Investigator access.

## Fleet

Fleet adds organization-managed behavior and server-backed operations:

- device enrollment and managed configuration;
- signed policy distribution and drift reporting;
- remote command approval, dispatch, result, and audit workflows;
- organization roles, device groups, reports, compliance views, and
  integrations;
- centrally managed Vault access for standard and outer+hidden file
  containers, including group-authorized entries and one-request mount roles;
- organization productivity and security reporting where configured.

Local Fleet Vault management uses machine-wide Windows access groups.
Administrators can assign a new private Vault directly to an enabled Windows
user. Once saved, only its owner can edit the private policy while unmounted;
a standard owner cannot transfer ownership. Group membership changes remain
restricted to authorized administrators. Seeing a group does not grant access
to its Vaults.

Fleet enrollment is an administrative boundary. On an enrolled device, the
organization's configured productivity collection can include application
names, window titles, URLs/page titles, source-file paths, project/language
metadata, and the interactive username. Built-in ActivityWatch inputs use
aggregate key/click/scroll counts rather than keystroke content and do not
capture screenshots, webcam frames, or clipboard contents. A generic
watcher-data passthrough can carry any fields supplied by an installed watcher,
so administrators must assess their watcher configuration rather than rely on
an absolute no-content claim. There is no per-cycle in-app consent gate; the
deploying organization is responsible for a lawful basis, employee/user notice,
access control, and retention policy.

## Current limits and technical references

Post-quantum cryptography is not a shipped capability in current releases.
Classical public-key trust and external transport/platform dependencies remain,
so roadmap work, source scaffolding, or one migrated component must not be
listed as a product feature. The public status and the evidence required before
any scoped quantum-resistant claim are in
[SECURITY.md](SECURITY.md#cryptography-and-post-quantum-status).

Current operational limits that qualify these capabilities are in
[SECURITY.md](SECURITY.md). Deliberate product exclusions are in
[NON-GOALS.md](NON-GOALS.md); source ownership and technical interfaces are in
[ARCHITECTURE.md](ARCHITECTURE.md).
