# Fleet desktop organization-security tabs

The WinCommander desktop Fleet panel keeps device enrollment and the local Vault
deployment manifest separate from organization policy. Clipboard Guard and Ink
Receipt require an in-memory Fleet administrator session; no password or token
is written to browser storage.

The desktop client uses these existing Fleet routes:

- `POST /api/v1/auth/login`
- `GET`/`PUT /api/v1/orgs/{org}/settings`
- `GET`/`PUT /api/v1/orgs/{org}/clipboard-guard/rules`
- `POST /api/v1/orgs/{org}/clipboard-guard/rules/test`
- `POST /api/v1/orgs/{org}/clipboard-guard/publish`
- `GET`/`PUT /api/v1/orgs/{org}/ink-receipt/policy`

Clipboard Guard supports content-free rule drafts, canonical compile/test, and
explicit publication. Ink Receipt supports ticket limits, managed destination
classes, watermark policy, failure stance, and policy publication. Server-side
role checks remain authoritative: the interface does not grant authorization.
