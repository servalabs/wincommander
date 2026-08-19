# Fleet Access Control and Vault mockups

Open `fleet-access-and-vault.html` in a browser (double-click). No build step.

This is a **proposal only**. The live Fleet panel is unchanged until you confirm the layout.

## What the mockup changes versus the current UI

**Access control**
- Windows users sit in 2–3 columns instead of one full-width row.
- A user is shown once. If display name equals username (`Accounting1` / `Accounting1`), the duplicate line is omitted. A SID subtitle appears only when it adds information.
- Names wrap instead of clipping with ellipsis.

**Vault permissions**
- Owner and disk boundary is a 3-column grid (owner, mount scope, engine path).
- The two switches, unallocated reserve, and Discover sit on one row.
- Selected volume fields use four columns.
- The group-permission matrix is unchanged in structure (it already scales horizontally).

## Implementation wait

Do not implement this in `src/panels/fleet/` until the layout is confirmed.
