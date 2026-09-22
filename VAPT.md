# WinCommander desktop VAPT and deployment-assurance checklist

## Assessment basis and scope

Prepared 2026-09-22 against commit `6899c868045adb518af44b7e4ff7c02ffcf67004` (package version `3.6.2`). This is a public, repeatable assessment specification, **not an executed penetration test, vulnerability clearance, compliance certificate, or deployment authorization**. No product code was changed and no intrusive, installed-Windows, hardware, provider, or independent tests were performed while preparing it. All test statuses below start at `not_started`.

Scope includes the WinCommander desktop renderer, Rust backend, PowerShell modules, local services/helpers, optional paid desktop components at their public interfaces, local data, installer, updater, and desktop build/distribution chain. Fleet scope ends at the endpoint: enrollment, configured destination, transport, incoming policy/commands, local authorization/execution, and outgoing evidence. Fleet server/console implementation, infrastructure, databases, and administrative APIs are excluded. Use an isolated protocol fixture for endpoint tests; separately authorize any real provider interaction.

Free is open source; Pro enforcement and some drivers/detectors are separately supplied. Public wrappers and wire types do not establish the security of their private implementations. Obtain the matching binaries and authorized source access for the independent desktop audit, or explicitly exclude those capabilities from the deployment decision. “Secure Storage” is the desktop Vault surface (`src/panels/vault/index.tsx`), not a separately proven storage engine.

This requested public checklist contains test specifications and evidence gaps, not confidential exploit results. Store execution logs, customer details, credentials, attack payloads, findings, and risk approvals in a restricted assessment repository. Follow [SECURITY.md](SECURITY.md), [OPEN_CORE.md](OPEN_CORE.md), and [NON-GOALS.md](NON-GOALS.md). A public source release and permission to install on a critical operational workstation are separate decisions.

### Threat model and boundaries

| Boundary | Assets and attacker capabilities | Required assurance |
| :-- | :-- | :-- |
| Untrusted content → React/WebView2 → native commands | Malicious file names, notifications, imported settings, embedded pages, compromised renderer, direct IPC calls | Origin/window isolation, typed validation, backend authorization and confirmations; UI locks alone grant no authority. |
| Standard user → `WinCommanderSvc`/elevated helper | Another local account, same-user malware, pipe squatting, PID reuse, forged SID/role/path, writable executable directories | Windows-derived identity, constrained verbs, caller-scoped file I/O, protected installation and state. |
| Free → Pro → engine/driver | Substituted sidecar, stolen session token, incompatible protocol, malicious local process | Verify the connected process and accepted artifact; reject unsupported peers; verify the effect of privileged work. |
| Vault policy → mount → filesystem | Stolen container, unauthorized user/group, stale token, policy race, writable alias, administrator misuse | Separate policy administration, permission to mount, and actual read/write enforcement. A key to the control room is not automatically a key to every safe. |
| Local disk/memory → logs/export/network | Keys, DPAPI material, PINs, recovery copies, content indexes, clipboard, camera frames, biometric-derived data, usernames and activity | Classify, minimize, protect, bound retention and disclose collection; never export sensitive content under an aggregate-status label. |
| Network/provider → desktop | MITM, malicious proxy/DNS/redirect, forged update, hostile configured Fleet peer, replayed command | Destination-specific trust, authenticated artifacts/messages, freshness, no secret forwarding across origins. |
| Contributor/build system → release → installation | Malicious dependency, compromised maintainer/runner/signing key, stale release, replaced DLL or installer | Locked inputs, review, SBOM, independent artifact verification, protected signing, recovery and revocation. |
| Endpoint → critical operation | Faulty storage, power loss, exhausted resources, offline operation, mistaken destructive action | Tested recovery, limited blast radius, independent read-back, controlled maintenance windows and operator approval. |

Evaluate unauthenticated network peers; ordinary, denied, read-only and read/write Windows accounts; elevated administrators; SYSTEM; authorized but malicious organization administrators; compromised same-user processes; and supply-chain actors separately. A standard-user-to-SYSTEM transition is a potential escalation. An administrator already controlling Windows is not a boundary the product promises to defeat: assess abuse visibility, separation of duties and recovery, and document residual risk. A compromised kernel/firmware or pre-boot physical attacker can defeat OS enforcement; use platform and physical controls rather than making an unsupported application claim.

### Standards adaptation

The complete supplied file inventory and applicability decisions are in the final traceability section. CERT-In secure-development guidance supplies design → development → audit → deployment phases. Its 2025 audit policy supplies independent review, written scope, evidence, severity, retest and report requirements. CSA-BR supplies management, protection, detection, response, recovery and improvement controls. NIST SP 800-115 supplies planning, discovery, bounded validation and reporting; SSDF supplies PO/PS/PW/RV lifecycle practices; CSF 2.0 supplies Govern/Identify/Protect/Detect/Respond/Recover outcomes.

Apply ASVS 5.0.0 requirements to the relevant native, WebView, API-client, storage and release boundaries. Use an L2-oriented baseline with selected L3 requirements for privileged, cryptographic and critical-infrastructure paths; this is a tailored profile, not blanket ASVS conformance. WSTG 4.2 methods apply to reachable WebViews, local listeners, request validation and endpoint communication. MASVS in the supplied `latest` file is **2.1.0**: adapt storage, crypto, IPC/WebView, network and privacy principles; Android/iOS platform tests are not applicable. Anti-obfuscation/resilience requirements must not turn readable open source into a supposed vulnerability. Web-server-only, GraphQL, SAML, OAuth-server and mobile-specific requirements require an applicability decision, not invented desktop tests or a silent pass.

`TEST.md` is an informal triage aid. Use its secret, authorization, injection, dependency and debug-exposure questions, but not its assumed finding counts, age-based CVE conclusions, blanket environment-variable advice, or sample automatic fixes. Threat analysis and evidence take precedence over template heuristics.

### Evidence and status rules

| Proof class | Minimum proof | Does not establish |
| :-- | :-- | :-- |
| **S: source** | Commit, exact path/symbol, reachable caller-to-sink analysis and assumptions | Execution, installed permissions, hardware enforcement or audit clearance. |
| **A: automated** | Exact command, tool/rules/database versions, seed/fixtures, exit code, counts including skips, sanitized output hash | Real Windows behavior when mocked; a text-pattern contract test is only a source contract. |
| **W: Windows** | Matching installed artifact hashes, OS/build, account/token/session, before/after independent OS read-back, denied and allowed attempts | Physical device behavior or effectiveness on other Windows editions. |
| **H: hardware** | W evidence plus device/driver/firmware/model, physical stimulus, observed enforcement and recovery | Other hardware, firmware or media; a VM is not equivalent. |
| **I: independent** | Authorized scope, independent assessor, methods, evidence, findings, signed retest and exact assessed build/configuration | Future versions, omitted components, or regulatory approval outside the report scope. |

**E0 is mandatory for every item:** ID, executor/reviewer, UTC start/end and clock source, commit and dirty-state manifest, package/installer/helper/driver hashes as applicable, configuration/edition, environment and account role, exact steps/tool versions, expected and observed result, positive and negative controls, skipped subcases, sanitized evidence location and SHA-256, cleanup result, finding/risk reference and retest build. Protect original evidence with encryption, access control, custody/access history and approved retention; publish only redacted summaries. Screenshots of green UI switches do not replace OS/file-I/O evidence. Never store real camera imagery, biometrics, clipboard contents or secrets merely to prove they are absent from telemetry; use synthetic markers and minimal captures.

Allowed **Status** values: `not_started` (no completed execution), `automated_pass` (specified automated assertions passed), `automated_fail` (reproducible automated assertion failure), `needs_manual_proof` (automation/source cannot close the claim), `accepted_risk` (named accountable owner, reason, impact, compensating controls, expiry and reassessment). No other status is implied. These values contain no human-pass state: retain `needs_manual_proof` for W/H/I-dependent items and record a separate signed **evidence disposition: satisfied / unsatisfied / incomplete** in the restricted run record. Gate closure requires `satisfied` evidence for every required proof class. `accepted_risk` is never verification and cannot waive the hard blockers below.

**Priority** ranks test urgency, not a discovered vulnerability. Critical = plausible SYSTEM compromise, unauthorized destruction/key disclosure or safety impact; High = material access/privacy/enforcement failure; Medium = bounded exposure or availability/control weakness; Low = limited assurance/usability gap. Findings additionally need a justified CVSS vector/version, CWE, applicable CVE and time-stamped EPSS when available. Say “not assigned/not available” when there is no CVE/EPSS; never invent one. Availability or safety impact can raise a deployment-specific priority.

**Public-release blocker: yes** means an unresolved applicable requirement prevents a positive release/deployment decision for the affected build/feature. `no` still requires triage and may become a blocker if impact rises. Exclusions need evidence that the component is absent or disabled without a bypass, owner approval, and explicit claim restrictions. Lack of private source, hardware or a provider account is an evidence gap, not an exemption.

### Safe execution prerequisites

Stage 1 runs in a disposable checkout/CI worker with synthetic fixtures, no production credentials and no mounted real Vaults. Review test side effects before execution: `cargo test --workspace`, build scripts and package hooks are not automatically harmless. Do not run `dev:reset`, installers, live Vault scripts, shred/erase actions, destructive CLI examples, firewall changes or real Fleet commands on the operator workstation. Tests requested below that do not yet have a harness are work specifications, not claims of existing automation.

For stages 2–4, agree scope, systems, accounts, time window, rate/resource limits, emergency contacts, stop conditions and restoration procedure. Use VM snapshots and dedicated sacrificial media; keep recovery keys outside the test device. Stop on real data exposure, unexpected privilege, escaped traffic, uncontrolled storage changes or operational impact. Preserve minimal evidence and escalate privately. Production critical infrastructure receives passive validation first; active/destructive testing requires its own approved environment and rules of engagement.

## 1. AI-agent autonomous and repeatable checks

Existing commands to use after reviewing their side effects: `bun x tsc --noEmit`, `bun run lint`, `bun run lint:tiers`, `bun run gen:types:check`, `bun run gen:cli-catalog:check`, targeted `bun test <existing-test-file>`, and scoped `cargo test --manifest-path src-tauri/Cargo.toml -p <crate> <filter>`. Use an isolated generated-output directory/checkout for builds. `cargo audit`, `bun audit`, `cargo deny check`, secret scanners, fuzzers and mutation tools need recorded versions and current inputs; a scanner installation or missing database is not a pass. These commands were inspected as candidates, not executed for this document.

### WC-A01 — Reachable attack-surface inventory

- **Priority:** High; **Security domain:** threat modeling; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** an undocumented native/CLI/service entry bypasses a reviewed control.
- **Affected source/component:** `src-tauri/commander-free/src/lib.rs`, `src-tauri/commander-free/src/cli.rs`, `src-tauri/commander-free/src/backend.rs`, `src-tauri/wincmd-shared/src/svc.rs`.
- **Preconditions:** frozen source and generated CLI catalog; declared release features.
- **Test method:** enumerate registrations, dispatch arms and service verbs; compare generated catalog with actual release reachability, tiers, risk, caller identity and final mutation sink; flag orphan/unclassified paths.
- **Secure expected result:** every reachable entry has an owner, trust boundary, asset, authorization and test; unknown IDs deny.
- **Required evidence:** E0 + S/A entry-to-sink matrix, generation diff and exclusions.
- **Remediation guidance:** close unclassified dispatch, correct catalog drift and add negative authorization coverage.

### WC-A02 — Renderer isolation and CSP

- **Priority:** Critical; **Security domain:** WebView/native boundary; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** embedded content invokes trusted native operations or exfiltrates data.
- **Affected source/component:** `src-tauri/commander-free/src/ipc_boundary.rs`, `src-tauri/commander-free/src/server_apps.rs`, `src-tauri/commander-free/capabilities/`, `src-tauri/commander-free/tauri.conf.json`, `src/security/desktopBoundaries.contract.test.ts`.
- **Preconditions:** release and development configuration separated.
- **Test method:** test each window/origin against native and plugin allowlists; inspect remote navigation, custom protocols, unsafe HTML, external URL opening and CSP destinations; run IPC/CSP/embedded-view contracts.
- **Secure expected result:** only intended trusted views receive authority; alert access stays narrow; external origins cannot impersonate bundled content.
- **Required evidence:** E0 + S/A origin-command matrix and negative cases; W proof remains WC-L03.
- **Remediation guidance:** constrain capabilities and native guards together; remove unnecessary origins and unsafe rendering.

### WC-A03 — Destructive authorization and CLI parity

- **Priority:** Critical; **Security domain:** authorization; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** direct IPC, CLI or generic dispatch bypasses confirmation or changes its target afterward.
- **Affected source/component:** `src-tauri/commander-free/src/authz/`, `src-tauri/commander-free/src/cli.rs`, `src-tauri/commander-free/src/selective_erase.rs`, `tools/ci/check-destructive-authz.sh`.
- **Preconditions:** synthetic targets and no destructive runtime backend.
- **Test method:** test wrong/expired/reused capabilities, changed canonical arguments, duplicate requests, internal-only IDs, denied production mutations and debug/release differences.
- **Secure expected result:** final backend consumes an action/target-bound authorization once; risk classification and a typed confirmation string alone do not grant identity or privilege.
- **Required evidence:** E0 + S/A dispatch matrix and denial tests.
- **Remediation guidance:** enforce at the final sink, atomically consume capabilities and rederive dangerous scope.

### WC-A04 — PowerShell and native argument injection

- **Priority:** Critical; **Security domain:** command execution; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** hostile parameter names, paths or environment values execute extra commands.
- **Affected source/component:** `src-tauri/commander-free/src/backend.rs`, `src-tauri/commander-free/scripts/core/`, `src-tauri/commander-free/src/package_updates/process.rs`, `tools/ci/check-ps-command-quoting.sh`.
- **Preconditions:** mocked process sink and PowerShell AST/parser harness in sandbox.
- **Test method:** trace JSON environment transport through router and module sinks; test quotes, backticks, dollar expressions, semicolons, newlines, NUL, switches and Unicode; inspect every string-built shell invocation.
- **Secure expected result:** inputs remain data; command/module names are allowlisted; each eventual shell/native sink preserves argument boundaries.
- **Required evidence:** E0 + S/A taint paths, captured argv and inert-marker assertions.
- **Remediation guidance:** use fixed executables, structured arguments and sink-specific validation; JSON serialization alone is not shell escaping.

### WC-A05 — Sidecar peer ownership

- **Priority:** Critical; **Security domain:** process/IPC authentication; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** rogue process receives a token or impersonates Pro.
- **Affected source/component:** `src-tauri/commander-free/src/sidecar.rs`, `src-tauri/commander-free/src/sidecar_process_auth.rs`, `src-tauri/wincmd-shared/src/lib.rs`.
- **Preconditions:** fake process/pipe probes and known accepted image hash.
- **Test method:** trace spawn → connected PID → independently opened image → accepted hash → token send → signed request; test missing/wrong PID, path/hash, protocol, signature and response identity.
- **Secure expected result:** secrets are withheld until peer checks succeed; no trust in self-reported image identity or secret command-line arguments.
- **Required evidence:** E0 + S/A ordering and malformed-peer tests; W substitution test required separately.
- **Remediation guidance:** bind verification to retained process/image identity and fail closed on probe failures.

