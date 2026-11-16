# Status — WinCommander Free

Current state of this repository and its unfinished boundaries. It states what
is true of `main`; shipped capabilities are in [FEATURES.md](FEATURES.md) and
the change record is in [CHANGELOG.md](CHANGELOG.md). Open work is in
[ROADMAP.md](ROADMAP.md), known limits in [WEAKNESSES.md](WEAKNESSES.md), and
manual or owner-side work in [USER-TASKS.md](USER-TASKS.md).

Written against `main` at `ddc01916` (after the 2026-11-16 merge of `dev`, section
below). Update the commit reference whenever this file
is revised.

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

## The 2026-11-16 merge of `dev`

On the owner's decision, `main` was fast-forwarded to `dev` (859 commits) and the
`dev` branch was then deleted, so `main` is the only branch. The merge was made
knowingly with failing gates. The lint and test failures were fixed the same day
(commit `27f22f4f`):

- `cargo clippy` (workspace, and the two pq feature combinations) exit 0 on
  Windows. The 121 errors were fixed with scoped `#[allow]` attributes on
  items that are only unreachable in some feature or build combinations, type
  aliases, and a few rewrites; the allowed dead code is still dead code in
  those builds.
- `cargo test --workspace --no-fail-fast` passed 3184 tests with 0 failures on
  Windows. Three tests had failed: the fleet service-gate test (its expected
  count of gated sites was stale: 14 now, including the five recovery
  mutations), the CLI-catalog test (`cli_catalog.generated.json` was stale by
  three folder-delete handlers; regenerated), and the duplicate-finder test
  (fails only when `USERDOMAIN` does not match the real machine name, as in an
  SSH session where it is `WORKGROUP`; it passes with the real name).
- `gen:types:check`, `lint:tiers`, `lint:quality` (against the previous
  `main`), `lint:docs` and the security guardrail scripts pass. Hosted CI has
  not been re-run.
- The changed-file size guard flagged 20 source files that arrived over its
  limits with the merge; they remain oversized and are listed in
  [WEAKNESSES.md](WEAKNESSES.md#code-health).
- **Standard-user regression:** the evidence-history storage migration moves the
  device-wide ledger to a directory that standard (non-administrator) users
  cannot write, so a standard-user session cannot record or persist evidence
  events. See [WEAKNESSES.md](WEAKNESSES.md#known-regressions-on-main).

Experimental and gated work that arrived with the merge (folder deletion,
settings-generation cutover, encrypted index, service-owned recovery fixtures)
stays disabled or unwired by default and is listed in
[ROADMAP.md](ROADMAP.md#2-wip). Docs for features that arrived with the merge
(`FEATURES.md`, `ARCHITECTURE.md`, `SECURITY.md`) were merged textually but not
reconciled feature by feature against the 859 commits; treat them as unverified
for those features. The owner decision still open is how to fix the gates
([USER-TASKS.md](USER-TASKS.md)).

## How this was checked

- Statements about code were checked, before the merge above, by reading source and tests on the previous `main` from
  a Linux checkout. Windows-only tests (`#[cfg(windows)]`, PowerShell suites)
  were not executed there, and nothing was exercised on a live Windows
  machine, Server/RDS, physical USB, an installed service or a signed
  installer. Those gaps are in [WEAKNESSES.md](WEAKNESSES.md#validation-gaps).
- On that checkout `lint:tiers`, `lint:quality`, `lint:docs`,
  `gen:cli-catalog:check` and the TypeScript test suite pass, except six tests
  that need `pwsh` (Pro-beta-pair and location-observation suites).
  `gen:types:check` needs `cargo` and was not run.
