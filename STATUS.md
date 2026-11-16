# Status — WinCommander Free

Current state of this repository and its unfinished boundaries. It states what
is true of `main`; shipped capabilities are in [FEATURES.md](FEATURES.md) and
the change record is in [CHANGELOG.md](CHANGELOG.md). Open work is in
[ROADMAP.md](ROADMAP.md), known limits in [WEAKNESSES.md](WEAKNESSES.md), and
manual or owner-side work in [USER-TASKS.md](USER-TASKS.md).

Written against `main` at `25bd7c16`. Update the commit reference whenever this
file is revised.

## What `main` is

- App version `3.6.7` (`package.json`, `src-tauri/commander-free/tauri.conf.json`).
  The CHANGELOG has a single `Unreleased` section; no earlier release sections
  are kept there (release notes are published with each GitHub Release).
- Toolchain: Rust `1.99.1` (`rust-toolchain.toml`); Bun `1.4.0` pinned by
  `tools/ensure-dev-environment.ps1`, Bun `1.4.3` in CI.
- CI (`.github/workflows/invariants.yml`) gates: TypeScript schema, tier and
  risk invariants plus the Markdown link check, changed-file quality guards,
  security guardrails, `bun test`, generated-type drift, `cargo check`,
  `cargo test`, `cargo clippy -D warnings`, gitleaks, ESLint, cargo audit,
  cargo deny and the Free-binary forbidden-strings scan (blocking). Coverage
  is report-only; a mutation job covers `wincmd-shared`. At the last
  observation (2026-11-15) CI was green for the commit above.
- Pro, Fleet server and Investigator are separate private products; this
  repository holds the Free client, shared contracts and neutral Fleet crates
  ([OPEN_CORE.md](OPEN_CORE.md)).

## Present on `main` but not part of default builds

- Post-quantum rehearsal crate and its opt-in Cargo features: see
  [ROADMAP.md](ROADMAP.md#2-wip). Releases do not enable them and they hold no
  production keys.
- Service placeholders: `commander-svc` starts idle `reconciler` and
  `command_worker` tasks ([ROADMAP.md](ROADMAP.md#3-partial)).

## The `dev` branch

`origin/dev` is 856 commits ahead of `main` and has nothing `main` lacks. It is
deliberately not merged. At the last full check (2026-11-15) it failed `main`'s
clippy gate, the changed-file size guard and some unit tests, and it carries
a standard-user regression (see the evidence-history entry in
[ROADMAP.md](ROADMAP.md#2-wip)). Everything that exists only on
`dev` is listed as WIP in [ROADMAP.md](ROADMAP.md#2-wip) and is not shipped.
Its docs diverge from `main`; reconcile them rather than merging blindly. The
decision on how to land `dev` is in [USER-TASKS.md](USER-TASKS.md).

## How this was checked

- Statements about code were checked by reading source and tests on `main` from
  a Linux checkout. Windows-only tests (`#[cfg(windows)]`, PowerShell suites)
  were not executed there, and nothing was exercised on a live Windows
  machine, Server/RDS, physical USB, an installed service or a signed
  installer. Those gaps are in [WEAKNESSES.md](WEAKNESSES.md#validation-gaps).
- On that checkout `lint:tiers`, `lint:quality`, `lint:docs`,
  `gen:cli-catalog:check` and the TypeScript test suite pass, except six tests
  that need `pwsh` (Pro-beta-pair and location-observation suites).
  `gen:types:check` needs `cargo` and was not run.