### WC-A06 — Service verb and Windows caller authorization

- **Priority:** Critical; **Security domain:** local privilege escalation; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** an arbitrary user obtains privileged verbs or substitutes another account's SID.
- **Affected source/component:** `src-tauri/commander-svc/src/pipe.rs`, `src-tauri/commander-svc/src/peer_auth.rs`, `src-tauri/commander-svc/src/main.rs`, `src-tauri/wincmd-shared/src/svc.rs`.
- **Preconditions:** test identities for each capability class and group.
- **Test method:** trace live wiring, not module comments; test known/unknown verbs, read-only projections, InteractiveSession, SessionHelper and Privileged paths, token-capture failure and group-manager scope. Compare signed and bare requests.
- **Secure expected result:** OS peer identity and verb authorization remain mandatory regardless of envelope shape; self-chosen HMAC material is not Windows authentication.
- **Required evidence:** E0 + S/A verb-role matrix and caller-token provenance.
- **Remediation guidance:** deny unknown paths, retain captured token, narrow read projections and document the actual transport authentication contract.

### WC-A07 — IPC framing and resource limits

- **Priority:** High; **Security domain:** availability/protocol parsing; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** partial/oversized frames or nonreading clients exhaust service resources.
- **Affected source/component:** `src-tauri/commander-svc/src/pipe_transport.rs`, `src-tauri/commander-svc/src/pipe.rs`, `src-tauri/wincmd-shared/src/framing_tests.rs`.
- **Preconditions:** deterministic virtual clock, bounded duplex streams.
- **Test method:** test size boundaries, truncated headers/bodies, overflow, stalled reads/writes, connection/frame limits and permit recovery; verify no retry resumes a partially consumed frame.
- **Secure expected result:** rejection occurs before unbounded allocation; timeouts close transport; already-started operations are not falsely described as cancelled.
- **Required evidence:** E0 + A limits extracted from executable code, peak resource measurements and timeout assertions.
- **Remediation guidance:** bound concurrency/allocations, make partial-frame failures terminal and reconcile uncertain operations.

### WC-A08 — Datastore cryptography and DPAPI scope

- **Priority:** Critical; **Security domain:** keys/encryption; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** ciphertext swapping, key regeneration or mistaken DPAPI scope exposes or destroys data.
- **Affected source/component:** `src-tauri/commander-free/src/datastore.rs`, `src-tauri/commander-free/src/paths.rs`, `src-tauri/commander-free/src/startup_auth.rs`.
- **Preconditions:** synthetic v1/v2 records and fake protect/unprotect failures.
- **Test method:** inspect randomness, Argon2 parameters, AES-GCM nonce/tag/AAD, legacy migration, key persistence and user/machine scope; test tamper, wrong scope/key, truncation and failed atomic writes.
- **Secure expected result:** invalid records fail closed without silent destructive key rotation; machine DPAPI is never claimed to isolate users without ACL proof.
- **Required evidence:** E0 + S/A format/key-lifecycle map and corruption tests; WC-L06 for Windows isolation.
- **Remediation guidance:** authenticate context, protect key files, preserve recoverable originals and explicitly constrain legacy acceptance.

### WC-A09 — Vault policy, groups and grants

- **Priority:** Critical; **Security domain:** Vault access control; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** policy administration implicitly grants mount access, or a group change grants excess access.
- **Affected source/component:** `src-tauri/commander-svc/src/vault_access.rs`, `src-tauri/wincmd-shared/src/vault_access.rs`, `src-tauri/commander-free/src/vault_access.rs`, `src/panels/fleet/vaultFleetPolicy.ts`.
- **Preconditions:** denied/read/write users, direct and group grants, synthetic ACL adapter.
- **Test method:** exercise membership resolution, missing/deleted principals, conflicting grants, container identity, validation state, exact ACL read-back and policy rollback.
- **Secure expected result:** only explicit effective grants authorize mounting; policy-manager authority is separate; failed persistence/ACL verification never returns applied success.
- **Required evidence:** E0 + S/A grant truth table and failure transitions.
- **Remediation guidance:** centralize token-derived authorization and atomic policy/ACL state; require WC-L08/L09 proof.

### WC-A10 — Vault mount state and recovery model

- **Priority:** Critical; **Security domain:** mount lifecycle; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** stale policy, another session or a failed cleanup leaves unauthorized live access.
- **Affected source/component:** `src-tauri/commander-svc/src/vault_mount.rs`, `src-tauri/commander-svc/src/pro_broker.rs`, `src-tauri/commander-free/src/svc_client.rs`, `src-tauri/commander-free/src/vault_mount_verification.rs`.
- **Preconditions:** fake engine, journal, session and ACL failures.
- **Test method:** model apply/mount/unmount races, owner SID/session mismatch, boot cleanup, drive-letter reuse, per-entry recovery failure and timeout followed by late completion.
- **Secure expected result:** one consistent policy generation; uncertain outcomes stay uncertain; stale ownership cannot unmount another user's live mount; failed enforcement triggers safe cleanup.
- **Required evidence:** E0 + S/A state transitions, race seeds and negative receipts.
- **Remediation guidance:** serialize security transitions, persist recoverable identity and independently verify cleanup.

### WC-A11 — File identity and destructive path confinement

- **Priority:** Critical; **Security domain:** filesystem/TOCTOU; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** reparse points, hard links or reassigned drive letters redirect destructive work.
- **Affected source/component:** `src-tauri/commander-free/src/path_identity.rs`, `src-tauri/commander-free/src/context_menu_shred.rs`, `src-tauri/commander-free/src/selective_erase.rs`, `src-tauri/commander-context-shred/src/`.
- **Preconditions:** synthetic filesystem and disposable temp tree only.
- **Test method:** inspect handle-bound identity and receipt checks; test traversal, UNC/device namespaces, ADS, long paths, links, directory targets and file replacement between validation and use.
- **Secure expected result:** only the authorized object is mutated and read back; unsupported recursive shredding denies; aliases cannot widen scope.
- **Required evidence:** E0 + S/A identity/receipt tests; real NTFS races in WC-L32.
- **Remediation guidance:** use verified handles throughout and reject unsupported object types.

### WC-A12 — Native/unsafe-code review and SAST

- **Priority:** High; **Security domain:** memory and native API safety; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unsafe Windows calls corrupt memory, leak handles or retain impersonation.
- **Affected source/component:** `src-tauri/commander-svc/src/pipe.rs`, `src-tauri/commander-svc/src/encvol_driver.rs`, `src-tauri/commander-free/src/storage_probe.rs`, `src-tauri/commander-free/src/explorer_context.rs`.
- **Preconditions:** Rust/TypeScript/PowerShell rules covering real sinks.
- **Test method:** inventory `unsafe`, FFI, buffer sizes, integer conversion, token lifetime, impersonation/reversion and subprocess calls; run clippy plus a supported taint/SAST engine; manually triage unsupported analyses.
- **Secure expected result:** safe ownership and checked bounds; read-only probes cannot issue destructive device operations; no unsound path dismissed solely because Rust compiles.
- **Required evidence:** E0 + S/A reviewed unsafe-site ledger and scanner coverage limits.
- **Remediation guidance:** RAII cleanup, checked conversions, narrow handles and expert review for unsafe/native boundaries.

### WC-A13 — File/archive and search parsing

- **Priority:** High; **Security domain:** untrusted input; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** malicious PDF/Office/EPUB/archive, query or imported flow causes execution, disclosure or exhaustion.
- **Affected source/component:** `src-tauri/wincmd-search/src/extract/`, `src-tauri/wincmd-search/src/query.rs`, `src-tauri/commander-free/src/file_search.rs`, `src-tauri/commander-free/src/flow_bundle.rs`, `src-tauri/commander-free/src/settings_transfer.rs`.
- **Preconditions:** inert malformed-file corpus, extraction/time/memory quotas.
- **Test method:** test traversal entries, symlinks, nested/compression bombs, external XML entities, malformed encodings, SQL-like queries, unknown fields and imported action graphs.
- **Secure expected result:** bounded parsing, no external entity fetch or code execution, no file escape, and no implicit execution on import.
- **Required evidence:** E0 + A corpus hashes, parser coverage and resource ceilings.
- **Remediation guidance:** validate schemas and resolved destinations, limit expansion and isolate risky parsers.

### WC-A14 — Outbound destinations, proxies and SSRF

- **Priority:** High; **Security domain:** network boundaries; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** configured URL or redirect makes a privileged client contact unintended services or leak tokens.
- **Affected source/component:** `src-tauri/commander-free/src/net.rs`, `src-tauri/commander-free/src/activity_watch_http.rs`, `src-tauri/commander-free/src/settings_fleet_enrollment.rs`, `src-tauri/commander-free/src/pro_install.rs`.
- **Preconditions:** fake resolver/HTTP endpoints; no external attack traffic.
- **Test method:** test schemes, credentials, ports, encoded hosts/traversal, redirects, DNS rebinding, loopback/link-local destinations and proxy variables. Treat deliberate Fleet intranet endpoints separately from fixed update hosts.
- **Secure expected result:** each feature remains within its documented origin/namespace; no credential cross-origin redirect; ActivityWatch stays fixed loopback and no-proxy.
- **Required evidence:** E0 + S/A destination policy and refused-request captures.
- **Remediation guidance:** validate each redirect/resolution and scope exceptions per feature; disclose DoH fallback to deployment owners.

### WC-A15 — Fleet endpoint message authenticity and freshness

- **Priority:** Critical; **Security domain:** endpoint remote management; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** replayed, cross-device or modified command/policy gains desktop authority.
- **Affected source/component:** `src-tauri/fleet-proto/src/`, `src-tauri/fleet-agent-core/src/verify.rs`, `src-tauri/fleet-agent-core/src/dispatch.rs`, `src-tauri/commander-free/src/fleet_agent.rs`, `src-tauri/commander-svc/src/policy_store.rs`.
- **Preconditions:** test keys and protocol vectors; feature linkage recorded for Free versus paid transport.
- **Test method:** mutate canonical JSON/method/path, signature, audience/device, timestamps, nonce, epoch and idempotency IDs; replay before/after restart; test revoked enrollment and unknown action types.
- **Secure expected result:** only authenticated, applicable, fresh commands execute; duplicate execution and rollback deny; unknown actions cannot become arbitrary shell.
- **Required evidence:** E0 + S/A golden vectors and endpoint dispatch traces.
- **Remediation guidance:** bind all authorization context, persist replay/epoch state and fail closed on verification/storage errors.

### WC-A16 — Logging, diagnostics and data minimization

- **Priority:** High; **Security domain:** privacy/evidence; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** errors, exports or Fleet projections disclose secrets or personal content.
- **Affected source/component:** `src-tauri/commander-free/src/diagnostics.rs`, `src-tauri/commander-svc/src/diagnostics.rs`, `src-tauri/wincmd-shared/src/diagnostics.rs`, `src/lib/diagnosticSanitizer.ts`, `src-tauri/commander-free/src/log.rs`.
- **Preconditions:** synthetic sensitive markers and malformed events.
- **Test method:** inject markers at every error/export boundary; check allowlisted context, log injection, truncation, rotation, corruption and disk-full behavior; distinguish diagnostic summaries from legacy logs and productivity data.
- **Secure expected result:** approved metadata only crosses each boundary; drops/retention failures are visible; sensitive raw text is not relabeled safe.
- **Required evidence:** E0 + S/A field inventory, marker scans and negative export tests.
- **Remediation guidance:** use typed projections, redact at source and apply separate retention/access policies to each store.

### WC-A17 — Privacy Shield state and event contracts

- **Priority:** High; **Security domain:** camera/privacy; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** stopped/unavailable detection appears protected or raw camera-derived content leaves the detector.
- **Affected source/component:** `src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1`, `src-tauri/commander-free/src/backend.rs`, `src/lib/privacyShieldMode.ts`, `src/lib/fleetPrivacyShieldControl.ts`, `src/panels/privacy/PrivacyShieldCard.tsx`.
- **Preconditions:** synthetic detector events, owner-process and policy failures.
- **Test method:** inspect start/status/stop and owner lifetime, event-file validation, quotas, stale-event replay, managed-mode precedence and bounded Fleet projection; run related contracts.
- **Secure expected result:** no success inferred from process launch; unavailable state stays explicit; no frames, face embeddings or identifying camera details in aggregate events.
- **Required evidence:** E0 + S/A lifecycle and data-flow map; H privacy proof remains mandatory.
- **Remediation guidance:** validate session-bound events, reconcile failed starts/stops and minimize every output channel.

### WC-A18 — Monitor entitlement and authoritative state

- **Priority:** High; **Security domain:** monitoring/enforcement; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** forged UI entitlement or a nonauthoritative worker reports an absent protection as running.
- **Affected source/component:** `src-tauri/commander-free/src/monitoring.rs`, `src-tauri/commander-free/src/monitoring_catalog.rs`, `src-tauri/commander-free/src/argus.rs`, `src-tauri/commander-free/src/sidecar.rs`, `src/hooks/securityMonitorRearm.contract.test.ts`.
- **Preconditions:** paid/unpaid, missing agent, stale and malformed status fixtures.
- **Test method:** enumerate monitor start/stop/config/history paths; verify backend gates, authoritative session routing, bounded snapshots and distinct locked/unavailable/degraded/stale states.
- **Secure expected result:** unavailable enforcement is never reported active; a stopped collector cannot retain misleading cached health.
- **Required evidence:** E0 + S/A per-monitor state/authority matrix.
- **Remediation guidance:** query the owning runtime and expose freshness/failure rather than optimistic success.

### WC-A19 — Installer, repair and uninstall source safety

- **Priority:** Critical; **Security domain:** installation privilege; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** elevated packaging/repair writes an attacker-selected path or deletes user data.
- **Affected source/component:** `src-tauri/commander-free/nsis/hooks.nsh`, `src-tauri/commander-free/src/service_repair.rs`, `src-tauri/commander-free/src/service_repair_paths.rs`, `tools/build-tauri-release.ts`, `tools/release-packaging.contract.test.ts`.
- **Preconditions:** effective release configuration and installer-resource manifest.
- **Test method:** trace service install/stop/delete, ACL setup, elevation, path resolution, rollback, migration and recursive cleanup; review failure branches and bounded waits.
- **Secure expected result:** only approved binaries/services and owned paths are affected; no broad data deletion or writable privileged launch path; data preservation/removal choice is explicit.
- **Required evidence:** E0 + S/A lifecycle matrix and packaging contracts; WC-L01/L15 for installed proof.
- **Remediation guidance:** constrain ownership and canonical paths, verify ACLs and fail safely on partial installation.

### WC-A20 — Update and downloaded-component trust

