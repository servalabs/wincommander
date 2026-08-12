<!-- Thanks for the PR! Fill in below — be concise. -->

## What this PR does

<!-- 1–3 sentences. Why is this change needed? What does it do? -->

## Tier classification

<!-- Tick exactly one. -->

- [ ] **Free-only** — change touches only `commander-free` /
      `wincmd-shared` / frontend Free behaviour
- [ ] **Pro-only** — change touches only `commander-pro` (paid sidecar)
- [ ] **Both** — Free + Pro both change (e.g. new shared IPC type)
- [ ] **Tooling / docs / CI** — no runtime tier impact

## Checklist

- [ ] `bun x tsc --noEmit` passes
- [ ] `bun run lint` passes (ESLint, `--max-warnings=0`)
- [ ] `bun run lint:tiers` passes (tier + risk invariants)
- [ ] `cd src-tauri && cargo check --workspace` passes
- [ ] `cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] If I edited any `.ps1` under
      `src-tauri/commander-free/scripts/modules/`, I ran
      `bun run encrypt-backend`
- [ ] If I changed the fleet/IPC wire types in `wincmd-shared`, I ran
      `bun run gen:types` and committed `src/types/generated/`
- [ ] If this introduces a new toggle / backend command, it declares
      `tier`, `needsAdmin`, `irreversible`, `reducesSecurity`,
      `defenderFlagged`
- [ ] I smoke-tested the change in a Win11 admin VM/host (UI behaviour
      verified, not just code shape)

## Defender / AV impact

<!-- Required if Defender-flagged or otherwise AV-noisy. Free crate must stay
clean. If the answer is "this MUST live in commander-pro", confirm it does. -->

- [ ] No Defender / AV impact
- [ ] Lives in `commander-pro` (not `commander-free`)
- [ ] N/A

## Screenshots / clips

<!-- For UI changes. A 5-second screen recording > a static screenshot. -->

## Related

<!-- Issues, prior PRs, docs/ references you read while writing this. -->

## Contributor License Agreement

- [ ] I have signed the [CLA](../CLA.md) — via the CLA-assistant bot
      if it's wired up by the time you read this, otherwise by
      pasting the agreement text into this PR description per
      [CLA.md § How to sign](../CLA.md#how-to-sign)
