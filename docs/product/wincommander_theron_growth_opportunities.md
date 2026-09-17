# Verified Unbuilt Opportunity Backlog: WinCommander + Theron

*Re-audited 18 August 2026 against the checked-out product repositories. This document deliberately excludes work that is already shipped.*

## The correction

The earlier report incorrectly treated several real capabilities as opportunities to build. They are already present and should be used as foundations:

| Already implemented | Evidence |
|---|---|
| Self-hosted Fleet server and agent: enrolment, per-device HMAC check-ins, TLS pinning, signed command dispatch, groups, admin roles, drift, compliance, reporting, remote file operations and fleet search | [WinCommander Pro features](wincommander-pro/FEATURES.md), [Fleet API](wincommander-pro/fleet-server/API.md) |
| Theron as Fleet's existing console/BFF: device list/detail, command history, command trigger, Fleet chat tools | [Theron Fleet integration](theron/FEATURES.md) |
| Desired-state policy, setting hashes and drift detection | [WinCommander features](wincommander/FEATURES.md) |
| Evidence vault with hash chain/signature, plus Theron's §63 certificate and custody ledger | [WinCommander Pro features](wincommander-pro/FEATURES.md), [Theron features](theron/FEATURES.md) |
| Vendor due diligence, claims/loan intake ring analysis, Signals board, map/time views, tasks, alerts and source-grounded briefs | [Theron features](theron/FEATURES.md) |

So the real opportunity is not “make Fleet,” “make policy drift,” “make evidence,” or “make an alert dashboard.” It is to **finish, prove, and connect the important seams that remain missing**.

## How to read the list

- **Finish** = the code exists but is incomplete, unproven in a real deployment, or missing an essential control.
- **Extend** = a strong shipped base exists, but a meaningful workflow is absent.
- **New vertical** = a genuinely new product area, especially relevant to Gujarat Sentinel.
- Scores are directional: 5 means highest.

## Priority shortlist — genuinely unbuilt work

| Rank | Opportunity | Kind | Home | Impact | Feasibility | Why it is genuinely not done |
|---:|---|---|---|---:|---:|---|
| 1 | Fleet → Theron signal-to-case bridge | Extend | Both | 5 | 4 | Theron can view/control Fleet, but Fleet drift/alerts/evidence are not yet written into Theron’s Event, Case, Task or Operation models. |
| 2 | Fleet managed-investigation release proof | Finish | WinCommander Pro | 5 | 3 | Wazuh/Velociraptor integration is code-complete but lacks a release-proven stack and a 20-device end-to-end pilot. |
| 3 | Real endpoint-security snapshot delivery | Finish | WinCommander Pro | 5 | 4 | The real-device osquery snapshot currently fails because its signed-tool manifest returns HTTP 404. |
| 4 | Sentinel camera/IoT registry and vendor adapters | New vertical | Theron + edge adapters | 5 | 3 | Neither product has a camera/VMS/ONVIF asset model, adapter layer or maintenance workflow. |
| 5 | Federated CCTV/IoT event contract and correlation | New vertical | Theron + Fleet edge | 5 | 3 | Theron correlates its own signals, but it has no VMS/ANPR/sensor event federation contract. |
| 6 | Stable intake rescreening and analyst workbench | Finish | Theron | 4 | 5 | Existing intake screening loses an analyst’s review status on re-run and has no record editing, UI or chat workflow. |
| 7 | Scheduled vendor rescreening with alert delivery | Finish | Theron | 4 | 5 | Screening exists on demand; the due loop and email/Telegram delivery path do not. |
| 8 | Tamper-evident complete evidence bundle | Finish | Theron + WinCommander | 5 | 4 | Certificate/vault primitives exist, but Theron’s planned sealed bundle does not yet package cited artifacts, hashes, custody-chain segment and offline verification instructions together. |
| 9 | Fleet operations hardening for real organisations | Finish | WinCommander Pro | 4 | 4 | Organisation provisioning, alert-channel UI, secure SMTP, search pagination, and retention jobs are still absent or incomplete. |
| 10 | CDR analytics and live entity watchlist | Finish | Theron | 5 | 3 | CDR ingest is present, but pattern-of-life/co-location analysis and real-time watchlist alerting remain planned. |
| 11 | Classification compartments and TLP policy | Finish | Theron | 4 | 3 | Basic clearance exists; the planned RESTRICTED/SECRET compartment and TLP framework is not implemented. |
| 12 | Authorised watchlist governance workflow | New vertical | Theron | 5 | 3 | There is no purpose-bound, expiry-aware, confidence-calibrated workflow for sensitive ANPR/person/watchlist decisions. |