- **Priority:** Critical; **Security domain:** artifact verification; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** attacker supplies executable bytes, integrity metadata, a downgrade or stale staged installer.
- **Affected source/component:** `src-tauri/commander-free/src/updater.rs`, `src-tauri/commander-free/src/pro_install.rs`, `src-tauri/commander-free/src/investigator_install/manifest.rs`, `src-tauri/commander-free/tauri.conf.json`.
- **Preconditions:** test signing keys and mocked downloads.
- **Test method:** mutate artifact/signature/hash/version/host, redirect and staged-version pairing; trace metadata authentication and verify-before-execute ordering separately for Free and optional downloads.
- **Secure expected result:** bad or mismatched material never executes; a hash from an untrusted source is not authenticity; downgrade policy is explicit.
- **Required evidence:** E0 + S/A trust-root map and tamper tests.
- **Remediation guidance:** authenticate metadata, constrain destinations, bind staged bytes and review rollback/key rotation.

### WC-A21 — Reproducible build and artifact composition

- **Priority:** High; **Security domain:** release supply chain; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** distributed binaries differ from reviewed source or contain unintended/private material.
- **Affected source/component:** `.github/workflows/release.yml`, `tools/build-tauri-release.ts`, `tools/encrypt.ts`, `src-tauri/commander-free/build.rs`, `rust-toolchain.toml`, `bun.lock`, `src-tauri/Cargo.lock`.
- **Preconditions:** two clean isolated builders, exact source/submodule/tool/dependency inputs and no signing secrets in untrusted jobs.
- **Test method:** build twice; compare unsigned payloads and explain timestamp/encryption/build nondeterminism; separately verify signed artifact identity, resources, frontend bundle and provenance.
- **Secure expected result:** independently explainable source-to-artifact correspondence; deterministic claims require matching bytes for the declared comparison scope.
- **Required evidence:** E0 + A build recipes, manifests, hashes and binary-difference report.
- **Remediation guidance:** pin inputs, remove unexplained nondeterminism and publish reproducibility limits instead of claiming a disabled job proves it.

### WC-A22 — Dependency CVEs and SBOM

- **Priority:** High; **Security domain:** software composition; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** vulnerable transitive dependency or separately downloaded tool escapes inventory.
- **Affected source/component:** `package.json`, `bun.lock`, `src-tauri/Cargo.lock`, `src-tauri/deny.toml`, `src-tauri/commander-free/scripts/modules/dependencies/`, `.github/workflows/invariants.yml`.
- **Preconditions:** frozen build inputs and dated advisory databases.
- **Test method:** run Cargo/Bun audits and composition scan; produce CycloneDX/SPDX SBOM for source and shipped artifacts including Python/model packages, native tools, WebView2, service, sidecar and driver dependencies.
- **Secure expected result:** every shipped/downloaded component has version, origin, hash, owner and advisory disposition; unsupported/unscanned components remain gaps.
- **Required evidence:** E0 + A SBOM, scan outputs, dependency paths and reachability assessment.
- **Remediation guidance:** update or remove vulnerable components, document bounded exceptions and rescan after dependency changes.

### WC-A23 — Licenses and public/private composition

- **Priority:** Medium; **Security domain:** licensing/provenance; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** redistribution includes incompatible, unlicensed or private content.
- **Affected source/component:** `OPEN_CORE.md`, `LICENSE`, `LICENSES/`, `THIRD_PARTY_NOTICES.md`, `src-tauri/deny.toml`, `tools/check-backend-leakage.ps1`, `tools/strings-grep-free.ps1`.
- **Preconditions:** actual release payload and SBOM from WC-A22.
- **Test method:** compare component licenses/notices/source obligations with payload; run boundary checks; review scanner exceptions and proprietary download separation.
- **Secure expected result:** complete attributable license inventory; paid implementation does not leak into public source/artifacts; string hygiene is not malware or security certification.
- **Required evidence:** E0 + A license/binary reports; legal review in WC-E06.
- **Remediation guidance:** resolve license conflicts, supply required notices/source and remove unintended private content.

### WC-A24 — Secrets in source, history and artifacts

- **Priority:** Critical; **Security domain:** credential exposure; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** exposed signing/provider/Fleet credentials enable impersonation or malicious releases.
- **Affected source/component:** `.gitleaks.toml`, `.github/workflows/`, `.env.example`, built frontend/native/installer assets and Git history.
- **Preconditions:** authorized full-history clone and redacting scanner; no secrets printed into public logs.
- **Test method:** scan tracked history and artifact contents with Gitleaks/equivalent; inspect allowlists, embedded credentials, build logs and test fixtures using fingerprints only.
- **Secure expected result:** no usable secrets; intentional public verification keys and inert fixtures are distinguished from credentials.
- **Required evidence:** E0 + A redacted reports, scanner configuration and rotation/revocation references for confirmed leaks.
- **Remediation guidance:** revoke exposed credentials first, remove unsafe distribution and narrowly justify suppressions.

### WC-A25 — CI/CD trust and effective gates

- **Priority:** Critical; **Security domain:** build authorization; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** untrusted PR, cache or workflow input reaches release credentials or bypasses security gates.
- **Affected source/component:** `.github/workflows/invariants.yml`, `.github/workflows/ai-review.yml`, `.github/workflows/prepare-release.yml`, `.github/workflows/publish-release-tag.yml`, `.github/workflows/release.yml`.
- **Preconditions:** source workflow graph; repository settings exported read-only by an authorized owner for separate proof.
- **Test method:** inspect triggers, SHA pins, permissions, shell interpolation, third-party actions, artifact/cache trust and environment approvals; distinguish failed, skipped, report-only and required jobs.
- **Secure expected result:** untrusted input cannot publish/sign; release consumes artifacts from the approved commit and required gates actually block.
- **Required evidence:** E0 + S/A workflow graph; live rules/permissions proof in WC-E04.
- **Remediation guidance:** least-privilege tokens, protected environments and explicit gate promotion; do not rely on comments or bypassable hooks.

### WC-A26 — Bounded fuzz campaigns

- **Priority:** High; **Security domain:** parser robustness; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unexpected input reaches a panic, unbounded allocation or authorization confusion.
- **Affected source/component:** `src-tauri/wincmd-shared/src/lib.rs`, `src-tauri/fleet-proto/src/lib.rs`, `src-tauri/commander-free/src/settings_transfer.rs`, `src-tauri/wincmd-search/src/extract/`.
- **Preconditions:** isolated pure/parser harnesses; disable real commands, filesystem escape and network egress.
- **Test method:** fuzz frames, canonical signing data, import schemas and file formats with fixed seeds/time/memory limits; minimize crashes and replay corpus on the assessed build.
- **Secure expected result:** bounded rejection, no sanitizer-detected defect or security invariant violation; coverage gaps remain explicit.
- **Required evidence:** E0 + A harness source/hash, seeds, duration, coverage, minimized cases and regression results.
- **Remediation guidance:** fix parser/bounds faults and retain regressions; absence of a harness is not a successful fuzz test.

### WC-A27 — Mutation testing of critical assertions

- **Priority:** Medium; **Security domain:** test effectiveness; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** no.
- **Threat or abuse case:** tests stay green after authorization, signature or bound checks are removed.
- **Affected source/component:** `src-tauri/wincmd-shared/src/`, `src-tauri/commander-free/src/authz/`, `.github/workflows/invariants.yml`.
- **Preconditions:** disposable checkout, bounded campaign; never mutate release artifacts.
- **Test method:** run the configured `cargo mutants -p wincmd-shared --no-shuffle`; add explicitly scoped campaigns for auth/receipt checks when harnessed; inspect surviving mutants individually.
- **Secure expected result:** security-significant mutations are detected; equivalent/time-limited mutants are not counted as killed.
- **Required evidence:** E0 + A mutant list, killed/survived/timeout/untestable counts and adjudication.
- **Remediation guidance:** strengthen behavior assertions at decision boundaries and turn critical survivors into targeted regression requirements.

### WC-A28 — Truthful settings and managed locks

- **Priority:** High; **Security domain:** security UX/state; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** a saved preference or optimistic patch is presented as enforced Windows protection.
- **Affected source/component:** `src-tauri/commander-free/src/settings_local_write.rs`, `src-tauri/commander-free/src/settings_epoch_write.rs`, `src-tauri/commander-svc/src/machine_settings.rs`, `src/hooks/useManagedPolicy.ts`, `src/components/shared/SettingsControlStatus.tsx`.
- **Preconditions:** pending, denied, offline, stale, unsupported and policy-locked fixtures.
- **Test method:** test UI and direct backend writes, mixed patches, rollback, stale epoch and read-back failures; map configured/requested/applied/verified states to actual probes.
- **Secure expected result:** locks enforce behind UI; failed writes do not stick visually; unknown/read-back unavailable never becomes verified.
- **Required evidence:** E0 + S/A state-transition and field-lock tests.
- **Remediation guidance:** separate desired and observed state, reject locked fields at the sink and show freshness/coverage.

### WC-A29 — Debug and autonomous-test exposure

- **Priority:** High; **Security domain:** production attack surface; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** debug/test endpoint or AI-selected action becomes a production bypass.
- **Affected source/component:** `src-tauri/commander-free/src/devtools.rs`, `src-tauri/commander-free/src/autonomous_agent_test/`, `src-tauri/commander-free/src/autonomous_test.rs`, `tools/autonomous-test-boundary.test.ts`, `src-tauri/commander-free/src/cli.rs`.
- **Preconditions:** release-feature build and inert hostile instructions in filenames/imports/results.
- **Test method:** compare debug/release registrations; test fixed action catalog, scope, authorization, dry-run and receipt handling; ensure untrusted text cannot expand agent permissions or select arbitrary shell/hosts.
- **Secure expected result:** production rejects debug authority; AI output is untrusted data and each action is independently authorized.
- **Required evidence:** E0 + S/A release reachability and policy tests.
- **Remediation guidance:** compile out debug surfaces and keep deterministic backend validation for every agent action.

### WC-A30 — Offline, concurrency and recovery invariants

- **Priority:** High; **Security domain:** resilience; **Test type:** AI-automated; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** simultaneous operations, clock rollback or partial persistence resurrect stale authority.
- **Affected source/component:** `src-tauri/commander-free/src/settings.rs`, `src-tauri/commander-free/src/session_instance.rs`, `src-tauri/fleet-agent-core/src/state.rs`, `src-tauri/commander-svc/src/policy_store.rs`, `src-tauri/commander-free/src/child_jobs.rs`.
- **Preconditions:** simulated outages, clocks, lock contention and filesystem errors.
- **Test method:** interleave saves/restarts/retries, corrupt journals, roll time backward, deny storage and interrupt workers; check replay suppression and explicit uncertain outcomes.
- **Secure expected result:** no partial security grant, silent reset, duplicate destructive work or stale green status; bounded retry and recoverable state.
- **Required evidence:** E0 + A interleaving/seed traces and recovery assertions.
- **Remediation guidance:** atomic persistence, monotonic epochs, bounded workers and explicit reconciliation after uncertainty.

## 2. Controlled local Windows, VM, disposable-account and test-device checks

Use installed **release** artifacts, not only a development launch. Minimum matrix: supported Windows client builds; clean install and upgrade; Free and separately entitled components; standard user and elevated administrator; two concurrent users; local console and RDP; online/offline; missing/stopped service and sidecar; reboot, logoff, lock and crash. Windows Server/RDS requires its own declared support assessment. Use physical hardware for camera, USB, print, Wi-Fi, TPM and storage claims. Every parameterized subcase needs its own E0 execution record.

### WC-L01 — Installed service privilege and ACLs

- **Priority:** Critical; **Security domain:** installation/LPE; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** a standard user replaces or reconfigures a SYSTEM executable.
- **Affected source/component:** `src-tauri/commander-free/nsis/hooks.nsh`, `src-tauri/commander-svc/src/main.rs`, `src-tauri/commander-free/app.manifest`; installed `WinCommanderSvc`.
- **Preconditions:** clean snapshotted VM, two disposable users, release installer.
- **Test method:** install/repair with and without elevation; collect `sc.exe qc`, `sc.exe sdshow`, effective ACLs and process tokens; attempt harmless denied writes/configuration changes as the other user.
- **Secure expected result:** app runs with intended token; only authorized administration modifies service/path; executable path is safely quoted and ancestors/payload/state cannot be substituted.
- **Required evidence:** E0 + W service/token/ACL read-back and denied-write results.
- **Remediation guidance:** repair inherited ACLs and service DACLs; separate user-writable state from privileged executable material.

### WC-L02 — Hostile named-pipe clients and squatting

- **Priority:** Critical; **Security domain:** IPC/LPE; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** same/other-session process squats a pipe, forges ownership or holds all service connections.
- **Affected source/component:** `src-tauri/commander-svc/src/pipe.rs`, `src-tauri/commander-svc/src/peer_auth.rs`, `src-tauri/commander-svc/src/pipe_transport.rs`, `src-tauri/commander-free/src/svc_client.rs`.
- **Preconditions:** local test harness, accounts with different SID/session/elevation, strict resource cap.
- **Test method:** attempt precreation, remote connection, wrong-session helper, PID exit/reuse and altered peer image; exercise partial frames and limits; compare signed/bare-request authorization without real mutations.
- **Secure expected result:** remote/squatted/untrusted peers cannot obtain authority; service recovers capacity and legitimate callers remain usable within agreed limits.
- **Required evidence:** E0 + W pipe descriptors, actual token/session captures, refusal and recovery timings.
- **Remediation guidance:** protect pipe creation, pin live peer identity and ensure bounded resources; do not substitute HMAC for OS authorization.

### WC-L03 — Direct native calls from untrusted views

- **Priority:** Critical; **Security domain:** renderer privilege; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** alert/embedded page invokes privileged commands or navigates into a trusted origin.
- **Affected source/component:** `src-tauri/commander-free/src/ipc_boundary.rs`, `src-tauri/commander-free/src/server_apps.rs`, `src-tauri/commander-free/capabilities/`.
- **Preconditions:** isolated malicious-page fixture, installed WebView2 app, synthetic data.
- **Test method:** attempt native/plugin calls, window-label spoofing, custom-protocol navigation, postMessage confusion, popup redirects and file/URL opening from each view.
- **Secure expected result:** native authority remains limited to intended views and commands; remote content cannot read settings or trigger actions; navigation failures are explicit.
- **Required evidence:** E0 + W call responses, navigation/network trace and proof no side effect occurred.
- **Remediation guidance:** enforce origin/view checks at native dispatch and tighten navigation/plugin capabilities.

### WC-L04 — Sidecar substitution and DLL search order

- **Priority:** Critical; **Security domain:** executable integrity; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** planted EXE/DLL in working directory, PATH or writable cache executes with elevated authority.
- **Affected source/component:** `src-tauri/commander-free/src/sidecar.rs`, `src-tauri/commander-free/src/sidecar_process_auth.rs`, `src-tauri/commander-svc/src/pro_broker.rs`, `src-tauri/commander-free/src/service_repair_paths.rs`.
- **Preconditions:** inert replacement binaries/DLLs, disposable VM, matched genuine sidecar.
- **Test method:** launch from user-controlled directories; alter candidate paths/PATH; swap image during handshake; inspect Process Monitor image-load and child-process traces; test old protocol and wrong hash.
- **Secure expected result:** only accepted images/libraries load; mismatches close the session before secrets; any permitted unsigned fork has explicit trust provisioning.
- **Required evidence:** E0 + W image paths/hashes, loader traces and denied substitutions.
- **Remediation guidance:** absolute protected paths, safe DLL loading and stable process/file identity verification.

