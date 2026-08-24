# Contributing to WinCommander

Thanks for your interest in contributing. WinCommander uses a
**dual-licence** model:

- **Free tier** (`commander-free`, `wincmd-shared`, frontend):
  [GNU Affero General Public License v3.0](LICENSE) — OSI-approved
  open source with copyleft + network-use clause.
- **Pro / Cleanup tier** (`commander-pro`): governed by the
  [WinCommander EULA](https://servalabs.com/eula) — private repo,
  proprietary; paid to run.

By submitting a PR you agree to the [Contributor License Agreement](CLA.md).
The CLA grants ServaLabs an irrevocable right to relicense your
contribution under any future terms, so ServaLabs can evolve the
licence model without contributor sign-off each time.

This document is the engineering on-ramp. The product overview,
two-binary architecture, and licence model live in the [README](README.md)
and [OPEN_CORE.md](OPEN_CORE.md).

## Ground rules

- Read [AGENTS.md](AGENTS.md) first — it's the canonical engineering
  entry point: build/test/run commands, conventions, and gotchas.
  CI enforces a subset of its rules.
- [BESTPRACTICES.md](BESTPRACTICES.md) is the global AI/coding baseline
  that overrides project-local rules where it is stricter. Read it
  before making structural changes.
- Every new toggle or backend command must declare `tier`, `needsAdmin`,
  `irreversible`, `reducesSecurity`, `defenderFlagged`. The tier-invariant
  linter (`bun run lint:tiers`) blocks PRs that violate the matrix.
- Defender-flagged *execution logic* MUST live in `commander-pro` only —
  the Free binary ships no security-reducing operations. The strings-clean
  goal is checked by `bun run lint:strings-free` (run it locally; the CI
  `strings-grep-free` job runs the same scan as a **blocking** gate).
- Re-run `bun run encrypt-backend` after any edit to
  `src-tauri/commander-free/scripts/**/*.ps1` so the `.enc` siblings
  rotate with the per-build salt.

## Local setup

Prerequisites:

- Windows 11 (Admin account)
- Visual Studio Build Tools 2026 with **Desktop development with C++**,
  **MSVC v143**, and **Windows 11 SDK (10.0.26100)**
- Optional: Python 3.12 if you're touching the AI / Privacy Shield
  features (`tools/install_python_deps.ps1`)

Install + run:

```powershell
# First run — installs Bun when absent and the Rust version pinned by this repo.
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\dev.ps1

# Subsequent runs
bun run dev:tauri    # canonical: launches the full Tauri window lifecycle
bun run dev:reset    # erases %APPDATA%\WinCommander\settings.json first
```

`bun run dev:tauri` invokes the `beforeDevCommand` from
`tauri.conf.json`, which verifies the pinned toolchain, then runs `bun run dev:server` (encrypt-backend → build
Pro sidecar in debug → launch Vite) and then opens the Tauri window.
The bare `bun run dev` only runs the frontend pipeline without the
Tauri window — useful for frontend-only iteration but you won't see
the app shell. Use `bun run dev:reset` whenever you want to test
first-run / fresh-install behaviour.

### The `encrypt-backend` prerequisite

A fresh clone has no `.enc` blobs — they are gitignored. The
`build.rs` compile step calls `include_bytes!` on each `.enc` file, so
`cargo build` (and `bun x tauri dev`) will fail until the blobs exist.

**Run this once after cloning, and again after any edit to
`src-tauri/commander-free/scripts/**/*.ps1`:**

```powershell
bun run encrypt-backend
```

This rotates the per-build AES salt and writes fresh `.enc` files that
the next `cargo build` picks up.

### Free-only dev/build loop (no `../commander-pro` required)

Contributors who have only cloned this repo (no sibling
`../commander-pro`) can work on the Free app without the Pro shell-out.
Use the `*:free` scripts added for this purpose:

**Vite-only frontend development** (no Tauri window):

```powershell
bun run dev:free   # kill:dev + encrypt-backend + Vite — no build:pro
```

**Full Tauri dev window — Free path (two terminals required):**

`tauri.conf.json`'s `beforeDevCommand` is hardcoded to
`bun run fetch-icons && bun run dev`, and `bun run dev` includes
`build:pro` (which shells out to `../commander-pro`). That config is
shared and intentionally not overridden here.

Free contributors use two terminals:

```powershell
# Terminal 1 — start Vite (Free; no Pro build):
bun run dev:free

# Terminal 2 — once Vite is serving on http://[::1]:1420, open the Tauri window.
# beforeDevCommand will try bun run dev and fail on build:pro; that is expected.
# Tauri connects to the already-running devUrl (http://[::1]:1420) regardless.
bun run dev:tauri:free
```

`bun run dev:tauri:free` is an alias for `bun run dev:tauri` (both
invoke the same bootstrap, which runs `bun x tauri dev --config src-tauri/commander-free/tauri.conf.json`).
The `beforeDevCommand` will error when it hits `build:pro` and there is
no `../commander-pro` sibling, but by that point Vite is already
serving from terminal 1, and Tauri connects to it successfully. The
window opens normally; only the Pro sidecar is absent.

**Free web bundle (no Tauri packaging, no Pro build):**

```powershell
bun run build:free   # encrypt-backend + tsc + vite build
```

This produces the Vite `dist/` output. It omits `build:pro:release` and
`hash-pro` — neither is required for a Free web bundle.

**Pro features** require a sibling clone at `../commander-pro/` (not
`../wincommander-pro/`). If your local checkout still uses a legacy
folder name such as `../wicommander-pro/`, `bun run build:pro` resolves
it automatically; set `WINCOMMANDER_PRO_WORKSPACE` to override the
location explicitly. They are not part of the public Free build. See
[OPEN_CORE.md](OPEN_CORE.md) for the dual-licence model.

## Testing your change

Before opening a PR:

```powershell
bun x tsc --noEmit              # frontend type-check
bun run lint                    # ESLint (--max-warnings=0) — blocking
bun run lint:tiers              # tier + risk invariants
bun run gen:types:check         # Rust→TS wire types in sync (run `bun run gen:types` if not)
cd src-tauri && cargo check --workspace
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings   # blocking
cd src-tauri && cargo test --workspace
```

CI runs all of these on every push. PRs that fail CI won't be
reviewed. `bun run lint:strings-free` is the local form of the AV-clean
gate; CI enforces it as a hard requirement via the `strings-grep-free`
job (`-HardGate`), which fails the merge on any forbidden token.

For UI changes, smoke-test the actual feature in a Tauri dev window —
type-check + invariants verify code shape, not user-visible behaviour.

## Testing Pro features as an external contributor

The Free crate compiles and runs without any licence material. The
Pro crate compiles too, but features it gates need a paid entitlement
to actually run. Two paths for contributors:

1. **Dev mode auto-unlock.** When the licensing env vars
   (`WINCMD_LICENSE_API_BASE`, `WINCMD_LICENSE_PUBLIC_KEY`) are
   unset *and* the build is `cfg!(debug_assertions)`, the licence
   layer returns a synthetic `["all"]` entitlement
   ([commander-free/src/license.rs](src-tauri/commander-free/src/license.rs))
   so paid panels unlock for development. Don't ship release builds
   with this path.
2. **Dev licence.** Email security@servalabs.com if you need to test
   release behaviour — we'll issue a development licence bound to
   your device hash.

## Working with the two-binary architecture

The Free binary is the Tauri host (UI + settings engine + licence
layer + IPC broker). The Pro binary is a headless sidecar spawned
on demand over a Windows named pipe; per-spawn session tokens prevent
replay across runs.

Common tasks:

| Task | Read first | Then read |
|---|---|---|
| Tier classification + paid-vs-free | [ARCHITECTURE.md — Executable open-core model](ARCHITECTURE.md#executable-open-core-model) | [src/types/toggles.ts](src/types/toggles.ts), [src/registry/](src/registry/) |
| New paid command in Pro | [ARCHITECTURE.md — Executable open-core model](ARCHITECTURE.md#executable-open-core-model) | Request Pro contributor access; Free-side tiering is in [src-tauri/commander-free/src/backend.rs](src-tauri/commander-free/src/backend.rs). |
| IPC wire format / pipe transport | [ARCHITECTURE.md — Free ↔ Pro IPC](ARCHITECTURE.md#free--pro-ipc) | [src-tauri/wincmd-shared/src/lib.rs](src-tauri/wincmd-shared/src/lib.rs), [src-tauri/commander-free/src/sidecar.rs](src-tauri/commander-free/src/sidecar.rs) |
| UI / styling / components | [ARCHITECTURE.md — Components](ARCHITECTURE.md#components) | the source itself: [src/](src/) and [src/styles/](src/styles/) |
| New panel / panel refactor | [src/types/panels.ts](src/types/panels.ts) | [FEATURES.md](FEATURES.md), [ARCHITECTURE.md — Components](ARCHITECTURE.md#components) |
| Settings read/write + drift | [ARCHITECTURE.md — Data model / storage](ARCHITECTURE.md#data-model--storage) | [src/types/settings.ts](src/types/settings.ts) |

## Pull request hygiene

- Keep PRs focused. Mixed bug-fix + refactor + new-feature PRs get
  bounced.
- Squash on merge unless there's a strong reason to keep history.
- The PR template has a checklist; tick the items honestly.

### Autonomous agent flow

`main` is protected: nobody pushes to it directly. Coding agents (and
humans) get **repo write, not main write** — they ship by opening a PR,
not by pushing to trunk. The flow is hands-free and fast:

```
branch → commit → gh pr create → gh pr merge --auto --squash → next task
```

CI (`invariants.yml`) plus the advisory `ai-review` run in the
background; GitHub auto-merges the instant the required checks are green.
No human waits on a non-danger change.

- **Reviewer ≠ author.** The `ai-review` workflow is a *separate*
  identity + fresh context + adversarial prompt (BESTPRACTICES 2.7 /
  2.8.12). Self-review by the authoring agent is not review.
- **Human sign-off is scoped to danger paths only.** `CODEOWNERS`
  requires owner approval on the licence / sidecar / `backend.rs` /
  capabilities / workflows / licence-worker files. Everything else
  merges on green checks alone.
- **Secrets never live in the agent's environment.** The agent uses the
  debug auto-unlock / dev licences locally; production signing and
  deploy keys live only in the tag-gated release pipeline (Actions
  secrets / OIDC / Cloudflare Worker secrets). See [SECURITY.md](SECURITY.md).

## Reporting issues

- Bug reports + feature requests: GitHub Issues. Use the templates;
  they keep triage fast.
- Security issues: **don't** open a public issue. Follow
  [SECURITY.md](SECURITY.md) — short version: email
  security@servalabs.com.

## Communication

- GitHub Issues for bug reports + feature requests
- GitHub Discussions for questions, ideas, design feedback
- Email security@servalabs.com for vulnerability reports

## Licensing — CLA required

We require a Contributor License Agreement. See [CLA.md](CLA.md) for
the full text and a plain-English summary; the short version:

1. **You keep your copyright.** The CLA grants ServaLabs a broad
   licence to your contribution; it does not transfer ownership.
2. **ServaLabs gets the right to relicense.** That includes moving the
   Free tier to a different open-source licence (MIT, MPL 2.0, AGPL,
   BSL) or changing the Pro tier's commercial terms — at any time,
   without asking contributors again. This is how we avoid the
   "can't relicense" problem that plagues projects without CLAs.
3. **You confirm the contribution is yours** (or your employer
   permits you to contribute on their behalf). If you're contributing
   on behalf of a company, email legal@servalabs.com for the
   corporate CLA.
4. **No warranty.** Standard "as-is" disclaimer.

Sign once via the CLA-assistant bot the first time you open a PR, or
paste the acceptance line from [CLA.md](CLA.md) into your PR
description if the bot isn't wired up yet.

Contributions to `commander-pro/` are governed by the
[WinCommander EULA](https://servalabs.com/eula) — private repo,
proprietary. By submitting changes to the
Pro crate you still keep your copyright; the CLA grants ServaLabs the
right to ship your contribution as part of the paid binary.

Substantial PRs that materially change the licence-gating logic
(`commander-free/src/license.rs`,
`commander-free/src/backend.rs::run_backend_script`,
`cloudflare-license-worker/`) get an extra review pass — they're
load-bearing for the open-core model.
