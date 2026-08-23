# AGENTS.md — WinCommander public repository

This repository is public and AGPL-3.0 licensed. Keep every committed file safe
for public disclosure. Private Pro implementation, commercial planning, release
operations, acceptance evidence, internal audits, credentials, and vulnerability
ledgers belong in the sibling private `wincommander-pro` repository.

Before making changes, read [BESTPRACTICES.md](BESTPRACTICES.md). Apply the
stricter rule when instructions conflict.

## Public documentation

- [README.md](README.md) — install, usage, and public product overview
- [FEATURES.md](FEATURES.md) — user-visible Free/Pro/Fleet/Investigator boundary
- [ARCHITECTURE.md](ARCHITECTURE.md) — public architecture and source map
- [SECURITY.md](SECURITY.md) — disclosure policy, threat model, and public limits
- [OPEN_CORE.md](OPEN_CORE.md) — licensing and public/private source boundary
- [NON-GOALS.md](NON-GOALS.md) — deliberate product boundaries
- [POSITIONING.md](POSITIONING.md) — intended audience and product stance
- [CHANGELOG.md](CHANGELOG.md) — public release-facing changes
- [docs/cli.md](docs/cli.md) — Free command-line interface
- [docs/usb-keyboard-approval.md](docs/usb-keyboard-approval.md) — public limits
  of reactive unknown-keyboard approval

The public code is the source of truth for public interfaces. Detailed paid
protocol catalogs, settings internals, plans, runbooks, mockups, and acceptance
records are intentionally maintained only in `wincommander-pro`.

## Repository shape

- `src/` — React/TypeScript frontend
- `src/registry/` — public feature/toggle metadata and tier labels
- `src-tauri/commander-free/` — public Tauri/Rust desktop backend
- `src-tauri/wincmd-shared/` — neutral data-only inter-process contracts
- `src-tauri/wincmd-search/` — public local content-search engine
- `src-tauri/fleet-proto/` and `src-tauri/fleet-agent-core/` — neutral shared
  fleet wire types and client primitives
- `tools/` — public build, encryption, code-generation, tier, and release checks

The private sibling provides paid implementations. Never copy private source,
comments, tests, strings, or internal documentation into this repository.

## Working here

Run commands from the repository root in PowerShell on Windows.

- Setup: `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\dev.ps1`
- Development: `bun run dev:tauri`
- Type-check: `bun x tsc --noEmit`
- Frontend lint: `bun run lint`
- Tier invariants: `bun run lint:tiers`
- Generated type drift: `bun run gen:types:check`
- Free-binary string boundary: `bun run lint:strings-free`
- Rust checks: from `src-tauri`, run `cargo check --all-targets` and
  `cargo test --workspace`
- Rust lint: from `src-tauri`, run
  `cargo clippy --workspace --all-targets -- -D warnings`

The Rust toolchain is pinned in `rust-toolchain.toml`. Bun is the package
manager and task runner.

## Public-repository rules

- Use `rg`/`rg --files` for search.
- Preserve unrelated working-tree changes.
- Keep product Markdown at the existing public root or `docs/` locations; do
  not add plans, readiness ledgers, internal audits, or private runbooks here.
- Do not commit `.env`, credentials, signing material, internal endpoints,
  private design URLs, local capture paths, or customer/employee data.
- Keep paid enforcement server-side/backend-side. UI visibility is not an
  entitlement boundary.
- Do not widen Tauri filesystem or shell capabilities without a narrow,
  reviewed requirement.
- Do not accept arbitrary shell commands, executable paths, URLs, hosts, or
  integrity hashes from the frontend.
- Keep `wincmd-shared`, `fleet-proto`, and `fleet-agent-core` neutral: data and
  protocol contracts only, never proprietary implementation logic.
- New public source must remain compatible with the AGPL boundary described in
  [OPEN_CORE.md](OPEN_CORE.md).
- New files should stay under roughly 300 lines. Put state/IPC in hooks or
  services and pure logic in `src/lib/`.
- Never use `dangerouslySetInnerHTML`. Never invoke PowerShell directly from
  JavaScript.

## Required generated artifacts

- After editing any plaintext backend `.ps1`, run `bun run encrypt-backend`.
- After editing shared Rust wire types, run `bun run gen:types`, then
  `bun run gen:types:check`.
- Backend command changes must preserve tier and module registration; run the
  tier and CLI-catalog checks defined in `package.json`.

## Before finishing

Run the smallest relevant checks plus the documentation/link checks for the
files changed. Do not claim live Windows, Server/RDS, physical-device, signed
release, browser, or field behavior unless it was actually exercised in that
environment. Record internal failures and follow-up work in the private repo,
not in public documentation.