### WC-L05 — Real process argument and environment handling

- **Priority:** Critical; **Security domain:** command injection; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** quoting that passes unit tests fails in actual PowerShell/native invocation.
- **Affected source/component:** `src-tauri/commander-free/src/backend.rs`, `src-tauri/commander-free/scripts/core/`, `src-tauri/commander-free/src/package_updates/process.rs`.
- **Preconditions:** disposable files and inert marker command, no destructive command IDs.
- **Test method:** exercise approved read-only routes with hostile names/values, spaces and Unicode; inspect process tree, argv/environment exposure and child cleanup; repeat under standard and elevated tokens.
- **Secure expected result:** no marker execution or expanded command scope; unexpected executable resolution denies; sensitive inputs do not leak to public logs/command lines.
- **Required evidence:** E0 + W sanitized process traces and unchanged sentinel files.
- **Remediation guidance:** correct actual sink quoting/argv, restrict executable resolution and minimize secret lifetime.

### WC-L06 — DPAPI and at-rest isolation across users

- **Priority:** Critical; **Security domain:** local secrets; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** another account decrypts readable machine-protected key material or recovery silently destroys data.
- **Affected source/component:** `src-tauri/commander-free/src/datastore.rs`, `src-tauri/commander-free/src/paths.rs`, `src-tauri/commander-free/src/startup_auth.rs`.
- **Preconditions:** synthetic records on machine/current-user install modes, two accounts, backup snapshot.
- **Test method:** attempt read/copy/decrypt from denied account; move encrypted material to another VM; test logon modes, corrupted/missing key, legacy migration and password/PIN changes.
- **Secure expected result:** access matches documented store scope; restricted material remains unreadable; corruption preserves recovery options and never silently creates a replacement identity.
- **Required evidence:** E0 + W effective ACLs, permitted/denied decrypt results and intact-backup hashes.
- **Remediation guidance:** align DPAPI scope with ACLs and owner model; surface degraded migrations and protect recovery copies.

### WC-L07 — Secure Storage local authentication and secret lifetime

- **Priority:** High; **Security domain:** Vault secrets/UI; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** PIN/decoy mode bypass, clipboard or process diagnostics expose Vault credentials.
- **Affected source/component:** `src/panels/vault/index.tsx`, `src-tauri/commander-free/src/startup_auth.rs`, `src-tauri/commander-free/src/authz/`, `src-tauri/commander-svc/src/pro_broker.rs`.
- **Preconditions:** test-only credentials and synthetic Vault; encrypted restricted dump storage if needed.
- **Test method:** test wrong/repeated PIN, direct IPC while locked/decoy, cancelled password prompt, lock/logoff and failed engine launch; inspect clipboard, argv, temporary files and permitted process diagnostics for markers.
- **Secure expected result:** sensitive actions reauthorize; secrets are not persisted/exposed unintentionally; no claim that memory is protected from an already privileged administrator.
- **Required evidence:** E0 + W denial matrix and redacted marker-presence results, not raw credentials.
- **Remediation guidance:** backend gates, bounded retries, masked entry, minimal secret copies and cleanup on every exit.

### WC-L08 — Vault ACLs and group revocation

- **Priority:** Critical; **Security domain:** Vault authorization; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** stale/nested group membership or excessive inherited ACLs preserve access after revocation.
- **Affected source/component:** `src-tauri/commander-svc/src/vault_access.rs`, `src-tauri/commander-free/src/vault_access.rs`, `src/panels/fleet/VaultAccessEditor.tsx`.
- **Preconditions:** owner, direct/group read-only/read-write, denied user and policy administrator; disposable NTFS container.
- **Test method:** save grants, independently read exact ACLs and effective membership; revoke/change group, reconnect and obtain fresh logon token; test manager without a mount grant and each unsupported principal case.
- **Secure expected result:** authorized grants match policy; policy administration does not imply mount rights; stale-token limitations and revocation timing are explicit and bounded.
- **Required evidence:** E0 + W policy generation, redacted SID/group matrix, ACL read-back and fresh-user attempts.
- **Remediation guidance:** reconcile memberships/ACLs, reject unsupported nesting and define forced dismount/reauthentication on revocation.

### WC-L09 — Real mount and file read/write enforcement

- **Priority:** Critical; **Security domain:** Vault data access; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** mount permission is mistaken for enforced file access, or another session reads/writes a mounted volume.
- **Affected source/component:** `src-tauri/commander-svc/src/vault_mount.rs`, `src-tauri/commander-svc/src/vault_access.rs`, `src-tauri/commander-free/src/vault_mount_verification.rs`, `tools/test-vault-access-live.ps1`.
- **Preconditions:** matched engine/driver, disposable accounts/volume; review live script parameters, remote access assumptions and cleanup before use.
- **Test method:** for every user/mount scope, mount, list, read, create, overwrite, rename and delete via Explorer and independent file APIs; attempt denied access through alternate paths and another session; unmount/remount and verify content hashes.
- **Secure expected result:** read-only cannot mutate; denied users cannot access content; permitted writes persist; policy and driver/filesystem enforcement agree.
- **Required evidence:** E0 + W real I/O matrix, error codes, hashes and fresh UI mount trace.
- **Remediation guidance:** enforce permissions at mount/device/filesystem layer and remove access when enforcement cannot be verified.

### WC-L10 — Hidden volumes, backups and recovery

- **Priority:** Critical; **Security domain:** encryption/recovery; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** wrong outer/hidden mode destroys data or backups expose keys/content.
- **Affected source/component:** `src-tauri/wincmd-shared/src/vault_access.rs`, `src-tauri/commander-svc/src/vault_mount.rs`, `src/panels/vault/mountAutoMode.ts`, `src/panels/vault/StegoBackupSection.tsx`.
- **Preconditions:** synthetic standard/outer+hidden containers, independently retained recovery copies.
- **Test method:** test mode selection, incorrect credentials, hidden protection and conflicting writes using supported engine modes; restore backups on a separate VM; inspect metadata/key confidentiality and rollback compatibility.
- **Secure expected result:** unsupported modes refuse; protected regions remain intact; recovery restores only authorized data and does not silently weaken access.
- **Required evidence:** E0 + W before/after content hashes, mode/receipt and restoration results.
- **Remediation guidance:** explicit mode contracts, verified backups and documented limits; do not promise forensic deniability from hidden-volume support.

### WC-L11 — Crash, session and boot mount cleanup

- **Priority:** Critical; **Security domain:** Vault lifecycle; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** crash, RDP disconnect or reboot leaves a usable unauthorized mount.
- **Affected source/component:** `src-tauri/commander-svc/src/vault_mount.rs`, `src-tauri/commander-free/src/attend_watch.rs`, `src-tauri/commander-free/src/rdp_session_watch.rs`, `src-tauri/commander-svc/src/main.rs`.
- **Preconditions:** snapshot, test files, declared lock/idle/logoff policy and open test handles.
- **Test method:** kill UI, sidecar and service separately; lock, disconnect, log off, switch user and reboot; simulate interrupted mount persistence; independently probe drive/device accessibility before and after recovery.
- **Secure expected result:** lifecycle follows the declared policy; failed/uncertain cleanup remains visible and does not allow new unsafe mounts; no cross-session cleanup of the wrong object.
- **Required evidence:** E0 + W timeline, OS device/drive state, I/O denials and recovery journal result.
- **Remediation guidance:** durable mount identity, service/session cleanup and bounded fail-closed recovery.

### WC-L12 — Concurrent Vault changes and target reassignment

- **Priority:** Critical; **Security domain:** races/TOCTOU; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** policy changes or file/drive replacement redirect a mount/unmount to another target.
- **Affected source/component:** `src-tauri/commander-svc/src/vault_access.rs`, `src-tauri/commander-svc/src/vault_mount.rs`, `src-tauri/commander-free/src/svc_client.rs`.
- **Preconditions:** two disposable volumes and controlled race harness.
- **Test method:** interleave grant removal, mount, unmount, rename/reparse replacement and letter reassignment; delay replies past client timeout and retry; inject journal-write failure.
- **Secure expected result:** operation uses one stable target/policy generation; stale retry cannot affect another mount; outcome-unknown is reconciled before further action.
- **Required evidence:** E0 + W identities, generation/request IDs, interleavings and post-operation I/O.
- **Remediation guidance:** retain handles, serialize transitions and return verified typed receipts.

### WC-L13 — Driver/device privilege boundary

- **Priority:** Critical; **Security domain:** kernel/native storage; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unauthorized device/IOCTL access or incompatible driver corrupts storage or escalates privilege.
- **Affected source/component:** `src-tauri/commander-svc/src/encvol_driver.rs`, `src-tauri/commander-svc/src/pro_broker.rs`; installed encrypted-volume driver and helper boundary.
- **Preconditions:** sacrificial test device, matched signed driver, backup and external recovery media.
- **Test method:** inspect driver signature/load state/device ACLs; attempt unauthorized opens and bounded invalid requests; test missing/mismatched driver, HVCI/Secure Boot compatibility and unload/upgrade with active mounts.
- **Secure expected result:** unauthorized control denied; no kernel crash/data corruption; incompatible states refuse safely and recovery remains possible.
- **Required evidence:** E0 + H driver hashes/signatures, device ACLs, OS events and recovery result; I code/IOCTL audit in WC-E03.
- **Remediation guidance:** constrain device access and request validation; block unsupported combinations and independently audit private driver code.

### WC-L14 — Tampered update and component download

- **Priority:** Critical; **Security domain:** updater; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** MITM/cache corruption or compromised metadata causes unverified execution.
- **Affected source/component:** `src-tauri/commander-free/src/updater.rs`, `src-tauri/commander-free/src/pro_install.rs`, `src-tauri/commander-free/src/investigator_install.rs`.
- **Preconditions:** isolated test distribution channel and signing keys; no production key use.
- **Test method:** serve bad/missing signatures, modified/truncated bytes, hash mismatch, old version, redirect, wrong host and stale staged bytes; interrupt download/install and retry.
- **Secure expected result:** untrusted bytes never launch; rollback/freshness decisions are explicit; interrupted updates retain a known recoverable state.
- **Required evidence:** E0 + W traffic, trust errors, child-process absence and post-failure installed hashes.
- **Remediation guidance:** verify all executable inputs before use and bind metadata/version/hash to the installed result.

### WC-L15 — Upgrade, uninstall and rollback preservation

- **Priority:** Critical; **Security domain:** deployment lifecycle; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** interrupted upgrade or cleanup removes Vaults, credentials, unrelated files or leaves privileged launchers.
- **Affected source/component:** `src-tauri/commander-free/nsis/hooks.nsh`, `src-tauri/commander-free/nsis/migrate-legacy-user-launches.ps1`, `src-tauri/commander-free/nsis/configure-elevated-launchers.ps1`.
- **Preconditions:** legacy/current installed VM snapshots, synthetic user data and active test mount.
- **Test method:** upgrade/uninstall/reinstall with locked files, slow service stop, pending deletion, denied ACL update and reboot; compare services/tasks/shortcuts/registry/files; rehearse approved rollback without replaying vulnerable code.
- **Secure expected result:** owned obsolete payload is removed; user data survives unless explicit deletion was selected; no dangling privileged tasks; recovery and driver disposition are documented.
- **Required evidence:** E0 + W lifecycle logs, inventories and retained-data hashes.
- **Remediation guidance:** transactional staging, ownership-aware cleanup, clear retention choice and tested rollback instructions.

### WC-L16 — TLS, proxy, DNS and pin-mode validation

- **Priority:** High; **Security domain:** network authentication; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** bad certificates, pin misconfiguration or proxy/DNS fallback changes the trusted peer.
- **Affected source/component:** `src-tauri/commander-free/src/net.rs`, `src-tauri/fleet-agent-core/src/pinning.rs`, `src-tauri/fleet-agent-core/src/config.rs`, `src-tauri/commander-free/src/license.rs`.
- **Preconditions:** test CA/endpoints, controlled proxy/DNS and declared certificate-trust modes.
- **Test method:** test untrusted/expired/wrong-name certificates, revoked certificates where supported, bad/empty pins, pin rotation, TLS downgrade, redirect and proxy failure; observe DoH fallback and blocked egress.
- **Secure expected result:** default TLS validates chain/name/time; any deliberate pin-as-trust-anchor mode has explicit equivalent identity/rotation controls and approved limitations; malformed configuration must not silently weaken intended policy.
- **Required evidence:** E0 + W per-client/mode handshake results and sanitized network capture.
- **Remediation guidance:** align verifier behavior with declared policy; reject invalid security configuration and constrain fallback routes.

### WC-L17 — Configured Fleet endpoint and command execution

- **Priority:** Critical; **Security domain:** remote-to-local authority; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** enrollment replacement, cross-device policy or forged remote action gains local privilege.
- **Affected source/component:** `src-tauri/commander-free/src/settings_fleet_enrollment.rs`, `src-tauri/commander-free/src/fleet_agent.rs`, `src-tauri/fleet-agent-core/src/transport.rs`, `src-tauri/fleet-agent-core/src/verify.rs`.
- **Preconditions:** isolated Fleet-compatible fixture, synthetic organization/device identities and disposable local target.
- **Test method:** enroll/re-enroll/revoke, change destination, tamper and replay epochs/commands, inject unknown action IDs and inspect outgoing fields; correlate request, receipt, local execution and OS read-back.
- **Secure expected result:** only the configured authorized endpoint and scoped command can affect the device; secrets are not forwarded to replacement origins; receipt is distinct from completion.
- **Required evidence:** E0 + W endpoint traffic and local audit/read-back timeline; no Fleet server-internal conclusions.
- **Remediation guidance:** reauthorize endpoint changes, invalidate old credentials and enforce typed signed commands locally.

### WC-L18 — Offline managed behavior and continuity

- **Priority:** High; **Security domain:** managed resilience; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** offline or signed-out endpoint falsely claims current policy enforcement or later replays stale destructive work.
- **Affected source/component:** `src-tauri/commander-free/src/fleet_agent.rs`, `src-tauri/commander-svc/src/fleet_transport.rs`, `src-tauri/commander-svc/src/reconciler.rs`, `src/hooks/useAutoHeal.ts`.
- **Preconditions:** enrolled test VM with known policy and recovery path.
- **Test method:** disconnect network, quit versus hide UI, log off/switch user, stop sidecar/service and reboot; reconnect after policy/credential change; measure freshness and mapped versus observational controls.
- **Secure expected result:** unavailable session-bound functions are declared; no assumption that the service owns all Fleet loops; cached policy has explicit validity and revalidation rules.
- **Required evidence:** E0 + W continuity timeline, process ownership and independent setting probes.
- **Remediation guidance:** correct health/claim boundaries, enforce stale-policy behavior and remediate required continuity before managed deployment.

### WC-L19 — Physical camera permissions and fail-closed Shield

