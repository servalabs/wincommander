# WinCommander Free: direct-main CI repair and tag release prompt

Copy this prompt into a new coding task **only after the intended WinCommander
Free changes are complete**.

````text
Work only in D:\GitHub\wincommander (the public WinCommander Free repository).
Do not open, edit, commit, merge, tag, build, or release D:\GitHub\wincommander-pro.

I explicitly authorize narrowly scoped commits and pushes directly to origin/main
for this release-preparation task. Do not create a branch or pull request. Do
not start a release, create a tag, delete a tag, or change a version until the
checks below have passed and you have shown me the exact final terminal commands.

Goal: make the current WinCommander Free main commit release-ready by repairing
only real CI failures, then give me safe, copy-paste-ready commands to tag and
push the exact version so GitHub Actions starts the Free release.

## Scope and safety

1. Begin with:
   - `git status --short`
   - `git branch --show-current`
   - `git fetch origin --tags --prune`
   - `git log -1 --oneline origin/main`
2. Preserve unrelated dirty files. Never use `git reset --hard`, force-push,
   rebase, or broad staging.
3. If `main` has uncommitted files not owned by this task, stop and name them;
   do not mix them into a release commit.
4. Work only on the Free/public release path. Pro files, repositories, signing
   secrets, R2 credentials, release assets, and production data are out of
   scope.
5. Do not call a local build or a unit test proof of a real installed Windows
   release. Clearly distinguish local checks from GitHub Actions and from
   installer acceptance.

## Diagnose and repair CI

1. Inspect the newest `invariants` GitHub Actions run for `origin/main` using
   `gh run list` and `gh run view --log-failed` after a run completes.
2. Fix only demonstrated failures. Trace each failure to its source/test/workflow
   contract; do not silence errors, weaken assertions, or add broad allowlists
   merely to make CI green.
3. Keep each change small and coherent. Use focused commits such as
   `fix(ci): ...` or `test(ci): ...`; stage only the paths owned by that fix.
4. Run the applicable local gates before every push:
   - `bun run lint`
   - `bun test --path-ignore-patterns "src/panels/cleanup/cleanupLockdownCoverage.test.ts"`
   - from `src-tauri`: `cargo check --all-targets`
   - from `src-tauri`: `cargo clippy --workspace --all-targets -- -D warnings`
   - from `src-tauri`: `cargo test --workspace --no-fail-fast`
   - `bun test tools/release-packaging.contract.test.ts`
   - `git diff --check`
5. Commit and push only when those checks pass. After every push, wait for the
   GitHub `invariants` run for that exact SHA. If it fails, inspect the exact
   remote log, repair the cause, and repeat. Do not call it release-ready until
   the latest applicable GitHub Actions run is green.

## Version and tag preflight

After CI is green, identify the intended semantic version as VERSION. It must be
newer than the latest published Free release and it must match all of these
files on `origin/main` exactly:

- `package.json`
- `src-tauri/commander-free/Cargo.toml`
- `src-tauri/commander-free/tauri.conf.json`
- `src-tauri/Cargo.lock` (`commander-free` package entry)

Verify using commands rather than assumptions. The tag must point exactly to
the current `origin/main` commit. Check whether `vVERSION` already exists.

If a tag of that name already exists, do not delete or replace it. Explain what
it targets and ask for explicit approval before proposing any deletion command.
Never force-move a release tag.

## Final response format

Report:

1. The exact commits pushed and their purpose.
2. Local checks passed and the green GitHub Actions run URL/ID for the final SHA.
3. Any remaining installer/device acceptance that was not performed.
4. The exact commands below, populated with the verified VERSION, only if every
   preflight passed and no same-named tag exists:

```powershell
git switch main
git pull --ff-only origin main
git status --short
git show origin/main:package.json
git tag -a vVERSION -m "release: vVERSION"
git push origin vVERSION
```

Explain that the final `git push origin vVERSION` starts the WinCommander Free
GitHub Actions release. Do not run these tag commands yourself unless I
explicitly ask you to create and push the tag.
````
