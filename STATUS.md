# Status — WinCommander Free

Current state of this repository and its unfinished boundaries. It states what
is true of `main`; shipped capabilities are in [FEATURES.md](FEATURES.md) and
the change record is in [CHANGELOG.md](CHANGELOG.md). Open work is in
the private open-work ledger, known limits in [WEAKNESSES.md](WEAKNESSES.md), and
manual or owner-side work in [USER-TASKS.md](USER-TASKS.md).

Reviewed against `main` at `e18afcd4` (2026-11-18) for versions, CI wiring and
the facts marked as re-read from source. Statements about the 2026-11-16 merge
were checked at `dc5722ba`, and Fleet connect at `356e4b14`; those were not
re-run at `e18afcd4`. No builds or tests were run for this revision. Update the
commit reference whenever this file is revised.

## What `main` is

- App version `3.6.7` (`package.json`, `src-tauri/commander-free/tauri.conf.json`).
  The CHANGELOG has a single `Unreleased` section; no earlier release sections
  are kept there (release notes are published with each GitHub Release).
- Toolchain: Rust `1.100.0` (`rust-toolchain.toml`); Bun `1.4.0` pinned by
  `tools/ensure-dev-environment.ps1`, Bun `1.4.3` in CI.
- CI (`.github/workflows/invariants.yml`) gates: TypeScript schema, tier and
  risk invariants plus the Markdown link check, changed-file quality guards,
  security guardrails, `bun test`, generated-type drift, `cargo check`,
  `cargo test`, `cargo clippy -D warnings`, gitleaks, ESLint, cargo audit
  and the Free-binary forbidden-strings scan (blocking). cargo deny runs
  report-only (`continue-on-error`). Coverage is report-only; a mutation job covers `wincmd-shared`. For
  `dc5722ba` the hosted `invariants` run (2026-11-16) concluded `success`, read
  from the GitHub run list on 2026-11-17. Two later pushes to `main` had a
  `failure` conclusion at that time and were not investigated; CI status for
  the current head of `main` (`e18afcd4`) was not checked.
- Pro, Fleet server and Investigator are separate private products; this
  repository holds the Free client, shared contracts and neutral Fleet crates
  ([OPEN_CORE.md](OPEN_CORE.md)).

## Present on `main` but not part of default builds

- Post-quantum rehearsal crate and its opt-in Cargo features: see
  the private open-work ledger. Releases do not enable them and they hold no
  production keys.
- Service placeholders: `commander-svc` starts idle `reconciler` and
  `command_worker` tasks (tracked in the private open-work ledger).

## Fleet connect (2026-11-17)

Merged from `codex/fleet-connect-v2-20261117` with `origin/main` at `356e4b14`:
the connect stack-overflow guard, https-only addresses with clear errors,
stale-trust repair, the epoch floor bound to the enrollment generation (user
confirmed, service-checked), the dispatch toggle and status-only Shield
reporting ([CHANGELOG.md](CHANGELOG.md)). The app and `commander-svc` are
coupled: epoch adoption relies on new `get_policy` fields (confirmed and trust
generation) from the service and fails closed against an older one, so ship
them together. Open: no rollback anchor for the
persisted epoch state, recovery and re-admission disabled by design, commands
still on the legacy check-in route, and old-database identity history not
migrated ([WEAKNESSES.md](WEAKNESSES.md#fleet-connect)). Checked by automated
tests only, not against a live Fleet server or an installed service.

## The 2026-11-16 merge of `dev`

On the owner's decision, `main` was fast-forwarded to `dev` (859 commits, the figure recorded for the merge and not recomputed here; [CHANGELOG.md](CHANGELOG.md) counts 1145 from `v3.6.7`, a different baseline) and the
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
  `main`), `lint:docs` and the security guardrail scripts passed when run for commit
  `27f22f4f` on Windows. Commits after it (including the `rusqlite` 0.42 update
  in `dc5722ba`) were not re-checked locally by this file's author.
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
the private open-work ledger. Docs for features that arrived with the merge
(`FEATURES.md`, `ARCHITECTURE.md`, `SECURITY.md`) were merged textually but not
reconciled feature by feature against the 859 commits; treat them as unverified
for those features. The failing gates were fixed in `27f22f4f` (above); 35
commits have landed since and their gates were not re-run.

## How this was checked

- Statements about code were checked, before the merge above, by reading source and tests on the previous `main` from
  a Linux checkout; they were not re-verified after the merge except where a
  section above says so. Windows-only tests (`#[cfg(windows)]`, PowerShell suites)
  were not executed there, and nothing was exercised on a live Windows
  machine, Server/RDS, physical USB, an installed service or a signed
  installer. Those gaps are in [WEAKNESSES.md](WEAKNESSES.md#validation-gaps).
- On that checkout `lint:tiers`, `lint:quality`, `lint:docs`,
  `gen:cli-catalog:check` and the TypeScript test suite pass, except six tests
  that need `pwsh` (Pro-beta-pair and location-observation suites).
  `gen:types:check` needs `cargo` and was not run.