- **Priority:** High; **Security domain:** Privacy Shield; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** camera denial, unplug or detector failure leaves an unprotected screen labeled protected.
- **Affected source/component:** `src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1`, `src-tauri/commander-free/src/backend.rs`, `src/panels/privacy/PrivacyShieldCard.tsx`.
- **Preconditions:** test camera/device, synthetic subject/scene, declared response policy.
- **Test method:** deny Windows/user/app camera access, occupy/unplug device, use shutter, lose owner process, exhaust quota, fail detector startup and stop/restart; test local/managed modes, multiple displays and RDP.
- **Secure expected result:** never silently widen camera permission; unavailable detection cannot claim protection; configured protective response occurs when achievable and failure is visible.
- **Required evidence:** E0 + H permission read-back, camera/process state, response latency and local/remote status timeline.
- **Remediation guidance:** explicit permission handling, owner-bound cleanup and truthful degraded status; restrict claims when fail-closed protection cannot be maintained.

### WC-L20 — No camera or biometric leakage

- **Priority:** Critical; **Security domain:** biometric/privacy data; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** frames, face templates/embeddings or identifying scene data persist or leave the device.
- **Affected source/component:** Privacy Shield detector reached by `src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1`, event handling in `src-tauri/commander-free/src/backend.rs`, `src-tauri/commander-free/src/fleet_agent.rs`.
- **Preconditions:** synthetic camera feed or consented test scene; restricted forensic and network capture tools.
- **Test method:** monitor writes/network across startup, normal detection, error/crash, stop, reboot and diagnostics export; inspect event schemas, temp/model caches and dump policy; separate model-download traffic from observation data.
- **Secure expected result:** frames/biometrics never enter routine logs, notifications, Fleet messages or persistent stores; unavoidable privileged memory exposure is scoped honestly.
- **Required evidence:** E0 + H data-flow/capture summary and marker scans; I detector/dependency review if implementation unavailable.
- **Remediation guidance:** eliminate content outputs, harden event files, disable content-bearing dumps and minimize transient data.

### WC-L21 — Clipboard protection and content minimization

- **Priority:** High; **Security domain:** clipboard/DLP; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** clipboard content leaks in alerts/history or protection incorrectly blocks/unblocks another user.
- **Affected source/component:** `src-tauri/commander-free/src/paste_monitor.rs`, `src-tauri/commander-free/src/local_clipboard_rules.rs`, `src-tauri/commander-free/src/safe_clip.rs`, `src-tauri/commander-svc/src/pipe.rs`.
- **Preconditions:** synthetic text/files/images and multiple disposable sessions.
- **Test method:** copy/paste sensitive markers, change formats/owners rapidly, lock/unlock, exercise clear/snooze and managed rules, stop helper and inspect local/Fleet alerts.
- **Secure expected result:** documented formats and rules enforced in correct session; no clipboard content in aggregate evidence; helper failure and reactive limitations visible.
- **Required evidence:** E0 + W paste/clear outcomes, session matrix and marker scans.
- **Remediation guidance:** bind policy to session/helper, avoid raw-content logs and distinguish warning from enforcement.

### WC-L22 — Screen-capture limits and multi-display behavior

- **Priority:** High; **Security domain:** screen privacy; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** tool detection is mistaken for universal capture prevention.
- **Affected source/component:** `src-tauri/commander-free/src/screen_privacy.rs`, `src/panels/privacy/screenCaptureProtection.contract.test.ts`; paid capture detector boundary.
- **Preconditions:** test device, multiple monitors, approved recording tools and remote session.
- **Test method:** test known/unknown capture tools, window/display capture, minimized app, multi-monitor, RDP and helper failure; use synthetic on-screen data and compare captured output with detected events.
- **Secure expected result:** claimed blocking works only where demonstrated; detection-only, unsupported capture paths and physical-camera limits are explicit.
- **Required evidence:** E0 + H scenario matrix and synthetic capture/alert correlation.
- **Remediation guidance:** correct enforcement or narrow copy; never generalize a process-name match to capture immunity.

### WC-L23 — USB, storage and HID approval

- **Priority:** High; **Security domain:** removable devices; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unknown/replugged/composite HID or USB storage bypasses a claimed block.
- **Affected source/component:** `src-tauri/commander-free/src/usb_guard.rs`, `src/lib/usbTrust.ts`, `src/lib/usbHidApproval.ts`, `src/components/shared/UsbHidApprovalDialog.tsx`.
- **Preconditions:** spare keyboard/recovery console, test USB/HID/composite devices; inert keystroke macro only.
- **Test method:** attach/replug/reboot, change identity/interface, deny/allow/revoke, test managed locks and helper failure; measure keystrokes admitted before detection and verify actual storage I/O.
- **Secure expected result:** actual enforced scope matches UI; reactive/pre-boot limitations disclosed; recovery input stays available without silent broad allow.
- **Required evidence:** E0 + H device IDs/driver versions, admission latency, I/O and consent/session results.
- **Remediation guidance:** fix policy/identity handling and narrow unsupported first-keystroke or firmware claims.

### WC-L24 — Ransomware detection and containment

- **Priority:** High; **Security domain:** ransomware/availability; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** detection is late/missing or containment terminates the wrong process.
- **Affected source/component:** `src-tauri/commander-free/src/ransomware_monitor.rs`, `src-tauri/commander-free/src/services/fs_watcher.rs`; paid attribution/response boundary.
- **Preconditions:** synthetic disposable directory and benign rate-controlled rewrite simulator; never real ransomware.
- **Test method:** exercise threshold bursts, slow writes, multiple roots, watcher overflow, benign bulk work and unavailable attribution; measure alert/containment time and changed files.
- **Secure expected result:** detection and attribution claims match observations; no unrelated process termination; degradation is visible; no promise of zero files changed.
- **Required evidence:** E0 + W workload parameters, modified-file count, process attribution and restoration proof.
- **Remediation guidance:** correct event/attribution handling, bound false positives and expose response limits.

### WC-L25 — DLP and tamper monitoring

- **Priority:** High; **Security domain:** data loss/tamper; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unapproved content transfer or disabled collector produces a false healthy state.
- **Affected source/component:** `src-tauri/commander-free/src/argus.rs`, `src-tauri/commander-free/src/monitoring.rs`, `src/panels/privacy/ArgusDlpSection.tsx`, `src/panels/privacy/ArgusTamperSection.tsx`.
- **Preconditions:** entitled test install, synthetic documents and approved transfer targets.
- **Test method:** attempt supported transfers, policy changes, collector stop/config edits, event deletion and privilege changes; distinguish local detailed evidence from outbound aggregates.
- **Secure expected result:** documented detection/blocking is demonstrated; tamper and stale collection are reported; no unsupported promise to defeat SYSTEM.
- **Required evidence:** E0 + W transfer outcomes, independent process/config probes and minimized outbound payload.
- **Remediation guidance:** close supported enforcement gaps, protect evidence and separate observation from prevention.

### WC-L26 — Decoy, canary and webhook boundaries

- **Priority:** High; **Security domain:** deception/local network services; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** forged beacon/webhook triggers harmful automation or listeners expose the endpoint broadly.
- **Affected source/component:** `src-tauri/commander-free/src/canary_tokens.rs`, `src-tauri/commander-free/src/file_monitor.rs`, `src-tauri/commander-free/src/services/webhook_server.rs`, `src-tauri/commander-free/src/flow_engine.rs`.
- **Preconditions:** isolated network/tailnet, synthetic canary documents and harmless flow target.
- **Test method:** enumerate listeners/firewall scope; open/touch decoys; send invalid signatures, replay and bounded bursts; test unavailable bind address and payload limits; inspect information embedded in generated files.
- **Secure expected result:** no unintended public/wildcard listener or command execution; authentication/replay policy is explicit; alerts are bounded and not evidence of a specific attacker identity.
- **Required evidence:** E0 + W listener/route inventory, packet trace and event/action correlation.
- **Remediation guidance:** constrain binding, authenticate/replay-protect automation and redact beacon metadata.

### WC-L27 — Print and removable-media evidence

- **Priority:** High; **Security domain:** print privacy/enforcement; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** print monitoring leaks document/printer/user names or misses claimed enforcement.
- **Affected source/component:** `src-tauri/commander-free/src/print_log.rs`, `src-tauri/commander-free/src/argus.rs`, `src/panels/privacy/PrintMonitoringSection.tsx`, `src-tauri/wincmd-shared/src/svc.rs`.
- **Preconditions:** test printer, virtual PDF printer and removable media with synthetic content.
- **Test method:** print/cancel/retry offline and online, spool failure, duplicate receipt and cross-session jobs; compare local spooler events with minimized outbound records.
- **Secure expected result:** counts/outcomes distinguish submission from printing; no document content/names in aggregate reports; unsupported enforcement remains explicit.
- **Required evidence:** E0 + H job/receipt timeline and payload-field comparison.
- **Remediation guidance:** bind receipts, limit collection and accurately expose printer/provider limits.

### WC-L28 — Wi-Fi and network protection

- **Priority:** High; **Security domain:** network/device enforcement; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** interface/route changes bypass Wi-Fi guard or kill-switch while UI remains green.
- **Affected source/component:** `src-tauri/commander-free/src/wifi_check.rs`, `src-tauri/commander-free/src/network_guard.rs`, `src-tauri/commander-free/src/vpn_kill_switch.rs`, `src-tauri/commander-free/src/firewall_audit.rs`.
- **Preconditions:** owned access points/adapters and out-of-band recovery; no interference with neighboring networks.
- **Test method:** switch known/unknown APs, rename/spoof test AP identity, lose VPN, switch Ethernet/Wi-Fi, IPv4/IPv6 and DNS/proxy routes; inspect actual packets and Windows firewall read-back.
- **Secure expected result:** intended blocked traffic is blocked across declared interfaces; uncertainty stays visible; no accidental permanent lockout.
- **Required evidence:** E0 + H route/rule/packet matrix and restoration result.
- **Remediation guidance:** cover interface transitions and IPv6, bind baseline appropriately and provide safe recovery.

### WC-L29 — RDP, redirection and remote access

- **Priority:** High; **Security domain:** remote sessions; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** remote login, clipboard/drive redirection or stale policy bypasses intended restrictions.
- **Affected source/component:** `src-tauri/commander-free/src/rdp_redirection.rs`, `src-tauri/commander-free/src/remote_sessions.rs`, `src-tauri/commander-svc/src/rdp_lock_query.rs`, `src-tauri/commander-free/src/attend_watch.rs`.
- **Preconditions:** isolated client/server VMs and console recovery; explicit Server/RDS support profile if claimed.
- **Test method:** verify registry/GPO precedence, effective listener/connectivity, session lock/idle/disconnect, redirected resources and supported remote-tool detection; test concurrent sessions and app exit.
- **Secure expected result:** UI reflects effective policy/listener state; observed sessions and enforcement remain correctly scoped; warnings do not imply remote access was blocked.
- **Required evidence:** E0 + W policy/read-back, connection and redirected-I/O outcomes.
- **Remediation guidance:** resolve policy precedence, apply in owning service/session and correct unsupported claims.

### WC-L30 — Notifications and monitor health under failure

- **Priority:** High; **Security domain:** alerting; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** dropped/stale alerts or locked-screen content leakage masks a security event.
- **Affected source/component:** `src-tauri/commander-free/src/native_notify.rs`, `src-tauri/commander-free/src/monitoring.rs`, `src/components/CustomNotificationWindow.tsx`, `src/lib/notificationStore.ts`.
- **Preconditions:** synthetic alerts across local and paid monitors.
- **Test method:** test first-start readiness, burst/coalescing, duplicate IDs, app hidden, locked/RDP session, notification suppression, renderer crash, stop/restart and delayed events.
- **Secure expected result:** bounded, current, content-minimized alerts; stale sessions cannot replay protection events; suppressed delivery and unavailable monitors are distinguishable.
- **Required evidence:** E0 + W producer-to-display timeline, counts/drops and minimized screenshots.
- **Remediation guidance:** session-bind queues, reconcile delivery health and avoid secret-bearing notifications.

### WC-L31 — Dynamic parsing and local DAST

- **Priority:** High; **Security domain:** runtime input robustness; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** malformed files, imports or local HTTP responses compromise the running desktop.
- **Affected source/component:** `src-tauri/wincmd-search/src/extract/`, `src-tauri/commander-free/src/file_search.rs`, `src-tauri/commander-free/src/activity_watch_http.rs`, `src-tauri/commander-free/src/settings_transfer.rs`.
- **Preconditions:** isolated corpus, fake loopback service and resource budgets; approved instrumented build plus release replay.
- **Test method:** feed malformed/bomb files, deeply nested JSON, Unicode names, query extremes and oversized/stalled responses through actual UI/IPC; DAST only discovered HTTP/WebView surfaces, not nonexistent Fleet internals.
- **Secure expected result:** bounded errors, responsive controls, no escaped writes/fetches or stale-content disclosure after permission removal.
- **Required evidence:** E0 + W request/corpus and resource traces, crash triage and release reproduction.
- **Remediation guidance:** enforce bounds at ingestion and processing; isolate parsers and recheck file access at use.

### WC-L32 — Secure deletion and crypto-erase truth

- **Priority:** Critical; **Security domain:** destructive operations; **Test type:** device-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** wrong target is destroyed or data remains recoverable after an “erased” claim.
- **Affected source/component:** `src-tauri/commander-free/src/selective_erase.rs`, `src-tauri/commander-free/src/context_menu_shred.rs`, `src-tauri/commander-free/src/recovery_wipe_plan.rs`, `src/lib/cryptoEraseReceipt.ts`.
- **Preconditions:** explicitly authorized sacrificial media only; retained synthetic recovery/escrow copies and power-interruption plan.
- **Test method:** test file links/reparse swaps/drive reassignment, protected targets, receipt mismatch, interrupted operation and backup/escrow recovery; verify only the approved target changed and test representative HDD/SSD/NVMe separately.
- **Secure expected result:** exact target-bound evidence; partial/unverified/escrow-retained states never report complete irrecoverability; overwrite does not promise flash-cell sanitization.
- **Required evidence:** E0 + H target identities, independent read-back/recovery attempt and typed receipt.
- **Remediation guidance:** reject ambiguous targets, qualify media limits and require verified key-copy disposition before crypto-erasure claims.

### WC-L33 — Residual data, crash reports and diagnostics

- **Priority:** High; **Security domain:** privacy/local forensics; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** temp files, crash dumps, caches, indexes or support exports expose sensitive markers.
- **Affected source/component:** `src-tauri/commander-free/src/datastore.rs`, `src-tauri/commander-free/src/diagnostics.rs`, `src-tauri/commander-free/src/log.rs`, `src-tauri/commander-free/src/file_search.rs`, `src/panels/secret/Diagnostics.contract.test.ts`.
- **Preconditions:** synthetic secrets/content, approved restricted forensic tooling and storage image.
- **Test method:** observe files/registry/WebView storage/WER/temp/pagefile exposure through use, crash, logoff, rotation, export and uninstall; inspect access from a second standard account; test disk-full and corrupt logs.
- **Secure expected result:** only declared protected residuals remain; exports contain safe metadata; retention failures are visible; OS dumps/pagefile are handled as deployment dependencies.
- **Required evidence:** E0 + W redacted marker inventory, ACLs, retention results and cleanup verification.
- **Remediation guidance:** reduce persistence, harden storage/export, configure approved dump policy and avoid unsupported deletion guarantees.

