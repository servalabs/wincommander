# Non-Goals — WinCommander

What this project deliberately is NOT and will not do. These scope boundaries derive from the code's intentional choices and the open-core model. See [POSITIONING.md](POSITIONING.md) for what it *is*, and [SECURITY.md](SECURITY.md) for the threat-model boundaries.

## This is NOT

- **NOT cross-platform.** Windows 11 only (`x86_64-pc-windows-msvc`; pinned in `rust-toolchain.toml`, Win32/PowerShell/registry-bound throughout). There is no macOS or Linux target.
- **NOT a telemetry / cloud product.** Zero telemetry; all state is local — settings, PIN hashes, licence cache, and the hidden-mode flag under `%ProgramData%\WinCommander\`, per-user scratch (logs, Privacy Shield quota) under `%LOCALAPPDATA%\WinCommander\`. There is no analytics pipeline, no account system, and no server-side state beyond paid licence rows.
- **NOT a plugin platform.** No plugin marketplace, no third-party plugin SDK, no signed `.wcplugin` packages. Exactly two ServaLabs-signed binaries (`wincommander-free.exe`, `wincommander-pro.exe`) plus the shared IPC crate.
- **NOT a fully open-source product.** The Free tier is AGPL-3.0; the paid Pro tier is closed source, private, and proprietary. Its runtime entitlement check governs use, while the [WinCommander EULA](https://servalabs.com/eula) governs the commercial code (see [OPEN_CORE.md](OPEN_CORE.md)).
- **NOT antivirus / EDR.** It hardens, audits, and can disable Defender (paid) — it does not replace real-time malware scanning or endpoint detection & response.
- **NOT a general forensic/incident-response suite for arbitrary OSes.** WinCommander targets Windows endpoints; it is not a substitute for a full cleanup lab.
- **NOT a consumer "one-click optimizer" that hides risk.** Irreversible and security-reducing actions are surfaced with explicit warnings, countdowns, and tier gating — not buried.

## We will not

- **We will not ship Defender-flaggable execution logic in the Free binary.** Higher-risk execution logic lives only in the licence-gated Pro sidecar; the Free binary retains the safeguards and re-harden/revert paths. CI blocks forbidden strings from the Free build.
- **We will not trust the frontend for security decisions.** UI gating is cosmetic; the backend re-checks entitlement, module enablement, and command classification.
- **We will not accept network hosts or integrity hashes from the frontend.** The Pro download host is pinned in Rust and artifacts are verified before use.
- **We will not bypass entitlement checks in development or release builds.** Every paid action requires a valid entitlement or trial.
- **We will not widen the Tauri filesystem/shell capability surface casually.** New webviews require their own narrow capability instead of inheriting broad permissions.
- **We will not phone home on the Free tier.** Without a paid licence, the licence worker is never contacted; the only baseline call is the update check.
- **We will not ship a voice-triggered lockdown.** Safety-critical duress triggers must be deterministic.

## Out of scope

- **Mobile companion app** — WinCommander owns the desktop; the phone/server are sibling products, not this repo.
- **Non-NIST/online-only cleanup enrichment** — not a current guarantee.
