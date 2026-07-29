# Changelog — WinCommander (Free)

All notable fixes and changes to this repo (`commander-free`, `wincmd-search`,
`wincmd-shared`, and the React frontend), in
[Keep a Changelog](https://keepachangelog.com/) style. Dates are UTC+5:30 (repo
commit timestamps). Shipped capabilities (not fixes) live in
[FEATURES.md](FEATURES.md); architecture and design invariants live in
[ARCHITECTURE.md](ARCHITECTURE.md); the lessons worth not relearning live in
[AGENTS.md](AGENTS.md) → *Gotchas*.

## [Unreleased]

### Fixed

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
