# Contributing to WinCommander

Thanks for your interest in contributing. WinCommander uses a
**dual-licence** model:

- **Free tier** (`commander-free`, `wincmd-shared`, frontend):
  [GNU Affero General Public License v3.0](LICENSE) — OSI-approved
  open source with copyleft + network-use clause.
- **Pro / Cleanup tier** (private Pro repository): governed by the
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
- Defender-flagged *execution logic* MUST live in the private Pro repository only —
  the Free binary ships no security-reducing operations. The strings-clean
  goal is checked by `bun run lint:strings-free` (run it locally; the CI
  `strings-grep-free` job runs the same scan as a **blocking** gate).
- Re-run `bun run encrypt-backend` after any edit to
  `src-tauri/commander-free/scripts/**/*.ps1` so the `.enc` siblings
  rotate with the per-build salt.

## Local setup

[AGENTS.md](AGENTS.md#working-here) owns the command list; this section adds
context for contributors.

Prerequisites:

- Windows 11 (Admin account)
- Visual Studio Build Tools with **Desktop development with C++**, an MSVC
  toolset, and a **Windows 11 SDK** (10.0.26100 is the version the
  maintainers use). No repository script or CI job pins these versions, and
  the setup script does not install them.
- Git submodules: `git submodule update --init --recursive -- assets`
- Optional: Python 3.12 if you're touching the AI / Privacy Shield
  features (`tools/install_python_deps.ps1`)

First run and later runs:

```powershell
# Installs the pinned Bun and Rust toolchain when absent, prepares native
# components, then starts Tauri.
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\dev.ps1

bun run dev:tauri    # same launcher
bun run dev:admin    # relaunch elevated for administrator-only development checks
bun run dev:reset    # deletes saved settings.dat/settings.json (machine and user profiles), then starts
```

`tools/dev.ps1` completes dependency, Pro and local-service preparation
*before* starting Tauri's frontend wait, so a cold native build cannot consume
that deadline; Tauri's `beforeDevCommand` then only starts Vite on
`http://127.0.0.1:1420`. Keep Vaults safely dismounted for backend replacement:
mounted or unverifiable Vault state stops native preparation, and
`bun run dev:server` remains available for a preserved frontend session. Use
the desktop launcher for native testing; a browser-only session does not load
new native code.

### Free-only development (no Pro checkout)

With no sibling Pro workspace, `tools/dev.ps1` starts the Free-only app
automatically (it sets `WINCOMMANDER_DEV_FREE_ONLY=1`); paid actions stay
unavailable. `bun run dev:free` starts only the Vite pipeline without the Pro
build, and `bun run build:free` produces the Free web bundle (`dist/`) without
`build:pro:release` or `hash-pro`.

Pro features need a sibling clone of the private Pro repository (looked up as
`../commander-pro`, `../wicommander-pro` or `../wincommander-pro`); set
`WINCOMMANDER_PRO_WORKSPACE` to override. It is not part of the public Free
build. See [OPEN_CORE.md](OPEN_CORE.md).

### The `encrypt-backend` step

Debug builds embed the plaintext PowerShell sources directly, so ordinary
development does not need the `.enc` blobs. Release-oriented scripts
(`build`, `build:free`, `build:tauri:release`) run `bun run encrypt-backend`,
which rotates the per-build AES salt and writes the gitignored `.enc` files.
Run it yourself after editing `src-tauri/commander-free/scripts/**/*.ps1` when
you are preparing a release build.

## Testing your change

Before opening a PR (see AGENTS.md for the full list):

```powershell
bun x tsc --noEmit              # frontend type-check
bun run lint                    # ESLint (--max-warnings=0) - blocking
bun run lint:tiers              # tier + risk invariants
bun run lint:quality            # code-quality guards (file-size limits etc.)
bun run lint:docs               # Markdown relative links and anchors
bun run gen:types:check         # Rust->TS wire types in sync (run `bun run gen:types` if not)
cd src-tauri && cargo check --all-targets
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings   # blocking
cd src-tauri && cargo test --workspace
```

CI runs these checks. `bun run lint:strings-free` is the local form of the
AV-clean gate; CI enforces it as a hard requirement via the `strings-grep-free`
job (`-HardGate`), which fails the merge on any forbidden token.

For UI changes, smoke-test the actual feature in a Tauri dev window -
type-check + invariants verify code shape, not user-visible behaviour.

## Testing Pro features as an external contributor

The Free crate compiles and runs without licence material. Pro is built
from a private repository that external contributors do not have; paid
actions need a development licence. Two paths for contributors:

1. **Free-only development.** Without a Pro workspace, `tools/dev.ps1`
   runs Free-only (`WINCOMMANDER_DEV_FREE_ONLY=1`); paid actions stay
   locked. In debug builds the licence API base and public key may be
   overridden from the environment or `.env` (`WINCMD_LICENSE_API_BASE`,
   `WINCMD_LICENSE_PUBLIC_KEY`; see
   [commander-free/src/license.rs](src-tauri/commander-free/src/license.rs));
   release builds ignore them.
2. **Dev licence.** Email security@servalabs.com if you need to test
   release behaviour — we'll issue a development licence bound to
   your device hash.

## Working with the two-binary architecture

The Free binary is the Tauri host (UI + settings engine + licence
layer + IPC broker). The Pro binary is a headless sidecar spawned
on demand over a Windows named pipe. Free verifies the connected child PID and
executable before disclosing a per-spawn session token; the token then prevents
replay across runs.

Common tasks:

| Task | Read first | Then read |
|---|---|---|
| Tier classification + paid-vs-free | [ARCHITECTURE.md — Tier & risk model](ARCHITECTURE.md#tier--risk-model) | [src/types/toggles.ts](src/types/toggles.ts), [src/registry/](src/registry/) |
| New paid command in Pro | [ARCHITECTURE.md — Free ↔ Pro IPC](ARCHITECTURE.md#free--pro-ipc) and [Tier & risk model](ARCHITECTURE.md#tier--risk-model) | Request Pro contributor access; Free-side tiering is in [src-tauri/commander-free/src/backend.rs](src-tauri/commander-free/src/backend.rs). |
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
  development licences locally; production signing and
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

Contributions to the private Pro repository are governed by the
[WinCommander EULA](https://servalabs.com/eula) — private repo,
proprietary. By submitting changes to the
Pro crate you still keep your copyright; the CLA grants ServaLabs the
right to ship your contribution as part of the paid binary.

Substantial PRs that materially change the licence-gating logic
(`commander-free/src/license.rs`,
`commander-free/src/backend.rs::run_backend_script`) get an extra review pass — they're
load-bearing for the open-core model.
