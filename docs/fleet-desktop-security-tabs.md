# Fleet desktop and organization-console boundary

The WinCommander desktop Fleet panel is an endpoint surface. It enrolls this
device, reports its connection state, and prepares the local non-secret Vault
deployment manifest. It does not authenticate Fleet administrators or edit
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
