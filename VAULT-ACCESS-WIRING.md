# Fleet Vault access — wiring fix

Branch `fix/vault-access-wiring`, cut from `main@4bf9ad9`.

## Goal

An administrator picks a Windows user or an access-control group for a Vault,
saves, and only those principals can mount that container. A mount made by one
user stays invisible to other sessions unless the entry is machine-presented.

## Not changed — already correct before this branch

The service-side security model was already complete. It was deliberately left
alone:

- `commander-svc/src/vault_access.rs::authorize_mount_for_token` runs
  `CheckTokenMembership` per grant SID, so group grants resolve through the
  caller's token group memberships.
- `authorize_mount` fails closed on wrong container identity, non-current
  validation state, or no matching grant.
- `vault_mount.rs::per_user_presented_drive_letter` plus session-scoped
  mounting keeps a `PerUser` mount out of other sessions.
- `WindowsLocalGroupReconciler` creates and exactly reconciles the
  deterministic per-entry `WC-Vault-*` groups that materialize direct user
  grants. Existing group principals stay direct ACL principals and are never
  nested.

The reported "denied by the secure service" was not an authorization bug. It
was an admin UI that could not express a correct grant and could not report
why one failed.

## What changed

### 1. Grant principals come from the directory, not free text

`VaultAccessTab.tsx` rendered the grant principal as a bare `Input`. An
access-control group carries a friendly `name` ("Sales") and a Windows
`localGroup` ("WC_Sales"); only `localGroup` resolves. Typing the friendly name
produced an unresolvable grant.

`VaultPrincipalPicker.tsx` now lists directory groups (emitting `localGroup`)
and Windows users (emitting `username`) under separate `optgroup`s, with a
labelled manual-entry escape hatch for domain accounts and built-ins. A saved
principal absent from the directory is preserved and pinned as
"Unverified — not in the current directory" rather than dropped. Bare names are
emitted deliberately: the service resolves via `LookupAccountNameW(NULL, ...)`.

### 2. Saving access groups creates the real Windows groups

`index.tsx::saveDirectory` was `localStorage.setItem` only, so the Windows group
never existed and `resolve_principal` failed at apply time. The renderer is
unelevated, so creation happens in the SYSTEM service behind a new privileged
verb:

    svc.vault.reconcile_access_groups        (SYSTEM/Admin only)
      -> { "groups":  [{ "local_group": "WC_Sales", "member_sids": ["S-1-5-…"] }] }
      <- { "results": [{ "local_group": "WC_Sales",
                         "state": "created"|"updated"|"unchanged"|"failed",
                         "error": null|"<reason>" }] }

It reuses the existing `LocalGroupReconciler` seam, sets membership to exactly
the supplied SIDs, validates names (non-empty, <=64 chars, rejects the reserved
`wc-vault-` prefix), caps batch and member counts, and reports per-group
failures without aborting the batch. It never calls `restore()`, so it
structurally cannot delete a pre-existing group.

Deliberately NOT added to `is_vault_management_verb`: a delegated "Vault Policy
Administrator" capability token can apply policies but cannot create Windows
groups. This strands nobody — `can_manage_policy` is `caller_privileged` alone,
so the Access control tab never renders for a delegated token holder.

Members without a SID are skipped and counted, never sent by name, so a renamed
account cannot silently bind to the wrong identity.

### 3. Diagnostics name the principal and the entry

`VaultError::PrincipalResolution` was payload-free and `pipe.rs` mapped it to a
constant string, so the rejected principal was never named. It now carries the
name and formats as `vault principal resolution failed for '<name>'`. `Copy` was
dropped from `VaultError` to allow the payload; `Clone` is kept.

The service returns per-entry results in `VaultPolicyStatus.entries[]`; the UI
stored them but rendered only `validation_state`, so "fix the listed access
problems" pointed at a list that was never shown. `vaultEntryResultLabel` now
renders each entry's result in plain language.

Privacy boundary held: the administrator typed the principal name, so echoing it
back is in scope. No resolved SID, container path, or ACL/SDDL detail enters any
error string.