## Detailed recommendations

### 1. Fleet → Theron signal-to-case bridge

**Type:** Extend. **Best first implementation.**

Fleet is already the Windows command-and-control layer. Theron is already its thin console, but the current integration only exposes Fleet devices, catalog and command results. A Fleet signal is not yet a first-class Theron event that can be linked to a case, asset owner, task, hypothesis, map point or report.

Build a small, versioned bridge that ingests only approved summaries from Fleet:

- config drift and health roll-up;
- a new high-severity Fleet notification;
- Wazuh/Velociraptor finding reference, not raw provider data by default;
- evidence-bundle reference and integrity result;
- command outcome where an operator needs follow-up.

**MVP:** create a `fleet_signal` Event with Fleet device ID, timestamp, severity, policy/control ID, source reference and redacted summary. Allow an operator to create/attach a Theron case and task. Make the original Fleet record the source of truth for command enforcement.

**Rule example:** `If a device has a new critical compliance failure and remains non-compliant for 30 minutes, create a Theron task for its registered owner. Do not dispatch a repair command automatically.`

**Why it matters:** it turns Fleet from a device dashboard into part of an operational story. Like linking a fire alarm to the building, floor plan, responsible engineer and incident log—rather than merely showing a red light.

**Guardrails:** one-way minimised ingest; explicit event schema version; deduplication; source link back to Fleet; no raw file contents, screenshots, URLs or productivity telemetry unless a case policy permits them.

### 2. Fleet managed-investigation release proof

**Type:** Finish.

WinCommander Pro already contains Wazuh and Velociraptor provider paths, a finding console and gated collection flow. The feature is not yet safe to market as an operational managed service: the repository’s risk ledger says no pinned provider stack has completed backup/restore/upgrade rehearsal or a 20-device finding → snapshot → collection pilot.

**Build:** a release-readiness package, not another dashboard.

1. Publish immutable provider manifests and SBOMs.
2. Assign a patch owner and source-request process for each third-party component.
3. Run backup/restore and upgrade rehearsals against the pinned stack.
4. Execute a 20-device pilot, including failure/recovery cases.
5. Retain the signed test evidence and document supported operating limits.

**MVP success measure:** a Wazuh finding maps to an enrolled device, an authorised operator requests a snapshot/collection through the normal gates, and the provider reaches a terminal, evidenced state.

**Why it matters:** this closes the gap between “the blueprint works in tests” and “an organisation can safely depend on it.”

### 3. Real endpoint-security snapshot delivery

**Type:** Finish.

The endpoint-security snapshot is an attractive Fleet capability but cannot currently execute on a normal machine because the signed osquery tool manifest URL returns 404. This should be fixed before adding new endpoint-risk features.

**Build:** publish an immutable signed osquery tool object and manifest; add a live smoke test in release CI; verify download hash, signature, tool startup, collection result, and clean failure/retry behaviour on a fresh Windows device.

**Value:** converts existing compliance/drift data into a genuinely usable security snapshot. It is a far better near-term investment than inventing a separate vulnerability dashboard.

### 4. Sentinel camera/IoT registry and vendor adapters

**Type:** New vertical. **Direct Gujarat Sentinel foundation.**

Sentinel’s public problem statement describes 26 departments with disparate VMS/NVR, camera, storage and retention arrangements. Theron has generic entities and GIS; it does **not** yet have a domain model for cameras, VMS, NVRs, sensors, feed ownership, retention, coverage, health or maintenance.

**MVP:** add a governed Asset Registry module:

- `Camera`, `Recorder`, `VMS`, `Sensor`, `Site`, `CoverageZone` and `MaintenanceContract` entities;
- owner department, location, retention, protocol/vendor, source confidence and last health check;
- map view and queues for offline device, unknown owner, expired retention policy, and overdue maintenance;
- one read-only adapter for an approved VMS/ONVIF-compatible source plus one manual/CSV import.

**Do not start with:** centralising all video or biometric identification. The first useful output is a reliable floor plan of the estate and its health.

**Full version:** signed adapters, per-department data boundaries, edge health collector, coverage polygons, SLA workflows and lifecycle budgeting.

### 5. Federated CCTV/IoT event contract and correlation

**Type:** New vertical. **The hard, differentiating Sentinel work.**

