# Changelog — WinCommander (Free)

All notable fixes and changes to this repo (`commander-free`, `wincmd-search`,
`wincmd-shared`, and the React frontend), in
[Keep a Changelog](https://keepachangelog.com/) style. Dates are UTC+5:30 (repo
commit timestamps). Shipped capabilities (not fixes) live in
[FEATURES.md](FEATURES.md); architecture and design invariants live in
[ARCHITECTURE.md](ARCHITECTURE.md); the lessons worth not relearning live in
[AGENTS.md](AGENTS.md) → *Gotchas*.

## [3.3.0](https://github.com/servalabs/wincommander/compare/wincommander-free-v3.2.4...wincommander-free-v3.3.0) (2026-08-03)


### Features

* **apps:** compact unmatched package updates ([ed1738e](https://github.com/servalabs/wincommander/commit/ed1738e6ed67282ac1681a1ec0d086b567b5c64b))
* **apps:** improve classic app status ([1bce722](https://github.com/servalabs/wincommander/commit/1bce722f9d54d1f793bc6deb8446f421e80bc391))
* **apps:** install a second app into the running install, not a schedule ([dadc383](https://github.com/servalabs/wincommander/commit/dadc383c1dd819a31a4868a2517c27b5925f301c))
* **apps:** prefer native debloat icons ([e6d0a7a](https://github.com/servalabs/wincommander/commit/e6d0a7a96616d8720110c753591c1937108fe385))
* **context:** run explorer actions directly ([43de8a7](https://github.com/servalabs/wincommander/commit/43de8a793a9e5c16161d0157b29d8feae171215c))
* **flows:** expand settings and test coverage ([19d010d](https://github.com/servalabs/wincommander/commit/19d010d71a102bd4a305b266577fe6bcb7b821c7))
* **license:** shorten the path to payment, fix the sidebar licence card ([8fe276a](https://github.com/servalabs/wincommander/commit/8fe276a86ba7ab0670596a4b33427f0e9522823c))
* **maintenance:** align cleanup scans and storage ([8d76afa](https://github.com/servalabs/wincommander/commit/8d76afaf2936153f726a1a518743ed525267256e))
* **maintenance:** align storage review controls ([3cde099](https://github.com/servalabs/wincommander/commit/3cde099969a67caadf77f55f7b8885d39e8ec9ad))
* **maintenance:** streamline cleanup and system scans ([2d43f3c](https://github.com/servalabs/wincommander/commit/2d43f3c0f2ad066fe61e573a6718444db9a9500e))
* **network,vault:** finish the diagnostics, crypto-erase and stego surfaces ([a44b912](https://github.com/servalabs/wincommander/commit/a44b91294311a0bf7ecd9f8b86d764a8765ed291))
* **network:** align private network controls ([c9a24f5](https://github.com/servalabs/wincommander/commit/c9a24f568ca1d9a81db668896956ace71a6fefb8))
* **privacy:** clarify shield triggers and flows ([be29cd7](https://github.com/servalabs/wincommander/commit/be29cd7a0592eb233ad5b6ef4e472c8faae11299))
* **privacy:** put Privacy Shield's advanced controls behind one disclosure ([7a2addb](https://github.com/servalabs/wincommander/commit/7a2addb2273a5c8876a6c7fcc06af7c77f5c955b))
* **settings:** refine Windows controls ([4a75d8b](https://github.com/servalabs/wincommander/commit/4a75d8ba4eed7853c77612ae3f7040fce4200a38))
* **titlebar:** split the notification bell into Alerts and Processes icons ([b63d877](https://github.com/servalabs/wincommander/commit/b63d877aba82fe9929655356ebd858680296d39a))
* **vault:** simplify secure storage workflows ([16dd526](https://github.com/servalabs/wincommander/commit/16dd526103c720d34201411b9f7c24dd98dcf2db))


### Bug Fixes

* build Pro before starting Vite ([58d7ff9](https://github.com/servalabs/wincommander/commit/58d7ff9e3187914e325c971e9ed12cc526c80ddd))
* Classic Windows apps false-detected Paint/Snipping as needing repair ([24cc307](https://github.com/servalabs/wincommander/commit/24cc3070a861db562ad150dcbb2c0bb6c1d99740))
* **cleanup:** allow independent tab scans ([c7c8669](https://github.com/servalabs/wincommander/commit/c7c8669ac34ead53274398ed112de8cf92af2d0e))
* **dashboard:** refine network traffic alerts ([fcb4bfe](https://github.com/servalabs/wincommander/commit/fcb4bfe2f97f556c8a5437166ebdab22554034cb))
* guide-tour deep-links and stale test assertions after tab refactors ([d6a5ccd](https://github.com/servalabs/wincommander/commit/d6a5ccd2f6081973821435f911b95bb8ab21f063))
* **guide:** adapt tours to current controls ([92b32b5](https://github.com/servalabs/wincommander/commit/92b32b578691a547c0e9be31d572f5742c7641bf))
* harden Scoop install against security-software blocks ([4eb22c8](https://github.com/servalabs/wincommander/commit/4eb22c8353aab17eff57c1f76fe86cdc92d06a6a))
* **maintenance:** constrain storage review layout ([9c4097f](https://github.com/servalabs/wincommander/commit/9c4097fcf3d5da37cef827e205498ccb91b5c70c))
* **maintenance:** preserve file hygiene workspace ([2126edb](https://github.com/servalabs/wincommander/commit/2126edb334665e6ba501cd4f079073a0b11966cd))
* scope entitlement errors and fleet polling ([45d953d](https://github.com/servalabs/wincommander/commit/45d953d4dca9144e0edf9e71b99009e58f7f4c18))
* **search:** shred results without modal ([e48dfac](https://github.com/servalabs/wincommander/commit/e48dfaccffdd3b9677cb9482edcc92cab72265a5))
* silence PowerShell console flash during startup-impact scan ([0eb477e](https://github.com/servalabs/wincommander/commit/0eb477e227e2743b63baf15cd8bb59b93bc3c716))
* silence PowerShell/netsh console flash on ARP and firewall scans ([a0200ab](https://github.com/servalabs/wincommander/commit/a0200abc1d6dab3921694873a12fcb33a596b01e))
* stabilize privacy monitoring layout ([1cf4e8a](https://github.com/servalabs/wincommander/commit/1cf4e8a6bc7c037f1a9b8eb2cde82c8df0afbd72))
* **tour:** stop Guided/Casual mode hiding steps the tour anchors to ([f62d6a2](https://github.com/servalabs/wincommander/commit/f62d6a297b34c5853db908e3126526cdb59e20d1))
* **tweaks:** show what vm_enable_feature actually did ([2505671](https://github.com/servalabs/wincommander/commit/25056719ff5a7c20aad4b7ea1170fccda532a9e5))
* **window:** make the tray reliably reveal the main window ([c183313](https://github.com/servalabs/wincommander/commit/c18331352567fe61c447c6cbfb3aa8c0300a7972))

## [Unreleased]

### Fixed

- **Commands that erase data asked for the weaker confirmation** (2026-08-03).
  The headless CLI graded every command from its *name*, and the destructive name
  lists were anchored to the start of the name — so the whole `Invoke-*Erase`
  family (`Invoke-7Erase`, `Invoke-UnallocatedSpaceErase`, `Invoke-CrashDumpErase`,
  `Invoke-PreviousWindowsInstallErase`) accepted `RUN:` instead of `DESTROY:`.
  Worse, the guess disagreed with `authz::DESTRUCTIVE_COMMANDS` — the repo's own
  CI-enforced catastrophic-command registry — so `fleet_connect` (re-pinning to a
  different fleet server and signing key) and `internet_kill_switch_set` were
  graded ordinary mutations. `classify_risk` now consults that registry **first**,
  then a table of handlers hand-verified against their bodies, then the name
  rules, then any name containing `erase`/`shred`/`wipe`/`destroy` — the last
  applied only after the read-only allowlist, so `Get-AutoEraseSchedules` stays a
  read. **17 commands were regraded, every one stricter.** Adding a command to the
  registry now hardens the CLI automatically; a test enforces it.

- **Three commands that write were graded read-only — no confirmation at all**
  (2026-08-03). `search_rename_file` renames a file on disk but matched the
  `search_` read prefix; the four `export_*` commands write an artefact yet were
  eligible for the read-only wait deadline, so an export could be killed
  part-written; `fleet_update_posture_snapshot` pushes posture to the fleet server
  but matched the `_snapshot` read suffix.

- **`--dry-run` could silently execute for real** (2026-08-03). The parser took
  whatever token followed an option as its value, so `--confirm --dry-run`
  consumed `--dry-run` as the confirmation string and left dry-run off. Read-only
  commands never check the confirmation, so the preview ran live. The same
  swallow smuggled `--safe-copy` past the parser into `lib.rs::run`'s argv scan,
  hijacking the requested command and exiting `0` without running it. Flag-shaped
  option values are now rejected, and that argv scan is skipped in CLI mode.

- **A wedged backend command hung forever** (2026-08-03). `--timeout-ms` was
  accepted on read-only backend-script commands and then ignored — the documented
  behaviour, but a trap for unattended automation. Both transports now honour the
  deadline (default 300,000 ms) and exit `10`. It remains a wait limit, not
  transactional cancellation.

- **An unanswerable confirmation dialog hung the process and blocked every other
  job** (2026-08-03). `authz::native_confirm` awaits a dialog with no timeout. In
  CLI mode the only window is invisible and unattended runs have nobody to click
  it, so the process waited forever *while holding the cross-process execution
  lock* — failing every other mutating CLI run with `cli_busy`. It now fails
  closed when the CLI runtime is active: an unanswerable confirmation is a denied
  confirmation.

- **Two paths produced no JSON at all** (2026-08-03), breaking the
  one-document-per-invocation contract. A CLI-runtime setup failure reached
  Tauri's own `panic!`, exiting `101` with empty stdout; it now reports
  `runtime_error` and exit `9`. And the `lockdown`/`full_lockdown` detached
  acknowledgement sat *after* `crate::run()`, which never returns because Tauri's
  `App::run` exits the process directly — so terminal lockdown printed nothing.
  It is now emitted before dispatch, with a guard so no second document can
  follow it. It still acknowledges only the dispatch, never the outcome.

- **The debug-only command list could drift silently** (2026-08-03). Four
  dev-panel handlers are `#[cfg(debug_assertions)]`-gated and must be refused by
  release builds, but the CLI matched them against a hardcoded name list while the
  catalog generator could not see `cfg` attributes at all. A fifth gated handler
  would have been reported as executable in release, and every test would still
  have passed. The generator now derives `debugOnly` from the attribute itself and
  fails if the parse stops working; the catalog carries the flag (schema v2).

- **A `__proto__` parameter key silently vanished** (2026-08-03). Parameters were
  spliced into the runner script as JavaScript object syntax, so that key set the
  prototype instead of becoming an ordinary parameter and the handler received
  something other than what was asked for. They are now `JSON.parse`d from a
  string literal.

- **Multi-token searches returned nothing at all** (2026-07-28).
  `search_everything` passed the entire query to `es.exe` as a single argv entry,
  which `es.exe` reads as a quoted phrase — so any query combining a term with a
  filter (`ext:md dm:thisyear`) returned **zero rows with no error**. The app's own
  app-priority query (`brave* ext:exe;lnk;msi;appx;msix`) had therefore never
  matched anything, so typing an app name ranked apps only by luck.
  `backend.rs::tokenize_es_query` now splits into one argv entry per token
  (preserving quoted runs), and the frontend sends a pre-split `tokens` array.

- **Files whose names contain `" - "` could not be found by typing their name**
  (2026-07-28). The argv-injection guard correctly rejects any token starting with
  `-`, but a plain whitespace split turned the common `"Name - Description"`
  pattern — including Windows' own WinX shortcuts — into a lone `-` token, erroring
  the entire search. Measured: **198 of 4,014 real files (4.9%)**, now 0.
  `searchQueryPlan.ts::toEverythingToken` also neutralises a leading `!`, which
  Everything was parsing as its NOT operator and silently answering with ~1.4M
  wrong rows.

- **Wrong-separator searches found nothing** (2026-07-28). Typing
  `Docker-Desktop` or `Docker_Desktop` for `Docker Desktop.exe` had **0% recall**
  across 268 real cases. Separator runs are now interchangeable: 0% → **97.3%**
  found within the fetch window.

- **The user's own files were ranked behind system folders** (2026-07-28).
  Everything's native order is by path, so `C:\Windows` and `C:\Program Files` came
  first: the developer's own `D:\GitHub\assets` ranked **1193rd of 1239** folders
  named `assets*` and fell outside the fetched window entirely. Searches now sort
  by recency, fetch a wider window, and prefer the user's own space over program
  and build/cache trees. An exact- or prefix-name match now promotes **any** result
  type, not only apps — folders were previously near-invisible (recall@10 8.6%).

- **Search Files reported "no results" while thousands existed** (2026-07-28).
  When a text query and filter chips were combined, the panel sent the bare query,
  over-fetched, and filtered in JS — so "folders named assets" kept the folders
  among 50 arbitrary rows. Filters now reach the backend, and the client-side
  re-filter (whose date window disagreed with Everything's) is gone.

- **Icon extraction stalled every keystroke by ~10 seconds** (2026-07-28).
  `parse_es_output` spawned one PowerShell per app-like result row (**measured
  ~866 ms each**), serially, on a tokio worker thread and *outside* the search
  timeout. Removed: the frontend already lazy-loads icons per visible row and
  caches them.

- **An un-indexed filter could hang the search box indefinitely** (2026-07-28).
  `es.exe` calls now run under a hard timeout (6 s search / 4 s count) with a plain
  explanation instead of a freeze. `attrib:` forces a live disk scan and ran >100 s
  in testing.

- **Content search ignored the folder scope** (2026-07-28).
  `ContentQuery.roots` was accepted and never applied, and `search_content` had no
  scope parameter — so an "in this folder" filter narrowed the file-name list while
  "Inside files" below it kept showing hits from the whole disk. Now scoped end to
  end, component-anchored so `…\wincommander` cannot leak `…\wincommander-pro`.

- **Frecency could permanently forget your most-used files** (2026-07-28). A
  heavily-used file that went idle past 90 days scored below a brand-new single
  open, so a burst of one-off opens evicted its history at the store cap. Eviction
  no longer discards durable open history, and more path spellings of one file now
  share an entry.

### Changed

- **`Ctrl+Space` gains a chip grammar** (2026-07-28). Typing a word like `folder`
  or `images` offers a filter chip promoted with `Tab`; `Backspace` at the start of
  the text demotes the last chip back to the exact word typed. Chips cover type,
  folders/files, time, size, and folder scope. Time chips *prefer* recent by
  default and become a hard date filter on a second click, because a hard filter
  turns a slightly-wrong memory into an empty list. An empty box now lists recent
  files instead of nothing, `>name` jumps into a folder, and the folder you were
  last viewing in Explorer is offered as a scope.

- **Two chips relabelled to match what the engine does** (2026-07-28). "Empty" →
  **"Empty folders"**: Everything's `empty:` matches empty folders only and can
  never surface a 0-byte file (10,373 exist on the test machine); pairing it with
  "Files" returned zero forever and is now mutually excluded. "Duplicates" →
  **"Same name & size"**: `dupe:` matches filename and size, not content.

- **File-type chips cover many more real formats** (2026-07-28). Notably `.c`,
  `.h`, `.hpp` for Code (~35,000 files previously invisible), `.avif`/`.tif` for
  Images, `.apk` for Apps. A missing extension reads to the user as "your file does
  not exist".

- **Very short queries no longer hit the engine** (2026-07-28). 1–2 character
  terms timed out 9 of 16 runs under load versus 0 of 8 for a long term, and are
  virtually always mid-keystroke.
