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
- [PERFORMANCE.md](PERFORMANCE.md) — responsiveness design and measurement status
- [OPEN_CORE.md](OPEN_CORE.md) — licensing and public/private source boundary
- [NON-GOALS.md](NON-GOALS.md) — deliberate product boundaries
- [POSITIONING.md](POSITIONING.md) — intended audience and product stance
- [CHANGELOG.md](CHANGELOG.md) — public release-facing changes
- [docs/cli.md](docs/cli.md) — Free command-line interface
- [docs/README.md](docs/README.md) — detailed documentation map
- [docs/engineering/ipc.md](docs/engineering/ipc.md) — public Tauri and Free/Pro IPC catalog
- [docs/frontend/settings-reference.md](docs/frontend/settings-reference.md) — public settings and toggle reference
- [docs/product/flows.md](docs/product/flows.md) — public automation reference

The public code and the linked architecture references are the source of truth
for public interfaces. Plans, runbooks, mockups, private implementation,
commercial work, and acceptance records are intentionally maintained only in
`wincommander-pro`.

## Documentation layout

- Keep repository-wide documents in the root: `README.md`, architecture,
  security, performance when present, and primary product-truth documents.
  Do not move them into `docs/`.
- Keep detailed, component-specific architecture, security, and performance
  references in `docs/engineering/`.
- Keep detailed product references in `docs/product/`; repository-wide features,
  positioning, roadmap, non-goals, weaknesses, user/owner tasks, and changelog
  remain at the root when present.
- Put area-specific references in the relevant `docs/` folder, such as Fleet,
  operations, integrations, Vault, Investigator, frontend, or backend.
- Do not add plans, readiness ledgers, internal audits, or private runbooks
  here. Unfinished private work lives in `wincommander-pro` `STATUS.md`,
  `USER-TASKS.md`, and `REFACTOR.md`. Public `docs/plans/` is unused.
- Put exploratory or historical research in `docs/research/`; do not treat it
  as current product truth without verification.
- Keep contributor and agent guidance in `docs/agents/`; retain root
  `AGENTS.md` as the tool-discoverable entry point.
- When adding or moving documentation, update its Markdown links and
  `docs/README.md` in the same change.

## Documentation ownership

- `README.md` is the concise public front door; it links to detail rather than
  reproducing feature, security, or technical reference material.
- `FEATURES.md` owns current user-visible capabilities and tiers;
  `POSITIONING.md` owns audience and differentiation; `NON-GOALS.md` owns
  deliberate exclusions; `OPEN_CORE.md` owns the licence boundary.
- `ARCHITECTURE.md` owns components, control/data flow, interfaces, and trust
  boundaries. `SECURITY.md` owns the threat model and disclosure policy.
- `PERFORMANCE.md` owns responsiveness design, budgets, baselines, and
  measurement evidence. `CHANGELOG.md` owns shipped historical changes.
- This repository carries **no weaknesses ledger and no roadmap**. The open
  limitations and validation gaps for this client are tracked privately in
  `wincommander-pro` (`docs/product/PUBLIC-FREE-WEAKNESSES-INTERNAL.md`), and
  approved future work in that repository's `ROADMAP.md`. A limitation that must
  reach users belongs here as a boundary in `NON-GOALS.md` or a threat-model
  statement in `SECURITY.md`, written for that audience — never as a new
  weaknesses or roadmap file.
- Detailed command, settings, and automation references own their subject-area
  contracts. Other documents may keep only a short summary and link to the
  authoritative reference.

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
