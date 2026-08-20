# Non-Goals — WinCommander

This document defines the boundaries that keep WinCommander understandable and
safe. `POSITIONING.md` explains the product; `SECURITY.md` explains the threat
model; the private Pro roadmap owns future paid work.

## Product scope

- **Windows client, not one cross-platform desktop binary.** WinCommander owns
  Windows 11. TuxCommander and secureOS/Privon are separate products sharing
  explicit Fleet contracts where appropriate.
- **Local-first, not default cloud telemetry.** Free keeps operational state on
  the device. Fleet reporting occurs only after explicit enrollment into an
  organization and uses documented, bounded contracts.
- **Open core, not an entirely open product.** Free is AGPL-3.0; Pro remains a
  separately licensed proprietary sidecar. Public code must never contain or
  reconstruct private Pro implementation.
- **Security control and evidence, not antivirus replacement.** WinCommander
  can inspect posture, harden configuration, and orchestrate approved actions;
  it does not replace Microsoft Defender or a dedicated EDR.
- **Bounded authorized investigation, not a universal forensic laboratory.**
  Fleet may request fixed, typed views and Investigator may collect approved
  evidence. Neither becomes unrestricted remote shell, arbitrary disk
  acquisition, or a substitute for specialist lab tooling.
- **Explicit risk, not a one-click optimizer.** Destructive or
  security-reducing actions remain visible, classified, confirmed, and
  auditable.

## Architecture boundaries

- **No plugin marketplace or third-party executable SDK.** The trusted desktop
  boundary remains the ServaLabs-signed Free binary plus separately entitled
  Pro components.
- **No frontend security authority.** Entitlement, scope, role, command class,
  confirmation, and device ownership are rechecked behind the UI.
- **No arbitrary command, path, host, query, or integrity value from the
  browser.** Remote operations use fixed catalog IDs and server-derived risk;
  download hosts and artifact identities are pinned in trusted code.
- **No second Fleet command or policy system.** New work reuses enrollment,
  signed epochs, check-in, approvals, MPA, result ingestion, and audit.
- **No central hoarding of endpoint forensic content.** Search and System
  Cleanup filtering happen on the endpoint; Fleet receives bounded typed
  projections. Detailed acquisition belongs to an explicit Investigator case.
- **No home-grown inventory or vulnerability scanner.** Fleet may wrap pinned,
  verified FOSS engines and normalize their outputs, but it does not recreate
  their package catalogues, SBOM formats, or vulnerability databases.
- **No casual webview privilege expansion.** Remote content never inherits the
  trusted main-window capability set.
- **No entitlement bypass.** Development, trial, portable, and release paths
  all recheck paid authority at the trusted boundary.

## Safety and claim boundaries

- **No destructive voice trigger.** Emergency actions require deterministic,
  authenticated inputs.
- **No universal flash-wipe claim.** Overwrite passes cannot guarantee removal
  from SSD/NVMe remapped cells. Prefer key destruction and supported firmware
  sanitize, with exact/degraded results.
- **No silent replacement of OEM or Windows recovery partitions.** A managed
  wipe environment uses an explicitly allocated, verified partition and must
  have uninstall and boot-recovery paths.
- **No self-issued certification claims.** Evidence reports describe observed
  controls; they do not declare legal compliance, FIPS validation, or forensic
  admissibility by themselves.
- **No claim that delivery equals execution.** Server approval, device receipt,
  process launch, and observed result are separate states.
- **No claim that mesh reachability is an air gap.** A private overlay network
  is still a network and retains observable connection metadata.
- **No production secret in logs, docs, fixtures, repository history, or client
  responses.** One-time tokens are shown once and protected thereafter.

## Deliberately separate products or decisions

- Mobile UI and phone lifecycle remain secureOS/Privon responsibilities.
- Full case acquisition, sealing, and examiner workflow remain Investigator
  responsibilities.
- Provider-native Wazuh/Velociraptor administration stays in their own tools;
  Fleet stores normalized references and approved command results.
- New high-risk collection categories require a separate evidence contract and
  cannot arrive as an incidental extension of search or cleanup.