Theron’s Event spine already normalises and correlates several intelligence feeds. What is absent is an event contract for VMS, ANPR, access-control, sensor and maintenance sources.

**Build:** an adapter-neutral event schema containing:

- source system and stable event ID;
- event time and permitted location/coverage reference;
- event kind (camera offline, intrusion, vehicle-read, crowd threshold, maintenance fault);
- confidence and model/version where an analytic produced it;
- sensitivity, retention and raw-evidence reference;
- authorised case/watchlist reference; and
- source-owner and audit fields.

**MVP:** correlate only camera-offline + maintenance signals and one approved ANPR-style metadata stream inside a time/location window. Output a *correlation candidate*, not a conclusion.

**Rule example:** `If two authorised sources report related events within 10 minutes and 500 metres, open a review candidate with both source references. Never automatically identify, accuse, detain or share a person.`

**Why it matters:** it lets every department retain its source VMS while sharing only authorised events. This matches Sentinel’s federation requirement and avoids a costly, privacy-heavy “copy all video centrally” design.

### 6. Stable intake rescreening and analyst workbench

**Type:** Finish. **High-value, low-risk Theron enhancement.**

Theron already performs claims/loan/batch shared-identifier clustering. The missing operational pieces are clear and documented:

- rescreening deletes clusters and loses `reviewed` / `dismissed` status;
- an incorrect record cannot be edited;
- there is no analyst board or chat read tool;
- large batches execute synchronously and are unbenchmarked.

**Build:** stable cluster keys based on sorted shared identifiers; `PATCH` record corrections; a review board with preserved statuses; read-only chat tools; record-count limit or background job for large runs.

**Value:** turns a technically capable detector into a repeatable analyst workflow. It applies to insurers, NBFCs, grants, vendor onboarding and authorised government-programme review.

### 7. Scheduled vendor rescreening with delivery

**Type:** Finish. **Small, commercially useful gap.**

Theron has vendor/counterparty watch entries and `due_entries()`, but no recurring loop runs the screen and no channel delivers a new sanctions/breach/adverse-media change.

**Build:** lifespan scheduler mirroring the existing geofence loop; configurable cadence; send only material changes through existing email/Telegram delivery; record delivery state and rate-limit retries. Before screening, optionally refresh authorised news coverage so “no adverse media” does not simply mean “nothing was ingested before.”

**Rule example:** `If a weekly rescreen produces a new sanctions match, create a review task and notify the designated compliance role; do not block payment or terminate a relationship automatically.`

### 8. Tamper-evident complete evidence bundle

**Type:** Finish.

Both products already have valuable evidence primitives. The missing link is an export that an external reviewer can verify without reconstructing the case by hand.

**Build:** Theron’s planned “seal evidence bundle”: one ZIP containing the §63 PDF, each cited artifact, SHA-256 values, applicable audit-chain segment, bundle manifest, WinCommander verification output when relevant, and simple offline verification instructions.

**MVP:** immutable manifest with a separate verification command and a clear warning that a lawful custodian/expert/counsel process remains required.

**Value:** makes the existing vault/certificate features work together as one defensible hand-off, rather than several strong but separate tools.

### 9. Fleet operations hardening for real organisations

**Type:** Finish.

Do not add another Fleet UI until these genuine operational gaps are closed:

- safe organisation provisioning and MSP membership-grant workflow;
- console UI for webhook/email notification channels;
- SMTP STARTTLS and authentication;
- background retention sweep instead of deletion only when a console page is opened;
- real continuation/pagination for fleet search results and older command history;
- live-device evidence for content search, remote file mutation and duress flows;
- signed-release/clean-machine verification for critical desktop functionality.

These are “boring” improvements, but they are what turn a capable command center into a dependable product.

### 10. CDR analytics and entity watchlist

**Type:** Finish. **High value, strict governance needed.**

Theron’s CDR ingest foundation exists. The planned analysis layer is still unbuilt: top/common contacts, home/work tower patterns, SIM↔IMEI swaps, tower-dump intersection, bounded co-location/co-travel, and watchlist event delivery.

**Build sequence:** start with transparent aggregate contact and tower summaries; add explicit confidence and date windows; then implement watchlist hits as reviewable Events. Keep live CDR/watchlist functions confined to lawful, authorised deployments with case/purpose controls.

### 11. Classification compartments and TLP policy

**Type:** Finish.

Theron already has a clearance ladder and fail-closed roles. The roadmap still identifies the fuller classification framework—compartments and Traffic Light Protocol (TLP)—as unbuilt.

