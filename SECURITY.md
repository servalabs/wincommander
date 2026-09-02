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
- Fleet continuity depends on the managed endpoint runtime staying healthy.
  Closing the main window hides WinCommander to the tray rather than exiting
  it, and a Windows user switch leaves the previous session running. But an
  explicit quit, a sign-out, a crash, a failed startup, or a sidecar or update
  failure can interrupt session-bound check-ins and enforcement. The installed
  LocalSystem service does not yet own the full Fleet lifecycle.
- Managed-state enforcement is partial, not absent. Supported settings that
  have a mapped enable/disable command are periodically probed and re-applied,
  including organization-locked paths. Controls without such a mapping, and
  one-shot or irreversible actions, are observational only. Console readings
  can also come from evidence channels with different freshness and coverage,
  so a reading is not by itself proof that every policy path converged.
- Windows Server/RDS, physical USB, hardware-backed keys, signed installers,
  and clean-machine behavior still require environment-specific validation.
  Planned or source-tested functionality is not a shipped or independently
  certified guarantee.

## Cryptography and post-quantum status

**Current releases are not quantum-resistant end to end.** WinCommander uses
maintained implementations of classical signatures, authenticated encryption,
password derivation, HMAC, and TLS. Protected application state uses 256-bit
symmetric encryption where implemented, but symmetric encryption alone does not
make licences, updates, Fleet identity/commands, evidence, recovery objects, or
network peer authentication post-quantum.

A supportable current statement is:

> WinCommander uses AES-256 authenticated encryption for protected application
> state and has a documented migration programme for standardized post-quantum
> key establishment and signatures. Current releases are not quantum-resistant
> end to end.

Any future protection claim must be scoped to WinCommander-controlled trust
paths and requires all of the following before publication:

- standardized post-quantum key establishment and signatures through reviewed
  implementations;
- explicit, versioned algorithm and key identifiers rather than inference from
  key or signature size;
- downgrade-resistant hybrid verification during migration, with every required
  component verified rather than accepting either one;
- key generation, custody, rotation, revocation, recovery, and bounded legacy
  verification;
- shared known-answer, interoperability, malformed-input, replay, rollback, and
  mixed-version tests across every issuer and verifier;
- an application-level release-verification path that does not rely only on
  classical platform signatures; and
- separate disclosure of Windows, firmware, DPAPI/CNG internals, Authenticode,
  TPM, public TLS, Tailscale, VeraCrypt, identity providers, timestamp
  authorities, and other external dependencies.

Do not describe WinCommander as **“quantum-proof,” “unbreakable,”
“future-proof,” “quantum key cryptography,”** or broadly **“post-quantum
secure.”** WinCommander is pursuing standardized post-quantum cryptography, not
quantum key distribution. A compiled PQC library, one migrated feature, or an
AES-256 badge is not sufficient evidence for a product-wide claim.

## Reporting non-security defects

Use the normal public issue process for public Free bugs that do not disclose a
security weakness, private implementation, customer data, credentials, or
internal infrastructure. When unsure, email the security address.
