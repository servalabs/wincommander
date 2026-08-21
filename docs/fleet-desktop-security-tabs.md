# Fleet desktop and organization-console boundary

The WinCommander desktop Fleet panel is an endpoint surface. It enrolls this
device, reports its connection state, and presents the local service-owned
Vault access surface. It does not authenticate Fleet administrators or decide
organization policy.

Clipboard Guard and Ink Receipt policy administration remains in the web
console served by `fleet-server`. The desktop links to the enrolled server's
origin in the system browser. Administrator credentials and session tokens
therefore never enter the desktop app.

The separation is deliberate:

- The Fleet server console owns organization Clipboard Guard and Ink Receipt
  rule authoring, testing, publication, roles, and audit history.
- An enrolled device receives signed policy and cannot weaken managed rules.
- Personal Clipboard Guard rules are configured under Privacy > Monitor and
  remain local to that Windows user.
- Ink Receipt has no local policy editor; it remains Fleet-managed only.

For Vaults, the desktop asks `WinCommanderSvc` what the current named-pipe
client is allowed to use. An ordinary authorized user sees **My vaults** and
only safe labels, access/presentation, mount state, and drive letter. The
password is requested for every mount and is held only long enough to send the
mount request; paths, Windows identities, and ACLs are not displayed. A policy
editor is shown only when the service returns the bounded capability that the
current caller may manage policy. This documents the source interface, not a
live-mount acceptance result.
