# Weaknesses — WinCommander Free

Known engineering and product limits of the Free client that are not security
claims. Limits that qualify what users may rely on for security are owned by
[SECURITY.md](SECURITY.md#current-operational-limits) and
[NON-GOALS.md](NON-GOALS.md); unfinished features are in
[ROADMAP.md](ROADMAP.md). This is a public list: it holds only limits already
disclosed in public documentation or visible in public source, and no
vulnerability details (report those through [SECURITY.md](SECURITY.md)).

## Known regressions on main

- **Standard users cannot record evidence events.** The evidence-history
  migration stores the ledger and its key in the machine-state directory, which
  is protected to SYSTEM and Administrators, while the desktop runs as the
  invoking (standard) user. A standard-user session is expected to fail to
  record events and to persist the failure marker. Verified for the ACL
  mechanism on one synthetic standard account only; the installed application
  and real ledger were not exercised. Widening access to Users is not an
  acceptable fix, because it would let any user alter or delete device-wide
  evidence and the key. A service-mediated design is needed; it is not
  implemented.
- **CI gates fail on `main`** after the 2026-11-16 merge of `dev` (clippy, the
  changed-file quality guard and three unit tests); details in
  [STATUS.md](STATUS.md#the-2026-11-16-merge-of-dev).

## Validation gaps

Behaviour below is covered by source review and automated tests at most; it has
not been validated in the named environment. Acceptance work is tracked in
[USER-TASKS.md](USER-TASKS.md).

- Windows Server and RDS sessions (Server Core is not a GUI target).
- Installed-service behaviour for standard users, password resets and
  simultaneous users.
- Physical USB devices, hardware-backed keys and TPM paths.
- Signed installers, the installed update path and clean-machine installs.
- Windows-only automated tests (`#[cfg(windows)]` Rust tests and PowerShell
  suites) do not run on Linux or macOS; contributors without Windows cannot
  reproduce them locally.

## Build and release

- Release builds are not byte-reproducible: the encrypted PowerShell modules use
  a per-build random salt (`tools/encrypt.ts`, `commander-free/build.rs`), and
  the drafted reproducible-build job in `invariants.yml` is disabled. Provenance
  rests on the SLSA attestation and SBOM published with each release. That the
  salt alone prevents identical rebuilds is inferred, not tested.
- The root `latest.*.json` update manifests all record version `3.2.8` (dated
  2026-08-06) while the app is `3.6.7`, and `latest.investigator.json` is
  empty. No script in this repository reads them; who owns them is an open
  question ([USER-TASKS.md](USER-TASKS.md)).
- The Markdown link check runs only when code paths change, because the CI
  path filters exclude `*.md`. Documentation-only changes are checked locally
  (`bun run lint:docs`).
- `gen:types:check` and Rust checks need a Rust toolchain; the repository's
  development flow targets Windows PowerShell.

## Code health

- Several legacy files are far above the 300-line guideline in
  [AGENTS.md](AGENTS.md), for example `commander-free/src/backend.rs`
  (about 11,000 lines), `commander-svc/src/vault_access.rs` (about 9,700) and
  `commander-svc/src/pipe.rs` (about 6,600). The quality guard only blocks new
  large files and growth of small ones, so existing giants stay.
- Two Switch components (`ui/switch.tsx`, `shared/WCSwitch.tsx`) coexist.
- Some source comments cite internal audit finding IDs (for example F-1, F-3)
  whose reports are not in this repository.
- One runtime message (`flow_capabilities.rs`) still says CameraTrigger is
  "tracked as `flows.fix-camera` in the roadmap"; no such tracker exists here
  (see [ROADMAP.md](ROADMAP.md#3-partial)).

## Optional third-party components

Components fetched on demand are not all pinned or verified by WinCommander
([INTEGRATIONS.md](INTEGRATIONS.md)):

- ImDisk installer: downloaded from a third-party host, including an `http://`
  fallback URL in `dependencies.ps1` and `ramdisks.ps1`; only a minimum-size and
  `MZ` header check was found before it is run.
- Privacy Shield detector models are fetched from Google's public model
  storage; only a minimum-size check was found, no hash pinning.
- Firefox extensions are installed by add-on slug through Firefox policy;
  WinCommander does not pin their hashes.
- winget packages rely on winget's own verification, and winget is absent on
  Windows Server 2019/2022.

## Review depth of public claims

- The network-use list in [README.md](README.md) and
  [INTEGRATIONS.md](INTEGRATIONS.md) comes from a source review, not a traffic
  audit.
- [PERFORMANCE.md](PERFORMANCE.md) describes responsiveness design only; no
  measured baseline exists.
