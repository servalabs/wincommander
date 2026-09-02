# Left work

## Fleet access-control and Vault behaviour

These items are documented only in this change. Do not refactor or change the
Fleet/Vault implementation until this work is explicitly scheduled.

- [ ] **Create real Windows groups when an access-control group is saved.**
  The current Access control screen stores groups and selected users in local
  browser storage only. It does not create a Windows local group or add its
  members, so `SERVER\\Sales` cannot resolve for the secure service unless it
  already exists as a Windows group.
- [ ] **Fix stale group-name validation.** The screen can show “Every group
  needs a name” even though the visible groups have names. Validate the current
  draft instead of an obsolete group entry.
- [ ] **Make Vault save/apply errors actionable.** The service authorizes real
  Windows account/group SIDs, not matching text alone. Show the rejected
  principal or policy-write error and verify the policy reached the service.
- [ ] **Add end-to-end coverage.** Create a Windows group, add a member, save
  a shared Vault policy, and mount as an authorized user. Also verify missing
  groups and unauthorized users are denied with a clear reason.

## Current evidence

- `SERVER\\Parth` is a valid local Windows account on the reported machine.
- `SERVER\\Sales` was not present as a Windows group when checked.
- No Vault policy was persisted for the secure service, so Mount correctly
  returned “denied by the secure service.”

## Verification loop and shared-contract follow-ups (2026-09-02)

Public-repository items from the 2026-09-02 architecture review. Evidence and
the private-side items live in the private repository's ledgers. Do them in
this order; each makes the next cheaper.

- [ ] **Run the cheap gates on every push.** `invariants.yml` triggers on
  `pull_request` only, so almost no commit is checked. Add a
  `push: branches: ['**']` trigger for `bun x tsc --noEmit`, `bun test`,
  `bun run lint:tiers`, and `cargo check` on touched crates, plus a pre-push
  hook running `bun test`. `bun test` on `main` currently fails 2 of 1177
  cases (`WindowsSettings.contract.test.ts`); fix them the same day.
- [ ] **Plaintext PowerShell modules in debug builds.** `build.rs` hard-fails
  without `.build_salt`, and `bun run encrypt-backend` embeds a fresh random
  salt on every run, so any `.ps1` edit forces a near-full rebuild. Add a
  `cfg(debug_assertions)` path that `include_str!`s the `.ps1` sources and a
  committed dev salt. Release builds are unchanged.
- [ ] **Unknown service verbs must fail.** `commander-svc/src/pipe.rs`
  `dispatch_verb` returns `{ "ok": true }` for verbs it does not know, so a
  wire-string typo reads as success. Define verb constants in `wincmd-shared`
  and return `unknown_verb`.
- [ ] **One mount plan and one error vocabulary.** Put a
  `#[serde(deny_unknown_fields)] MountPlan` in `wincmd-shared`, validate it once
  in the service, and make the shared `VaultMountReason` the only error type
  the UI receives: `vaultMountResultLabel` must render `result.reason`, and
  `VaultAccessTab.tsx` must not swallow the thrown error. Attach a
  service-issued incident id to every rejection.
- [ ] **Move the access-control directory out of browser storage.** The
  friendly-group → `WC_*` group → members mapping exists only in
  `localStorage`; Windows groups are created on Save in the Access control tab,
  not on Apply. The service should own the directory and reconcile groups
  inside `apply_policy`, dismounting only entries whose grants changed instead
  of every active vault.
- [ ] **Check-in envelope into `fleet-proto`.** `fleet-proto` has no
  `CheckinRequest`; agents and server each hand-maintain their own and have
  already drifted. Define it once with `#[derive(TS)]` and regenerate
  `src/types/generated/fleet.ts`.
- [ ] **Correlated, agent-readable logs.** Propagate one `request_id` from the
  UI through the service and sidecar, and extend
  `wincommander-free.exe run tauri:get_log_records` to merge all three
  processes' logs, so an agent can answer "did the request reach the sidecar"
  without a human.