### WC-L34 — Applied status versus real Windows state

- **Priority:** High; **Security domain:** security UX; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** cached/tattooed registry values or failed writes produce misleading protected status.
- **Affected source/component:** `src/components/shared/SettingsControlStatus.tsx`, `src/hooks/useManagedPolicy.ts`, `src-tauri/commander-free/src/machine_settings.rs`, `src-tauri/commander-svc/src/machine_settings.rs`.
- **Preconditions:** controls sampled across camera, RDP, network, Vault and monitoring; known external policy conflicts.
- **Test method:** apply each selected control, deny elevation, override by GPO, stop service, change state externally and force read-back failure; compare UI with effective API/registry/behavior probes.
- **Secure expected result:** requested/saved/applied independently verified and stale states remain distinct; no lock or success badge substitutes for actual enforcement.
- **Required evidence:** E0 + W paired UI and independent read-back with timestamps.
- **Remediation guidance:** correct probe mapping, invalidation and wording; gate verification on measured state.

### WC-L35 — Resource exhaustion and race recovery

- **Priority:** High; **Security domain:** availability; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** bounded individual inputs still collectively exhaust threads, handles, disk or memory.
- **Affected source/component:** `src-tauri/commander-svc/src/pipe_transport.rs`, `src-tauri/commander-free/src/child_jobs.rs`, `src-tauri/commander-free/src/services/fs_watcher.rs`, `src-tauri/commander-free/src/monitoring.rs`.
- **Preconditions:** isolated VM, agreed load ceiling and stop thresholds.
- **Test method:** combine IPC load, search, alert bursts, slow children and log writes; measure handles/CPU/RAM/disk, cancellation/timeout and recovery after load ceases.
- **Secure expected result:** no unbounded growth or privilege bypass; important protection/control operations remain within deployment budgets; overload is visible.
- **Required evidence:** E0 + W workload parameters, resource graphs, latency and cleanup/recovery results.
- **Remediation guidance:** global budgets, backpressure, fair scheduling and owned-child cleanup; do not test overload on production.

### WC-L36 — Offline installation and emergency restoration

- **Priority:** High; **Security domain:** operational recovery; **Test type:** local-runtime; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unavailable provider/update channel prevents recovery or creates pressure to bypass signature checks.
- **Affected source/component:** `src-tauri/commander-free/src/service_repair.rs`, `src-tauri/commander-free/src/pro_install.rs`, `src-tauri/commander-free/src/license.rs`, installer/recovery artifacts.
- **Preconditions:** isolated VM, approved known-good package, offline verification material and synthetic backups.
- **Test method:** restore without network, with expired entitlement and unavailable providers; reinstall/repair correct version, recover data and validate policy/driver compatibility; measure recovery time/data loss.
- **Secure expected result:** declared offline functions remain usable; unavailable functions fail explicitly without insecure bypass; recovery meets agreed RTO/RPO.
- **Required evidence:** E0 + W offline verification and restored data/configuration hashes, timed recovery record.
- **Remediation guidance:** maintain verified recovery packages/instructions and disclose provider-dependent limits before deployment.

## 3. Manual human checks

### WC-M01 — Threat-model challenge and applicability review

- **Priority:** High; **Security domain:** risk governance; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** omitted attacker, feature or dependency escapes the checklist.
- **Affected source/component:** `SECURITY.md`, `ARCHITECTURE.md`, `FEATURES.md`, `NON-GOALS.md`, WC-A01 inventory.
- **Preconditions:** engineering, security, deployment and user representatives; frozen scope and data flows.
- **Test method:** walk each trust boundary using spoofing, tampering, repudiation, disclosure, denial and escalation scenarios; challenge administrator, compromised endpoint, supply-chain and physical-access assumptions; record every standard requirement's applicability.
- **Secure expected result:** no unexplained scope exclusions; assets/owners and compensating platform controls match the intended environment.
- **Required evidence:** E0 + signed threat/applicability matrix and dissent/residual-risk decisions.
- **Remediation guidance:** add missing tests and constrain deployment/claims until required boundaries are covered.

### WC-M02 — Security claims and edition truth

- **Priority:** High; **Security domain:** product assurance; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** users rely on prevention, signing, encryption or continuity that was only planned or source-tested.
- **Affected source/component:** `README.md`, `SECURITY.md`, `ARCHITECTURE.md`, `FEATURES.md`, `src/types/panels.ts`, desktop help/status copy.
- **Preconditions:** release candidate and evidence records, including optional Pro features.
- **Test method:** map every protection/certification claim to actual edition, reachable implementation, W/H/I evidence and limits; reconcile stale comments/docs with effective packaging; review PQC, wipe, antivirus, USB and camera claims.
- **Secure expected result:** no unsupported “verified,” “certified,” universal prevention, quantum-resistant or administrator-proof claim; detection/enforcement and public/private components are clear.
- **Required evidence:** E0 + claim-to-proof matrix and approved public wording.
- **Remediation guidance:** correct claims or complete missing evidence before advertising the capability.

### WC-M03 — Dangerous-action confirmation and keyboard safety

- **Priority:** Critical; **Security domain:** destructive UX; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** Enter, focus movement, misleading target label or stale dialog authorizes unintended destruction.
- **Affected source/component:** `src/components/shared/AppConfirmDialog.contract.test.ts`, `src/components/ShredConfirmationDialog.contract.test.ts`, `src/panels/vault/CryptoEraseConfirmDialog.tsx`, `src-tauri/commander-free/src/authz/`.
- **Preconditions:** safe dry-run adapter/disposable target; keyboard and assistive technology.
- **Test method:** inspect target/risk/recovery warning; tab through, press Enter/Escape, double-click, switch targets, reopen and change policy while dialog is open; verify focus restoration and cancel behavior.
- **Secure expected result:** deliberate target-specific confirmation; safe initial focus, no accidental submission or stale capability; backend independently rechecks scope.
- **Required evidence:** E0 + annotated synthetic screenshots, interaction sequence and backend refusal receipts.
- **Remediation guidance:** correct focus/default actions, invalidate stale confirmations and make irreversible effects explicit.

### WC-M04 — Managed locks and authority comprehension

- **Priority:** High; **Security domain:** managed UX; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** user thinks a disabled toggle protects data, or cannot tell who controls a monitored feature.
- **Affected source/component:** `src/hooks/useManagedPolicy.ts`, `src/lib/fleetNavigationAccess.ts`, `src/panels/fleet/AccessControlTab.contract.test.ts`, `src/panels/vault/index.tsx`.
- **Preconditions:** managed/unmanaged, denied/read/write and policy-manager accounts.
- **Test method:** walk settings, tray controls and Secure Storage; explain who can administer, mount, stop and read data; attempt alternative local UI routes and inspect backend outcomes.
- **Secure expected result:** authority, lock reason, freshness and escalation route are understandable; no unauthorized alternate route; a policy manager is not presented as a universal Vault reader.
- **Required evidence:** E0 + user walkthrough and role/outcome matrix.
- **Remediation guidance:** align explanations and backend locks; expose unavailable policy authority honestly.

### WC-M05 — Privacy notices and collection choices

- **Priority:** High; **Security domain:** privacy transparency; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** monitoring, watcher passthrough or remote reporting collects more than users were told.
- **Affected source/component:** `SECURITY.md`, `src-tauri/commander-free/src/activity_watch_autostart.rs`, `src/lib/activityWatch.ts`, `src/panels/privacy/PrivacyShieldCard.tsx`, `src-tauri/commander-free/src/fleet_agent.rs`.
- **Preconditions:** collected-field inventory from WC-A16/L20/L33 and managed deployment policy.
- **Test method:** compare enrollment/install/feature notices with actual fields, destinations, defaults, retention and exports; assess generic ActivityWatch watcher fields separately from bounded Argus/Shield events; test supported withdrawal/deletion routes.
- **Secure expected result:** clear local/managed distinction and lawful choices; no invented per-cycle consent or blanket “no content leaves device” promise.
- **Required evidence:** E0 + field-to-notice mapping and privacy-owner signoff.
- **Remediation guidance:** minimize collection, update notice and restrict unassessed watcher/provider configurations.

### WC-M06 — Accessible security state and recovery

- **Priority:** Medium; **Security domain:** accessibility/security UX; **Test type:** manual; **Status:** not_started; **Public-release blocker:** no.
- **Threat or abuse case:** color-only, clipped or inaccessible status prevents recognition of a failed protection.
- **Affected source/component:** `src/components/shared/SettingsControlStatus.tsx`, `src/components/shared/PinEntryDialog.test.tsx`, `src/panels/panelAccessibility.contract.test.ts`, `src/components/CustomNotificationWindow.tsx`.
- **Preconditions:** keyboard/screen reader, high contrast, 200% scaling and small display.
- **Test method:** navigate failure, denied, stale, locked and recovery states; check announcements, focus trap/restoration, labels, scroll and notification visibility without relying on color.
- **Secure expected result:** users can identify protection failure, cancel and recover without an unintended dangerous action.
- **Required evidence:** E0 + accessibility walkthrough and screenshots with synthetic content.
- **Remediation guidance:** repair semantic labels/focus/contrast; escalate to release blocker when the defect hides material risk or causes unsafe activation.

### WC-M07 — Critical-operation deployment review

- **Priority:** Critical; **Security domain:** operational safety; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** a valid security action interrupts safety-critical work or prevents emergency access.
- **Affected source/component:** `src-tauri/commander-free/src/action_steps.rs`, `src-tauri/commander-free/src/flow_engine.rs`, `src-tauri/commander-free/src/recovery_wipe_plan.rs`, network/USB/RDP/Vault controls.
- **Preconditions:** deployment owner and safety/operations representatives; exact endpoint role and recovery objectives.
- **Test method:** assess each privileged/destructive automation, default state, outage response and dependency; select disabled/restricted features, pilot ring, maintenance windows, dual control and emergency recovery route.
- **Secure expected result:** approved least-privilege configuration protects essential operations; no untested destruction/lockout on operational equipment.
- **Required evidence:** E0 + signed deployment profile, safety impact, RTO/RPO and exercise results.
- **Remediation guidance:** exclude unsafe features, isolate deployment and complete representative recovery tests before operational use.

### WC-M08 — Key custody and recovery ownership

- **Priority:** Critical; **Security domain:** cryptographic operations; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** one person/provider compromise exposes signing/recovery keys or makes data unrecoverable.
- **Affected source/component:** `src-tauri/commander-free/src/datastore.rs`, `src-tauri/commander-free/src/evidence_vault.rs`, `src-tauri/commander-free/src/f6_keystore.rs`, updater trust root and Vault recovery workflow.
- **Preconditions:** private key inventory containing references only, not key values; designated custodians.
- **Test method:** review generation, custody, access separation, backup, rotation, revocation, expiry and destruction for each key class; rehearse authorized recovery and lost-custodian scenarios.
- **Secure expected result:** recovery is possible only through approved authority; signing and data recovery roles are distinct; escrow/recovery copies are included in erase limitations.
- **Required evidence:** E0 + custody/access review and witnessed synthetic recovery exercise.
- **Remediation guidance:** narrow custody, add dual control where required and document rotation/recovery compatibility.

### WC-M09 — Secure-development and exception review

- **Priority:** High; **Security domain:** SSDF/change governance; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** scanner suppressions or unreviewed changes invalidate audit assurance.
- **Affected source/component:** `AGENTS.md`, `BESTPRACTICES.md`, `.gitleaks.toml`, `src-tauri/deny.toml`, `.github/workflows/`, release change records.
- **Preconditions:** sampled security-critical changes, training/ownership and exception records.
- **Test method:** verify independent design/code review, regression evidence, developer access protection, dependency provenance and time-bounded exceptions; classify post-audit changes by security impact and required retest.
- **Secure expected result:** accountable owners and secure-development evidence exist before formal assessment; no expired waiver or major change inherits obsolete clearance.
- **Required evidence:** E0 + SSDF PO/PS/PW/RV mapping and sampled approvals.
- **Remediation guidance:** close process gaps, rotate risky access and require relevant tests/review on changed trust paths.

### WC-M10 — Incident and recovery tabletop

- **Priority:** High; **Security domain:** incident readiness; **Test type:** manual; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** signing-key theft, Vault leakage or harmful update goes uncontained while evidence is destroyed.
- **Affected source/component:** `SECURITY.md`, updater/pro-install paths, local diagnostic/audit stores and deployment runbook.
- **Preconditions:** named incident lead, security contact, legal/privacy and operations owner; synthetic scenario.
- **Test method:** rehearse intake, triage, evidence preservation, affected-build identification, stopping distribution, credential revocation, endpoint isolation, safe restoration, notices and recurrence prevention.
- **Secure expected result:** working contacts, decision authority and timed actions; containment avoids wiping evidence or losing recovery; statutory assessment is escalated promptly.
- **Required evidence:** E0 + timestamped tabletop decisions, gaps, owners and follow-up exercise.
- **Remediation guidance:** fix missing authority/channels and rehearse provider-dependent revocation/recovery.

## 4. External penetration testing, independent audit, legal/privacy and provider checks

### WC-E01 — Independent desktop penetration test

- **Priority:** Critical; **Security domain:** independent technical assurance; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** chained local/renderer/network weaknesses evade developer tests.
- **Affected source/component:** desktop release and WC-A01 inventory, especially `src-tauri/commander-free/src/`, `src-tauri/commander-svc/src/`, local Pro interface and installed artifacts.
- **Preconditions:** written rules of engagement, qualified independent Windows testers, matching source/binaries and disposable test environments.
- **Test method:** combine unauthenticated black-box, ordinary-user grey-box and source-assisted white-box tests; chain renderer → IPC → service → file/device boundaries, update trust, parser and data-exposure attacks; independently retest fixes.
- **Secure expected result:** no unresolved exploitable Critical/High issue or unexplained material coverage gap for the approved scope.
- **Required evidence:** E0 + I signed scope, methods, findings, minimal PoCs, limitations and retest report.
- **Remediation guidance:** fix root causes, search sibling paths and repeat affected chain tests on exact release candidate.

### WC-E02 — Independent cryptography and Vault assessment

- **Priority:** Critical; **Security domain:** cryptography/access enforcement; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** correct primitives hide flawed key custody, mount authorization or recovery semantics.
- **Affected source/component:** `src-tauri/commander-free/src/datastore.rs`, `src-tauri/commander-svc/src/vault_access.rs`, `src-tauri/commander-svc/src/vault_mount.rs`, optional desktop engine/driver and recovery interfaces.
- **Preconditions:** independent cryptographic/Windows specialist, authorized implementation access and WC-L06–L13 evidence.
- **Test method:** review primitives/KDF/RNG/nonces/AAD, key lifecycle, legacy formats, DPAPI threat assumptions, effective mount/file permissions, crash recovery and erase/escrow claims; challenge protocol downgrade and algorithm migration.
- **Secure expected result:** supported claims match actual key/OS/engine boundaries; private implementation and hardware gaps are not papered over by Free wrappers.
- **Required evidence:** E0 + I cryptographic inventory, reviewed implementation versions and independent attack/recovery results.
- **Remediation guidance:** repair designs/implementations and narrow unsupported claims; no FIPS/PQC certification inference from library choice.

