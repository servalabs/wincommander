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
