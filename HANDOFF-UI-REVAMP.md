# WinCommander UI revamp handoff

Status: **work in progress; not ready to merge**. Continue on the feature branches below. Once all outstanding work is implemented and verified, integrate the changes into main as requested. Do not treat fixture checks or a clean merge preview as proof that the native app works.

## Checkouts and branches

- Free: `D:\GitHub\.worktrees\ui-revamp-ready-20261104\wincommander`, branch `codex/ui-revamp-ready-20261104`.
- Pro: `D:\GitHub\.worktrees\ui-revamp-ready-20261104\wincommander-pro`, branch `codex/ui-revamp-ready-20261104`.
- Before this handoff the Free committed head was `12ca98a6`; Pro head is `fa7da09a`, which pins that Free commit. The handoff commit changes Free HEAD, so update and verify the Pro compatibility pin after the final Free commit is known.
- At the last audit, locally stored `origin/main` refs showed Free ahead 3 / behind 27 and Pro ahead 2 / behind 11. Fetch and recheck before integration. A merge preview for the earlier committed Free HEAD was clean; Pro had conflicts in `.github/workflows/ci.yml` and `release-dependencies.json`. The preview did not include later uncommitted Free work.
- The user's latest instruction authorizes merging into main **after** verification. No merge was performed in this handoff.

## Work present

The branch contains committed UI and workflow changes for the dashboard and DNS; network adapters, Wi-Fi Guard, neighbour, firewall and trust audits; Secure Storage and Stego Backup; Fleet/vault access; lockdown and secret settings; duplicate cleanup and disk analysis; System Cleanup; packages, updates and licensing. The handoff commit adds subsequent fixes including compact Net controls, WebRTC and network controls, lockdown and crypto-erase details, cleanup layout/coverage, App Launch Cache snapshot prototype, and a compact cleanup-record view. These are implementation scope, not proof that every request has passed live acceptance.

The initial detailed requirements are in `C:\Users\Parth\.codex\attachments\e3be8398-fd4e-4486-940d-f89095da1979\Pasted text.txt`; later requests and screenshots in the originating chat add corrections, especially folder selection, cleanup records and App Launch Cache. Reconcile each request against the actual app.

## Outstanding work

1. **App Launch Cache display:** user repeatedly reported a single mostly blank column, including after the dev server restart. The new source renders Category / Record / Details with pagination, but its appearance in the actual window is unconfirmed. Do not assume stale UI without evidence. Trace the exact live response and renderer, fix missing details, and verify both personas with real records without logging private contents.
2. **App Launch Cache backend:** snapshot, log reader and native entry point are behind the default-off `amcache-snapshot-experimental` feature. A real administrator-run snapshot and the PowerShell handoff returned 3,566 records on this machine; a public registry fixture and corrupted-log checks also passed. This is not proof of complete current Windows state or full app integration. Validate VSS writer/error/cleanup behavior, log replay and corrupt/missing logs, parser limits, and accurate snapshot-time/coverage messaging on a safe Windows environment. The currently built experimental binary predates later backend privacy and Fleet-isolation edits. Fleet snapshot collection remains disabled. Do not enable normal builds until reviewed and verified.
3. **Disk Space Analysis:** selecting folders for deletion remains disabled in the UI and backend. The user requested it. Implement safe recursive handling of Windows directory handles, links/junctions, protected descendants and races before offering deletion, and verify using disposable fixtures.
4. **System Cleanup audit:** synthetic examples were rendered for 81 scannable categories; this is not an inspection of every real card or record. Audit real read-only scan results and details, fix misleading or blank rows, preserve complete local details and honest partial-coverage messages. Do not run destructive Clear actions on real user data for acceptance.
5. **Network and Pro compatibility:** source has IP firewall handlers, but the previous dev run used an older installed Pro sidecar. Rebuild/use the matching paired sidecar, verify its actual path and test read-only operations in the app. Recheck DNS labels, adapter filtering, firewall and trust audits, update counts, lockdown, stego backup, licensing and remaining UI requests in both themes and reasonable window sizes.
6. **Delivery:** finish remaining work, run relevant tests and native checks, fetch both repositories, resolve conflicts on the feature branches, update the Pro pin to the final Free commit, confirm paired compatibility, commit all intended changes, and only then merge into main as the user requested. Do not push unless separately authorized.

## Evidence and limitations

- Earlier focused cleanup display/scan tests and TypeScript checks passed; 81-category checks used synthetic records. The experimental native build linked; a malformed CLI invocation was rejected safely. These results do not replace current-head or native UI acceptance.
- Windows Computer Use initialized but could inspect only the elevated app's title bar; it ran at a lower integrity level. A prior helper startup also failed because kernel assets were missing. Arrange an appropriate same-integrity tool session, or coordinate a precise manual in-app check with the user; do not bypass Windows protections.
- The actual scan could not read certain protected temporary folders. Preserve partial coverage for access-denied paths; paths that disappear while scanning are treated separately. Do not alter file ownership or ACLs to make a scan appear complete.
- `D:\GitHub\.worktrees\ui-revamp-ready-20261104\amcache-build`, `amcache-harness`, `cargo-cache`, `cargo-temp`, `notatin-reference` and named snapshot harness outputs were created for this work. The user asked for them to be removed, but automatic approval review rejected the assistant's recursive deletion. A manual PowerShell cleanup command was provided in the chat. Confirm what remains and follow the active policy; do not remove shared or pre-existing caches or the worktrees.
- The dev frontend was run from the Free worktree with `bun run tools/dev-server.ts --vite-only` at `http://127.0.0.1:1420/`, with the experimental debug executable launched separately. Recheck process identity, backend/sidecar versions and current state; a responding Vite server is not native acceptance. Avoid service synchronization or stopping services while vault state is unknown.

## Working rules

Preserve unrelated work and inspect Git status in both checkouts before editing. Keep local cleanup details available to the user while preventing sensitive raw records from leaking into logs or Fleet. The current App Launch Cache native snapshot path is disabled for Fleet. Keep the experimental feature disabled in ordinary builds until its safety and behavior are established.