### WC-E03 — Driver, native helper and platform audit

- **Priority:** Critical; **Security domain:** kernel/platform assurance; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** unsafe privileged driver/helper provides escalation beyond desktop-level tests.
- **Affected source/component:** `src-tauri/commander-svc/src/encvol_driver.rs`, `src-tauri/commander-svc/src/pro_broker.rs`, encrypted-volume driver payload and shipped native helpers.
- **Preconditions:** source or justified independent binary-review access, vendor permission, sacrificial hardware and crash recovery.
- **Test method:** review device ACLs, IOCTL dispatch, caller context, buffer/length handling, memory lifetime and signing/load policy; perform controlled dynamic/fuzz validation and compatibility testing.
- **Secure expected result:** no unresolved privilege/memory-safety defect; supported OS/device combinations and vendor responsibility are explicit.
- **Required evidence:** E0 + I/H driver/helper hashes, signature chain, audit coverage and retest/crash records.
- **Remediation guidance:** obtain vendor fixes, restrict installation and block affected components until independently revalidated.

### WC-E04 — Signing, publication and independent build verification

- **Priority:** Critical; **Security domain:** release/provider trust; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** compromised publisher/runner/key distributes trusted-looking malicious binaries.
- **Affected source/component:** `.github/workflows/release.yml`, `.github/workflows/publish-release-tag.yml`, Tauri updater key, Authenticode signer, published installer/helper/driver and manifests.
- **Preconditions:** authorized read-only repository/environment settings and provider evidence; clean independent download host.
- **Test method:** verify effective branch/tag rules, required jobs, environment approval, credential custody/revocation, provenance and live published hashes; independently rebuild declared payload scope; verify Authenticode identity/timestamp separately from updater signature.
- **Secure expected result:** exact approved source/artifacts are published; signer and updater integrity are independently proven; a `.sig` file or workflow label alone proves neither Authenticode nor publication.
- **Required evidence:** E0 + I live artifact verification, provenance, settings export and key-compromise drill.
- **Remediation guidance:** harden publishing custody/rules, replace affected keys and distribute a verified recovery/update path.

### WC-E05 — CERT-In and sectoral applicability

- **Priority:** High; **Security domain:** legal/regulatory readiness; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** deployment misses incident/log obligations or treats a desktop checklist as an official certificate.
- **Affected source/component:** `SECURITY.md`, `src-tauri/commander-free/src/diagnostics.rs`, deployment audit/log/time/incident arrangements.
- **Preconditions:** qualified counsel/compliance owner and actual entity, jurisdiction, sector and deployment classification.
- **Test method:** assess current CERT-In/sectoral requirements against supplied directions/FAQ/extension and CSA-BR; verify clock traceability, reportable-incident workflow, designated contact and required logs/retention/location; document nonapplicable provider obligations.
- **Secure expected result:** signed applicability and operational compliance evidence; short local diagnostic retention is not claimed to satisfy organizational retention duties; historical extensions are not current exemptions.
- **Required evidence:** E0 + I dated legal/control matrix, log retrieval and reporting exercise.
- **Remediation guidance:** implement lawful deployment logging/reporting and obtain required qualified audit; restrict deployment until material obligations are met.

### WC-E06 — Privacy, employee monitoring and licensing review

- **Priority:** High; **Security domain:** privacy/legal; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** lawful-looking monitoring or redistribution infringes user rights or licensing obligations.
- **Affected source/component:** `SECURITY.md`, `OPEN_CORE.md`, `THIRD_PARTY_NOTICES.md`, Privacy Shield, ActivityWatch, diagnostics and outgoing Fleet data contracts.
- **Preconditions:** exact data inventory, purposes, roles, jurisdictions, retention, subprocessors and license SBOM.
- **Test method:** assess legal basis, notice/consent where required, employee/biometric protections, access/export/deletion rights, cross-border handling, privacy impact and open-source/proprietary redistribution obligations.
- **Secure expected result:** approved proportionate collection and transparent limits; no assumption that administrator enrollment substitutes for every legal requirement.
- **Required evidence:** E0 + I privacy impact/applicability review, notices, rights workflow and license disposition.
- **Remediation guidance:** remove unnecessary collection, restrict deployment/purposes and correct notices/contracts/distribution.

### WC-E07 — External desktop dependencies and provider failures

- **Priority:** High; **Security domain:** third-party assurance; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** update/licensing/DNS/mesh/model/tool provider outage or compromise defeats endpoint trust.
- **Affected source/component:** `src-tauri/commander-free/src/net.rs`, `src-tauri/commander-free/src/license.rs`, `src-tauri/commander-free/src/pro_install.rs`, dependency installer scripts and network-backed desktop features.
- **Preconditions:** vendor security/support information and separately authorized provider tests; no unsolicited production probing.
- **Test method:** assess provenance/support/EOL, secure download/signature guarantees, data received, incident notification, revocation and outage recovery; simulate failures locally and obtain vendor assurance for unavailable internals.
- **Secure expected result:** each dependency has owner, trust decision, minimized data and tested failure behavior; optional provider absence cannot silently weaken a required control.
- **Required evidence:** E0 + I supplier matrix, attestations with scope/date and local failure-test references.
- **Remediation guidance:** replace unsupported dependencies, pin/verify artifacts and establish contractual recovery/notification routes.

### WC-E08 — Critical-infrastructure deployment audit

- **Priority:** Critical; **Security domain:** independent operational assurance; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** technically sound desktop software is deployed in an unsafe operational architecture.
- **Affected source/component:** exact WinCommander endpoint build/configuration, Windows platform controls and desktop-facing network boundaries; no Fleet server/console internals.
- **Preconditions:** asset owner risk classification, sector requirements, representative pilot and independent qualified assessor (CERT-In empaneled when required).
- **Test method:** evaluate CSA-BR high-risk markers where applicable, least privilege, segmentation/egress, physical access, backup, privileged change control, monitoring, staffing, incident and recovery exercises; passively verify production equivalence.
- **Secure expected result:** asset owner accepts a version/configuration-specific deployment with required controls and demonstrated continuity; no blanket product certification.
- **Required evidence:** E0 + I deployment report, configuration equivalence, residual risk and safety/operations authorization.
- **Remediation guidance:** close deployment deficiencies or restrict features/locations; active operational tests require separate scope approval.

### WC-E09 — Disclosure and incident-contact verification

- **Priority:** High; **Security domain:** vulnerability response; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** reports are lost, published unsafely or unsupported builds remain exposed.
- **Affected source/component:** `SECURITY.md`, supported-version policy, release advisories and security intake process.
- **Preconditions:** security owner authorizes a synthetic intake exercise; no real vulnerability published.
- **Test method:** verify intake ownership/backup, secure evidence transfer, acknowledgement/triage targets, coordinated advisory/CVE decision, researcher credit and supported-build patch delivery; test escalation on missed targets.
- **Secure expected result:** reports reach accountable responders; private evidence stays private; affected users receive actionable version/mitigation information.
- **Required evidence:** E0 + I witnessed intake/response timeline and advisory rehearsal.
- **Remediation guidance:** fix routing/staffing, publish accurate support windows and maintain confidential coordination.

### WC-E10 — Independent retest and bounded assurance statement

- **Priority:** Critical; **Security domain:** audit closure; **Test type:** external-audit; **Status:** not_started; **Public-release blocker:** yes.
- **Threat or abuse case:** a scan report or staging-only result is reused as unrestricted Safe-to-Deploy clearance.
- **Affected source/component:** assessed release artifacts, final VAPT evidence register, deployment configuration and change manifest.
- **Preconditions:** initial findings remediated; independent reviewer; exact candidate frozen.
- **Test method:** reproduce original findings, test root-cause variants, verify fix/negative-control evidence and deployment equivalence; review every incomplete/excluded item and all post-audit changes.
- **Secure expected result:** signed opinion names hashes, versions, environment, scope, dates, limitations, residual risks and retest result; staging-only scope remains explicit.
- **Required evidence:** E0 + I final report/evidence appendix signed by tester, independent reviewer and authorized audit leadership as applicable.
- **Remediation guidance:** reopen failed/incomplete items and re-audit material changes; never issue an unqualified clearance from this checklist.

## Public-release and deployment gate

**Decision from this document: gate not assessed / not satisfied.** This means acceptance evidence has not been assembled here; it does not assert that every listed threat is a present vulnerability. No item has been executed as a VAPT result in this authoring exercise.

| Decision | Required closure |
| :-- | :-- |
| Public source/distribution | No exposed usable credentials/private data or unlawful payload; accurate threat/support/disclosure statement; reproducible instructions and provenance appropriate to the published build. |
| Public desktop release / Safe-to-Deploy-style opinion | All applicable `yes` items have required A/W/H/I evidence disposition `satisfied`; no unresolved exploitable Critical/High finding; exact installer, updater, service, optional payload and supported OS matrix evaluated; accountable release owner signs. |
| Feature-limited release | A blocked optional feature is demonstrably absent or disabled without bypass, its claims removed and its exclusion approved. Merely hiding a button is insufficient. |
| Critical-infrastructure deployment | Public-release requirements plus WC-M07/WC-E08, high-risk applicability, independent audit, representative hardware, least-privilege/egress profile, incident readiness, backup/recovery exercises, pilot and asset-owner authorization. |

Hard blockers cannot be waived into a positive verdict: exploitable unauthorized SYSTEM/kernel execution; unauthorized Vault read/write or key disclosure; unverified executable update acceptance; unintended destructive targeting; camera/biometric leakage contrary to the stated boundary; unusable essential recovery; or a material false security claim. Medium/Low residual risks may be accepted only by the accountable owner with compensating controls, expiry and environment-specific justification. Scanner cleanliness alone closes none of these gates.

“Safe-to-Host” is borrowed here as an assessment style: WinCommander is installed on Windows endpoints, not a hosted web service. Only a qualified assessor and the applicable authority can issue the required formal statement. The opinion must bind the exact build, component set, deployment configuration, environment, test period and limitations. No self-issued CERT-In, OWASP, FIPS, critical-infrastructure or post-quantum certification is implied.

### Top unresolved evidence blockers to evaluate first

These are priorities for evidence collection, not a public exploit ledger.

1. **Privileged install and service boundary:** WC-A05/A06/A19/A25, WC-L01–L05 and WC-E03/E04. Effective machine-wide packaging must be reconciled with older public per-user/optional-service descriptions; independently prove installer/service ACLs, caller identity and artifact signing.
2. **Vault authorization through real I/O:** WC-A08–A11, WC-L06–L13 and WC-E02. A saved policy, ACL string, successful mount or unit test does not establish cross-user read/write enforcement and recovery.
3. **Release integrity and reproducibility:** WC-A20–A25, WC-L14/L15 and WC-E04. The inspected invariants workflow has report-only `cargo-deny`, coverage and mutation jobs; the reproducible-build example is commented out. Obtain actual results and effective gate settings rather than assuming these are blocking checks.
4. **Camera/privacy and monitor truth:** WC-A16–A18/A28, WC-L19–L30/L33/L34 and WC-M02/M05. Require physical-device and traffic/storage evidence for privacy and enforcement claims; public wrappers cannot establish paid implementation behavior.
5. **Managed continuity, independent audit and deployment readiness:** WC-L17/L18/L36, WC-M07–M10 and WC-E01–E10. Declared service Fleet loops do not prove session-independent management; missing external/legal/hardware proof remains open.

Source observations used to scope tests: `pipe_transport.rs` currently defines 32 connection slots, a 4 KiB Hello cap, five-second Hello/write deadlines, 30-second frame deadline and 128 frames per connection; the shared payload limit is separately defined. Do not copy older public limit numbers into test expectations. `pipe.rs` actually calls the session-helper gate despite stale introductory comments in `peer_auth.rs`; it also accepts bare Request envelopes, so the OS identity/verb check is the critical authority boundary. `datastore.rs` writes scope-authenticated v2 envelopes and still reads legacy v1; DPAPI scope differs by store/install mode. These are source observations, not runtime passes or independently confirmed vulnerabilities.

## Independent penetration-test scope and audit expectations

Commission the desktop engagement against WC-E01–E04/E10 and all applicable stage-2 cases. Supply source at the assessed commit, actual installer/update manifests and every shipped/downloaded executable hash, SBOM, data-flow/threat model, test accounts, representative Windows images/hardware and permission to review matching private desktop components. Do not supply production keys. List which observations are developer-provided and which the assessor reproduced independently.

Include external/unauthenticated input, same-user malware, another standard user, elevated administrator abuse, console/RDP/multiple sessions, network interception, compromised update inputs and supply-chain scenarios. Exercise local pipes, service verbs, WebViews, native/PowerShell execution, process/DLL substitution, Vault/file/driver access, local listeners, parsing, monitors, privacy and recovery. Fleet fixtures represent only the desktop protocol boundary; server/console internals and other products require separate engagements.

Agree named asset/environment scope, test window, rate/resource limits, permitted exploit depth, excluded production/provider assets, emergency contacts, stop rules, backup/restore, handling of accidental data, temporary credentials, encrypted evidence delivery and revocation/cleanup. Prefer passive production-equivalence checks for critical operations; DoS, social engineering, kernel stress and destructive storage testing require separately explicit written scope. Validate tools in the lab before use.

Require discovery plus manual/source-assisted validation, reproducible findings and false-positive adjudication, not only OWASP Top 10 or scanner output. Report business/safety impact, CVSS version/vector, CWE, CVE/EPSS where applicable, root cause, affected versions, remediation owner/date and negative retest. Include evidence of compliant as well as noncompliant controls, tools/versions, sampling, skipped paths, working notes, configuration/build hashes and constraints. Provide an executive summary, technical report and evidence appendix. Under the supplied CERT-In audit model, use independent maker/checker review and appropriate auditor/reviewer/head signatures; verify current empanelment/competence where required.

Final closure requires retesting the exact fixed candidate and identifying any staging-versus-production gap. Freeze the assessed artifact/configuration or perform documented impact analysis and retest after change. Use periodic review and major-change triggers; the supplied 2025 policy describes at least annual audits, with higher frequency where risk/regulators require. Treat that as an input to the deployment's current legal/audit plan, not universal certification of a product.

## Incident-response readiness

Use WC-M10/E05/E07/E09 as the evidence-bearing tests for the following release conditions:

- Named incident lead and backup, security engineering, release/signing custodian, legal/privacy, operations and provider contacts; a private channel that still works if the update/licensing service is unavailable.
- Triage by affected build/feature/OS/device; preserve clock-correct, integrity-protected evidence and relevant logs before uninstall, cleanup, wipe or key rotation destroys it. Record custody and collection limits.
- Scenarios for stolen release key, malicious update/dependency, service LPE, Vault access failure, camera/content leakage, harmful remote command and unavailable management. Prepare revocation, stop-distribution, trusted notification and clean recovery paths.
- Tested known-good offline recovery artifacts, backups and required keys; defined RTO/RPO and rollback restrictions; verify restored data and control state before reconnecting.
- Legal owner determines reportability, jurisdiction and deadlines using current rules. The supplied 2022 CERT-In directions discuss specified incident reporting within six hours of notice, a designated contact, time synchronization and 180-day ICT logs for covered entities; the FAQ clarifies reporting incomplete information initially and other applicability details. Do not equate the app's seven-day diagnostic retention with organizational log compliance or apply VPN-provider customer-retention requirements merely because the desktop uses a VPN. Review both direction and FAQ wording on log location with counsel. The 2022 extension is historical.
- Post-incident root-cause review, similar-defect search, patch validation, user communication, recovery verification and measured improvement under SSDF RV and CSF Respond/Recover.

