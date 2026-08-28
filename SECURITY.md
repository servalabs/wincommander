# Security — WinCommander

This is the public disclosure policy, high-level threat model, and honest
product-limit statement. Detailed internal designs, runbooks, vulnerability
ledgers, and acceptance evidence are maintained privately.

## Reporting a vulnerability

Do not open a public issue, pull request, or Discussion for a vulnerability.
Email **security@servalabs.com** with:

- the affected component and version/build hash;
- reproduction steps or a minimal proof of concept;
- expected and observed behavior;
- your impact assessment and any suggested mitigation.

Response targets are acknowledgement within 72 hours, a triage decision within
7 days, and a critical-fix target of 30 days where feasible. Ask in the first
email if you want release-note credit.

**Safe harbor.** Good-faith research under this policy is authorized when it
avoids privacy violations, data destruction, service degradation, and testing
of other customers or ServaLabs production infrastructure without written
permission; accesses only the minimum data needed; and allows reasonable time
for remediation. This is not a bug-bounty program.

Pro source review may be available by invitation to press, technology
reviewers, and security researchers through **licensing@servalabs.com**. Keep
vulnerability reports on **security@servalabs.com**.

## Supported versions

| Version | Support |
| :-- | :-- |
| Current published minor | Supported |
| Previous minor | Supported for 30 days after the next minor ships |
| Older versions | Not supported |

## Threat model

WinCommander assumes:

- frontend/WebView input can be attacker-controlled;
- local unprivileged users may attempt to read or influence state;
- network peers, update paths, and licence responses may be intercepted or
  modified;
- a paid entitlement shown in the UI may be forged;
- Windows, firmware, storage media, third-party tools, and Administrator
  sessions can fail in ways the application cannot fully control;
- an organization administrator may deliberately enable managed monitoring on
  a Fleet-enrolled device.

WinCommander does not claim to defend against a fully compromised Windows
kernel/firmware, an attacker with equivalent Administrator/SYSTEM control, or
physical attacks that operate before Windows enforcement begins.

## Public security posture

- Backend validation—not UI visibility—enforces tier, entitlement, arguments,
  risk, and administrative requirements.
- Frontend input cannot select an arbitrary shell command, executable, update
  host, or integrity hash.
- Tauri filesystem and shell capabilities are allow-listed and should remain
  narrow.
- Local settings and sensitive state use authenticated at-rest protection where
  implemented; secrets and signing material are not committed to this repo.
- Paid requests fail closed if the paid component or its validation is
  unavailable.
- Updates and published artifacts use signed metadata/artifacts and integrity
  verification.
- Fleet operations use authenticated devices, signed policy/command material,
  role checks, scoped organization access, and audit records. Exact private
  protocol and operational details are intentionally not public documentation.
- Destructive operations require backend validation and explicit confirmation;
  completion must not be inferred only from a process returning success.

## Privacy and managed monitoring

Free is local-first. Operational state and local search indexes remain on the
device unless the user enrolls it into an organization-managed Fleet service or
explicitly uses a network-backed capability.

On a Fleet-enrolled device, organization productivity reporting can include
application names, window titles, URLs/page titles, source-file paths,
project/language metadata, activity counts, and the interactive username. The
built-in ActivityWatch inputs use aggregate key/click/scroll counts rather than
keystroke content, and do not capture screenshots, webcam frames, or clipboard
contents. A generic watcher-data passthrough can carry any fields supplied by
an installed watcher, so administrators must assess their watcher
configuration rather than rely on an absolute no-content claim. Collection is
not controlled by a per-cycle in-app consent switch. Deploying organizations
are responsible for a lawful basis, clear notice, appropriate roles, retention,
export controls, and employee/user rights in their jurisdiction.

## Current operational limits

These qualify the product's security claims and are stated here because this
document owns the public limits. The threat-model boundaries above remain the
authoritative statement of what WinCommander does not claim to defend against.

- Ransomware alerts and response can reduce time to detection or containment.
  They cannot guarantee that no file changes before Windows observes activity.
- Unknown-keyboard approval is reactive. It cannot guarantee first-keystroke,
  pre-boot, firmware-level, or fast-replug prevention, and a UI confirmation is
  not proof of trusted physical input.
- Secure deletion and crypto-erasure depend on media behavior, firmware,
  encryption state, escrow, and verification. A successful request, or removed
  local access, does not prove every recovery copy is gone.
- Fleet reachability is not an air gap. An offline device cannot receive a new
  command until it reconnects.
- Fleet enforcement is not continuous. The fleet agent runs inside the Pro
  sidecar that the desktop app spawns, so check-ins, command delivery, and
  policy reapplication stop when WinCommander is not running — including an
  ordinary close, logoff, or user switch, with no adversarial action involved.
- Fleet reports a device's state from a local operating-system probe rather
  than from state WinCommander confirms it applied, and no component reconciles
  managed desired state back onto the machine. A console reading is evidence of
  what a probe observed, not proof that a policy is being enforced.
- Windows Server/RDS, physical USB, hardware-backed keys, signed installers,
  and clean-machine behavior still require environment-specific validation.
  Planned or source-tested functionality is not a shipped or independently
  certified guarantee.

## Cryptography and post-quantum status

WinCommander uses established classical signatures, authenticated encryption,
password derivation, HMAC, and TLS through maintained implementations. It does
**not** claim to be post-quantum safe today. A future post-quantum claim requires
coordinated migration of every trust root and peer, downgrade protection,
rotation, interoperability testing, and release-chain verification—not a
client-only setting.

## Reporting non-security defects

Use the normal public issue process for public Free bugs that do not disclose a
security weakness, private implementation, customer data, credentials, or
internal infrastructure. When unsure, email the security address.
