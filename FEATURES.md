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
- Local productivity view backed by the user's local ActivityWatch instance
  when installed and enabled.

### Maintenance

- Preview-first maintenance flows for supported cleanup categories.
- Duplicate, empty-folder, broken-shortcut, package, registry, and firewall
  review surfaces where available in the current build.
- Backend-owned candidate identifiers and live revalidation before mutations;
  the frontend does not submit arbitrary filesystem targets.
- Explorer and search-result secure erase for regular files uses one verified
  Windows handle for overwrite, read-back, and deletion. Folder shredding is
  refused until handle-safe recursive traversal is implemented.

### Search and automation surface

- Fast local filename search through the supported Everything integration.
- Local keyword search inside supported document and text formats.
- Read-only production CLI for catalog and audit workflows. See
  [docs/cli.md](docs/cli.md).
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
  keyfile to select a standard, outer, or hidden volume automatically and are
  read-only; writable outer-volume mounts require the explicit protected
  policy workflow.
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
[ARCHITECTURE.md](ARCHITECTURE.md) and the detailed [documentation map](docs/README.md).