Official reference discovery was checked on 2026-09-22: [CERT-In 2025 audit-policy publication](https://www.cert-in.org.in/s2cMainServlet?pageid=GUIDLNVIEW02&refcode=CISG-2025-02) and [CERT-In directions FAQ](https://www.cert-in.org.in/PDF/FAQs_on_CyberSecurityDirections_May2022.pdf). This lookup confirms the reference sources; WC-E05 must still establish current entity/sector-specific obligations and amendments.

## Vulnerability-disclosure checklist

These operational conditions are tested under WC-E09 and WC-M10; they do not create duplicate test IDs.

- Publish and maintain `SECURITY.md` with private reporting at **security@servalabs.com**, supported versions, safe-harbor boundaries and requested reproduction/build information.
- Verify the published response targets: acknowledgement within 72 hours, triage within seven days and critical-fix target of 30 days where feasible. These are policy targets, not evidence of actual response performance or replacements for incident-reporting deadlines.
- Provide secure handling for PoCs, logs and researcher identity; never require a public issue containing exploit details or user data. Verify the secure transfer method before sending secrets or sensitive evidence.
- Track intake, duplicates, severity, affected versions, owner, mitigation, fix, independent retest and disclosure decision; coordinate CVE/advisory and researcher credit where appropriate.
- Notify affected users with precise supported versions and verified mitigation/update instructions; distinguish temporary mitigations from full closure and revoke compromised artifacts/keys when necessary.
- Revoke auditor accounts/tokens and remove test agents/listeners/artifacts; retain encrypted reports only for the approved period, dispose of other evidence according to contract/regulation and document disposal limits truthfully.

## Traceability: reviewed security documents and repository areas

### Supplied security-library inventory

Source directory: `D:\GitHub\Prompts\Security\docs`. Recursive inventory found **18 files: 17 PDFs and `TEST.md`; no subfolders**. Five PDF pairs are byte-identical, leaving 12 unique PDF contents. Text was extracted across all PDF pages, with applicability/requirements review of the normative sections and section-targeted reading of the large testing catalogues. Image-only cover/license pages and the scanned MSME annex were inspected separately. Copies were matched by full SHA-256 before deduplicating content. No numbered gaps in filenames were treated as missing requirements or invented documents.

The page ranges below are **PDF page numbers**, including front matter. “Reviewed” means used as an analysis/reference source, not executed as compliance testing. External documents referenced inside these publications are not represented as additionally reviewed unless listed here; obtain current normative copies during the independent audit.

| Ref | Every supplied filename | Edition/pages and extracted requirements | Checklist trace |
| :-- | :-- | :-- | :-- |
| D01 | `01_CERT-In_Comprehensive_Cyber_Security_Audit_Policy_Guidelines_2025.pdf` | v1.0, 25 July 2025, 69 pages; sections 7–18: independence, competence, risk-based scope, standards beyond Top 10, secure-development prerequisite, agreements, confidentiality, execution, evidence, reporting and retest. | A01/A25; M01/M09/M10; E01/E05/E08–E10; gate/audit/evidence sections. |
| D02 | `Comprehensive_Cyber_Security_Audit_Policy_Guidelines.pdf` | Exact D01 duplicate; same reviewed content, including data handling, annual/major-change audit triggers and signed reports. | Same as D01. |
| D03 | `02_CERT-In_Secure_Application_Guidelines_2024.pdf` | January 2024 v1.1, 15 pages; PDF pp.5–15, four phases: threat modeling, secure coding, crypto/memory/files, SAST/DAST, SBOM, least privilege, audit and secure signed deployment/updates. | A01–A30; L01–L36; M09; E01/E04; release gate. |
| D04 | `Application_Security_Guidelines.pdf` | Exact D03 duplicate. Technology-specific advice adapted to Rust/PowerShell/Tauri; parameterized local data access, not an invented stored-procedure requirement. | Same as D03. |
| D05 | `03_CERT-In_Cyber_Security_Audit_Baseline_Requirements.pdf` | NSCS-46-16 October 2020, 19 pages; PDF pp.6–17, risk profiles and `csm`, `pro`, `det`, `res`, `rec`, `imp` markers; high-risk profile requires all markers in that baseline. | M01/M07–M10; E05/E08; privilege, remote access, removable media, wireless, data, continuity and incident tests. |
| D06 | `CyberSecurityAuditbaseline.pdf` | Exact D05 duplicate; organizational applicability retained separately from desktop control proof. | Same as D05. |
| D07 | `05_CERT-In_Directions_70B_28-04-2022.pdf` | 28 April 2022, eight pages; directions (i)–(vi), incident categories and contact annexes: time, reporting, assistance/PoC, logs and provider-specific records. | A16; L33; M10; E05; incident readiness. |
| D08 | `CERT-In_Directions_70B_28.04.2022.pdf` | Exact D07 duplicate; legal applicability requires current counsel review. | Same as D07. |
| D09 | `06_CERT-In_FAQ_CyberSecurity_Directions_2022.pdf` | May 2022, 28 pages; scope/reporting/privacy/log/time questions, especially Q24/Q30/Q34–43, and incident illustrations. FAQ distinguishes enterprise VPN use from covered VPN-provider services. | M05/M10; E05/E06; incident/logging plan. |
| D10 | `FAQs_on_CyberSecurityDirections_May2022.pdf` | Exact D09 duplicate; FAQ interpretation is not substituted for the directions or current legal advice. | Same as D09. |
| D11 | `CERT-In_directions_extension_MSMEs_and_validation_27.06.2022.pdf` | Four pages, including scanned MSME notification annex; limited enforcement extension to 25 September 2022. Historical scope/timing only. | E05; no current exemption inferred. |
| D12 | `08_NIST_SP_800-115_Technical_Security_Testing.pdf` | September 2008, 80 pages; sections 2–8 and appendices B–D: review, discovery, validation, test planning/execution, evidence/data handling, mitigation, rules of engagement and remote access. Legacy tool/SSL examples are not modern configuration recommendations. | Stage ordering, E0, A12/A26, L16/L26/L29/L31/L35, E01/E10. |
| D13 | `09_NIST_SP_800-218_SSDF_1.1.pdf` | February 2022, 36 pages; PO.1–PO.5, PS.1–PS.3, PW.1/PW.2/PW.4–PW.9 and RV.1–RV.3 tasks; secure environments, provenance, design, code/testing, secure defaults and vulnerability response. | A01/A12/A19–A29; M08–M10; E04/E07/E09. |
| D14 | `10_NIST_CSF_2.0.pdf` | February 2024, 32 pages; sections 1–5, Core/Profile/Tier appendices: Govern, Identify, Protect, Detect, Respond, Recover; supply chain and organizational risk communication. Not a prescriptive technical certification checklist. | Threat model; M01/M07/M09/M10; E07/E08; incident/release decisions. |
| D15 | `11_OWASP_ASVS_5.0.0.pdf` | May 2025, 120 pages; verification/reporting scope and V1–V16 applicability review. Primary desktop mappings: V1/2 input/business logic, V3 WebView, V5 files, V6/7/8 auth/session/authorization, V9 tokens, V11 crypto, V12 TLS, V13 configuration, V14 data, V15 architecture, V16 logs/errors. V4/V10 only for actual client/API/federated boundaries, not hypothetical server components. | A02–A20/A22/A28/A30; L02–L35; M02–M06; E02/E10. |
| D16 | `12_OWASP_WSTG_4.2.pdf` | v4.2, 465 pages; framework/reporting and all testing-family applicability reviewed. Adapt INFO mapping, CONF configuration, IDNT/ATHN/ATHZ/SESS identity/authorization/session, INPV injection/SSRF, ERRH errors, CRYP, BUSL workflows and CLNT WebView/storage/messaging methods. APIT GraphQL, Flash, web-server-specific and unrelated language/server tests are outside the observed desktop surface unless discovery changes scope. | A01–A04/A11/A13–A16; L02–L05/L14/L16/L17/L26/L31; E01/E10. |
| D17 | `13_OWASP_MASVS_latest.pdf` | Actual v2.1.0, 18 January 2024, 35 pages; STORAGE, CRYPTO, AUTH, NETWORK, PLATFORM, CODE, RESILIENCE, PRIVACY and manual-assessment limits. Mobile-specific and anti-static-analysis controls are not imposed wholesale on open-source Windows. | A02/A05/A08/A16/A20; L06/L07/L16/L19–L23/L33; M05; E02/E06. |
| D18 | `TEST.md` | Informal “Vibecoder Security Review,” 26,359 bytes: secrets, auth/data access, debug exposure, files, dependencies, hygiene/injection and false-positive triage. Supplemental only; not an audit standard or instructions to mutate code. | A03/A04/A13/A16/A22/A24/A29; explicit methodological limits above. |

### Document identity register

Full SHA-256 values allow another assessor to retrieve the same supplied editions; duplicate refs share one identity. These identify source documents, not evidence of product security.

| Refs | SHA-256 |
| :-- | :-- |
| D01/D02 | `ad2b9b38d5af8e1c55d450f7122059d65546bf7be904ba5ace59cbd1fcc3bc09` |
| D03/D04 | `89acd6b83672443a9e094a95e2fb5ba8fd60634e3c85f21590baf390304a308d` |
| D05/D06 | `282558b90ecceffd0fd975c82a6850198de74d345b067fc132efbb4e838f0898` |
| D07/D08 | `202c2f3953d792dcfb3ecb3634fc82ab75437ee596de89b8d59a211e8e42431f` |
| D09/D10 | `7c4ae9eab453db32a5feece63987f7606373d689755b5b2869bb367b5682d6af` |
| D11 | `f0a2805f2a3bd0745560d417ffb6d41bf2cfe9920a5fd8a45b806ff10d61979f` |
| D12 | `58e5ed41e5c8ca34ce14fd80b70f118f2c6d613d1647bc307edca30bcc45f063` |
| D13 | `617746e553a9e2da49bfbd4eef0dfc3094758a39b869314e4173ac36605cde22` |
| D14 | `3c31f46fee98cac0c4323453e5109291a213b4de7fef8c058af9bf67f717433c` |
| D15 | `a2fa1bbe38f12cac86d3a0f0023327e9772201f65a7f741f282f5e785d268fc5` |
| D16 | `0f1cd587e62539c9badd23699a99a351bf3468c3601980efa0a3b4a5b6ab6f6e` |
| D17 | `93d48b6bf8595ad097c82a16faa03229922cbc88e3dc7e8db4fbe45ffc247d21` |
| D18 | `a1a1e3eb0fee76e2179490da7ac4a830900935c13370589ac68fdf36fb1c52b0` |

### Repository review map and limits

The following areas were inventoried and inspected through relevant declarations, configuration, tests and caller/sink sections to ground the checklist. This is targeted security scoping, not a claim of line-by-line audit of every listed directory. Exact affected files/components are also attached to every test above. Source/tests were not executed as acceptance evidence.

| Reviewed area | Source basis and test coverage |
| :-- | :-- |
| Public scope and guidance | `AGENTS.md`, `BESTPRACTICES.md`, `SECURITY.md`, `ARCHITECTURE.md`, `FEATURES.md`, `OPEN_CORE.md`, `NON-GOALS.md`, `docs/README.md`, `docs/cli.md`; A01, M01/M02/M09. Older descriptions require comparison with current code. |
| Native surface and privilege | `commander-free/src/lib.rs`, `backend.rs`, `cli.rs`, `authz/`, `ipc_boundary.rs`, manifests and capabilities under `src-tauri/`; A01–A06, L01–L05, M03. |
| Service and sidecar | `src-tauri/commander-svc/src/{main,pipe,pipe_transport,peer_auth,pro_broker}.rs`; `src-tauri/commander-free/src/{sidecar,sidecar_process_auth,svc_client}.rs`; shared framing/verb contracts; A05–A07, L01–L04. |
| Vault and Secure Storage | Public `vault_access.rs`/`vault_mount.rs`/`vault_mount_verification.rs` paths named above, `src-tauri/wincmd-shared/src/vault_access.rs`, `src/panels/vault/`, `src/panels/fleet/` Vault client controls and live-test script; A09/A10, L07–L13. Fleet UI references here are only desktop controls. |
| Local key/data protection | `src-tauri/commander-free/src/{datastore,paths,startup_auth}.rs`, declared evidence/recovery entry points; A08/A16, L06/L07/L33, M08/E02. No private engine cryptographic audit performed. |
| File operations and parsing | `src-tauri/commander-free/src/{path_identity,context_menu_shred,selective_erase,file_search}.rs`, `src-tauri/commander-context-shred/`, inventory of `src-tauri/wincmd-search/src/extract/`; A11–A13/A26, L31/L32. Individual extractor implementations need the specified deeper review. |
| Network and Fleet client | `src-tauri/commander-free/src/{net,activity_watch_http,settings_fleet_enrollment,fleet_agent}.rs`, `src-tauri/fleet-agent-core/src/{config,pinning,verify,transport}.rs`, `src-tauri/fleet-proto/` and service policy/continuity declarations; A14/A15/A30, L16–L18. Feature linkage and paid runtime remain separate proof. |
| Privacy Shield | Public PowerShell module, `backend.rs` lifecycle/event seams, `src/lib/privacyShieldMode.ts`, `src/lib/fleetPrivacyShieldControl.ts`, `src/panels/privacy/PrivacyShieldCard.tsx`; A17, L19/L20, M05. Detector binary/dependencies and physical-camera claims require H/I proof. |
| Monitors and alerts | `src-tauri/commander-free/src/{monitoring,monitoring_catalog,ransomware_monitor,usb_guard,argus,screen_privacy,print_log,remote_sessions,wifi_check,canary_tokens}.rs`, related UI/test inventory and webhook server; A18, L21–L30. Paid wrappers expose interfaces, not independently reviewed implementations. |
| Diagnostics and truthful status | `src-tauri/commander-free/src/diagnostics.rs`, shared diagnostics contracts, `src/lib/diagnosticSanitizer.ts`, settings/managed-policy/test inventory; A16/A28, L30/L33/L34, M02/M04/M06. |
| Packaging, update and release | `src-tauri/commander-free/tauri.conf.json`, NSIS hooks, updater/pro installer paths, `tools/build-tauri-release.ts`, release/invariants workflows, `package.json`, lockfile/tool/license inventories; A19–A25/A27, L14/L15/L36, E04/E07. No live release/signature/provider clearance inferred. |

Not reviewed as product internals: sibling private Pro source, Fleet server/console, licensing/update-provider infrastructure, kernel/firmware source, customer deployment configuration, production credentials/data, and external acceptance records. Their relevant **desktop-facing** proof obligations remain explicitly assigned above. Documentation structure/path validation performed for this file is an editorial check, not a VAPT pass.