**Build:** classification labels with rules for downgrade, sharing, expiry, originator control, and redacted exports; tests proving that clearance plus compartment membership is required. Apply it to Fleet-derived events and Sentinel case material before integrating sensitive systems.

### 12. Authorised watchlist governance workflow

**Type:** New vertical. **Required before sensitive Sentinel use cases.**

The Sentinel scenario mentions authorised database/watchlist matches. The missing product feature is governance around the list—not the matching algorithm.

**Build:** a dedicated watchlist object with legal/policy authority, purpose, owner, start/end date, allowed sources, match threshold, review queue, false-positive reason, access log, and mandatory expiry. Searches must be case-bound and scope-limited by time/place/source.

**Rule example:** `A vehicle/identity match below the approved confidence threshold creates no operational alert. A high-confidence match creates a human review with the source clip/reference and the policy basis.`

**Why it matters:** it makes the sensitive part of the system explainable and auditable, rather than just powerful.

## Additional verified gaps worth scheduling

| Opportunity | Kind | What is missing now |
|---|---|---|
| Additional live Gmail/Drive/Telegram/iMessage/GitHub integrations | Finish | Listed in Theron’s committed roadmap; existing connectors do not cover all of these live flows. |
| Situation narrative refresh | Finish | Long-running Situation pages expose raw timeline entries; a refreshed narrative is not yet built. |
| Deterministic Theron Daily fact-check | Finish | Daily writer has critique/revise option but not a deterministic, headline-aware grounding pass. |
| SSRF hardening completion | Finish | Several OSINT/GEOINT and imagery callers still use validate-then-resolve rather than the new pinned transport. |
| Opt-in loop health inventory | Finish | Operators cannot see one consolidated view of every enabled/disabled OSINT/GEOINT loop and its reason. |
| User deletion / data-retention workflow | Finish | Hard deletion lacks preflight, reassignment and deliberate archival/purge choices. |
| Pluggable, signed connector SDK | Extend | Theron has fixed connectors; a governed third-party connector contract, capability policy and test harness are absent. |
| Sentinel route reconstruction | New vertical | Theron has maps/time controls but no multi-camera ANPR track-linking/routing service. |
| Edge bandwidth/storage policy | New vertical | No component decides what video stays local, what metadata travels, or when a low-bandwidth site degrades safely. |
| CCTV privacy/audit dashboard | New vertical | No purpose-bound video-search, masking, retention exception or abnormal-access control set exists. |

## Recommended sequence

### Now — make real foundations dependable

1. Repair/publish the osquery manifest and prove the endpoint snapshot on a real enrolled device.
2. Complete one end-to-end Wazuh/Velociraptor pilot through Fleet.
3. Build Fleet → Theron `fleet_signal` ingestion and task/case linkage.
4. Ship the vendor rescreen scheduler and the intake-screening stability/UI improvements.
5. Close Fleet’s organisation, notification, retention and pagination gaps.

### Next — package existing strengths into complete workflows

1. Deliver the sealed cross-product evidence bundle.
2. Complete CDR analysis/watchlist capability behind strict authority and purpose controls.
3. Implement TLP/compartment policy before broad sensitive-data integrations.
4. Finish the stated integrations and internal hardening gaps.

### Later — build the Sentinel vertical carefully

1. Asset/camera registry and health map.
2. Two read-only, authorised adapters and a normalised event contract.
3. Case-bound correlation candidates and maintenance workflows.
4. Only after governance is validated: route reconstruction, permitted watchlist workflows, and selective evidence access.

## What should **not** be re-built

- A separate fleet server or device dashboard.
- A new generic drift engine or compliance score.
- A second generic evidence vault or §63 certificate generator.
- A second vendor-screening or shared-identifier ring detector.
- A generic event board, map, alert service, task list, or assistant-generated brief.

Each already exists. The winning work is integration, operational proof, missing controls, and carefully scoped new verticals.

## Evidence sources

- [WinCommander feature inventory](wincommander/FEATURES.md)
- [WinCommander Pro / Fleet feature inventory](wincommander-pro/FEATURES.md)
- [WinCommander Pro risk ledger](wincommander-pro/WEAKNESSES.md)
- [Theron feature inventory](theron/FEATURES.md)
- [Theron roadmap](theron/ROADMAP.md)
- [Theron known limitations](theron/WEAKNESSES.md)
- [Gujarat Sentinel problem statement](https://sentinel.gujarat.gov.in/problems)