### 4. Mount is gated on the policy having reached the service

The admin list mapped `activePolicy.entries` — possibly a dirty, never-applied
draft — and rendered Mount unconditionally. `vaultMountGate` is now a pure
predicate over `{ authorized, entryResult, draftDirty }` giving a distinct
reason for draft-not-applied, not-authorized, and per-entry apply failure.

### 5. Live coverage for the group path

`tools/test-vault-access-live.ps1` granted only user principals. It now also
covers: create a group, add a member, grant the group, mount as that member,
write/read; a grant naming a missing group, asserting the error names the
rejected principal; and a non-member denied `not_authorized`. The test-owned
group is `WCLiveTest-GroupGrant-<hex>` and is removed unconditionally in
`finally`. Accounts are pre-checked by name before any container work.

### 6. Adjacent bug fixed

`AccessControlTab`'s `addGroup`/`updateGroup`/`deleteGroup` used value-form
`onChange({...directory})` from a props snapshot while the async `discoverUsers`
used the functional form, so a discovery landing mid-edit silently clobbered the
discovered users. All converted to the functional form.

## Not done

A prior review listed "fix stale group-name validation". `validateAccessDirectory`
always validates the current directory; no obsolete entry is read anywhere. The
claim was not reproducible and was deliberately not chased.

## Verification

    bun test                          1150 pass / 1 pre-existing load-flake
                                      (frecency perf test; 24 pass isolated)
    bun test src/panels/fleet           88 pass / 0 fail
    cargo test -p commander-svc        119 pass / 0 fail
    cargo test -p wincmd-shared         58 pass + 1 doctest
    cargo test -p commander-free       592 pass / 0 fail

The live harness has NOT been run. It needs an elevated session and the rebuilt
service installed over `C:\Program Files\WinCommander\wincommander-svc.exe`;
against the currently installed binary it would exercise the old service, which
has neither the new verb nor the named-principal error.

Worktree note: a fresh worktree needs `node_modules`, `dist`, a sibling
`../wincommander-pro`, and `bun run encrypt-backend` before `cargo` and the full
`bun test` suite will pass. Those are gitignored build inputs, not code.

## Bug found by live testing

`ensure_local_group` accepted only `NERR_GroupExists` (2223) from
`NetLocalGroupAdd`. Windows reports an existing *local* group as
`ERROR_ALIAS_EXISTS` (1379); 2223 is what the *global* group API returns. Every
reconcile against an already-existing group therefore failed at the first line
and left membership silently unchanged.

This was pre-existing, not introduced here: `reconcile_exact_members` is also
the managed `WC-Vault-*` path, so a second policy apply would have hit it too.
Unit tests could not catch it — they run against a fake reconciler. Fixed; the
status decision is now the pure `local_group_add_status_is_ok`, covered by
`an_already_existing_local_group_counts_as_created_ok`.

## Live verification

Run against the real service on SERVER: the branch binary in `--console` mode
with the installed service temporarily stopped. Nothing was overwritten.

| Case | Result |
|---|---|
| reconcile a fresh group | `created`, members exactly `SERVER\Sales1` |
| reconcile an already-exact group | `unchanged` |
| add a member, then remove it | `updated`, real membership tracked exactly |
| reserved `WC-Vault-` prefix | `failed`, "invalid local group name" |
| batch: one invalid + one valid | invalid fails, valid still applies |
| apply a policy granting the GROUP | `validation_state: current`, entry `applied` |
| managed group side effect | `WC-Vault-5667798df2bb9b2c-W` created |
| grant naming a missing group | `vault principal resolution failed for 'WCTest_NoSuchGroup'` |
| owner projection | `list_authorized` returns the entry, `access: write` |

Not executed: an actual mount by a group member. That needs a real encrypted
container plus the engine binary, and password-less SSH loopback, which is
broken on this box — the local ssh client rejects the key at
`C:\Users\Administrator\.ssh\key` for "bad permissions" while the server
accepts it. Fix with:

    icacls "C:\Users\Administrator\.ssh\key" /inheritance:r /grant:r "SERVER\Administrator:R"
