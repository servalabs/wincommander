// SPDX-License-Identifier: AGPL-3.0-or-later
//! `clipboard-guard-helper` — the per-user Clipboard Guard listener (plan
//! §4.2 Phase 2, §3, §4.6).
//!
//! # Why a separate per-user process
//!
//! Today the clipboard monitor (`commander-free/src/paste_monitor.rs`) runs
//! **inside the Tauri GUI process** as a `#[tauri::command]`-started task —
//! so it dies the moment the GUI closes (plan §2.2). The clipboard belongs
//! to the interactive desktop session, so a listener that must keep running
//! with the GUI closed has to be a separate per-user process. But its
//! *durable* state — policy, receipts — must **not** live per-user: it has
//! to survive helper restarts and be readable by an admin, so it lives in
//! `commander-svc` (the SYSTEM service), per plan §3 commitment 1. This
//! crate is the per-user half of that split: it holds no durable state of
//! its own (a fresh process re-fetches policy from svc on start) and talks
//! to svc for everything that must persist.
//!
//! # Why this crate is a library with a thin binary
//!
//! Every piece of real logic here — the event-driven listener, the bounded
//! read, the match/cooldown engine, verified clear/quarantine, and the svc
//! IPC client — lives in `pub mod`s in this **library**, not in
//! `src/bin/main.rs`. That binary only wires the pieces together and runs
//! the loop.
//!
//! The reason is concrete, not aspirational: `commander-free`'s
//! `paste_monitor.rs` also needs to move off its 2000ms poll (a follow-on
//! agent's job — see that file's module doc), and if that move hand-rolled
//! a *second* Win32 clipboard-format-listener + matcher inside
//! `commander-free`, the two implementations would silently diverge the
//! first time either one got a bugfix. Building the listener/read/engine/
//! actions API as a reusable library — generic over injectable traits
//! rather than tied to this crate's own `main()` — means `paste_monitor.rs`
//! can depend on this crate and drive [`listener`], [`read`], [`engine`],
//! and [`actions`] from inside its own `tauri::async_runtime::spawn` task,
//! using the exact same matching semantics this standalone helper uses.
//! See each module's doc for the specific seam a caller seam and the
//! `public_api` note in this crate's build handoff for the exact
//! signatures a caller codes against.
//!
//! # Scope — what V1 does and does not read
//!
//! Only `CF_UNICODETEXT` is read (plan §4.2 "Read bounds"). **V1 does not
//! touch images, files, HTML, RTF, or keystrokes.** The current free-tier
//! code has no read cap beyond a `text.len() < 13` floor
//! (`paste_monitor.rs:751-754`); this crate always truncates to
//! [`read::MAX_CLIPBOARD_READ_BYTES`] (1 MiB) via
//! `wincmd_clip_rules::truncate_for_match` before matching, so the UTF-8
//! boundary is handled correctly and a pathological clipboard payload can
//! never make matching itself unbounded.
//!
//! # Content-free discipline (plan §8)
//!
//! Matched text, regex captures, and raw clipboard contents stay in
//! transient memory for the duration of one match evaluation and are
//! **never** logged, never written to disk, and never sent over the pipe.
//! What crosses `svc.clipboard.report_event` is exactly the
//! `wincmd_shared::fleet::ClipboardEventReport` field set: ids, timestamp,
//! policy_version, rule_id, rule_revision, severity, the two closed-enum
//! action lists, and a content-free `suppressed_count`. [`report::build_report`]
//! enforces this structurally — its signature has no parameter that could
//! carry clipboard text, so a caller cannot accidentally pass content
//! through it even by mistake. See `report`'s tests for an end-to-end
//! sentinel-absence check through the real match → action → report path.
//!
//! # Honest limits — this wording must ship in product copy
//!
//! Clearing or quarantining the clipboard is a strong practical control,
//! **not a paste blocker** (plan §4.6). [`HONEST_LIMITS_DISCLOSURE`] is the
//! exact text; expose it verbatim wherever this feature's behaviour is
//! explained to a user or an admin — don't paraphrase it away.
//!
//! # What this crate does NOT do
//!
//! It does not decide policy (svc resolves and signs it), does not
//! persist a receipt journal (svc does), does not verify epoch signatures
//! (that's the Ink Receipt/epoch-install bridge's job), and does not
//! implement a second IPC mechanism — it dials the existing
//! `\\.\pipe\wincmd-svc` using the existing `Envelope`/`Hello`/`Signed`
//! framing from `wincmd_shared` (plan §1.4).

pub mod actions;
pub mod engine;
pub mod health;
pub mod ids;
pub mod ipc;
pub mod listener;
pub mod policy;
pub mod read;
pub mod report;
mod retry;
pub mod timestamp;

/// Verbatim product-copy disclosure (plan §4.6) about what Clipboard Guard
/// clearing/quarantining can and cannot guarantee. Surface this exact text
/// in any UI, settings panel, or documentation that describes the feature
/// — it is written here, as a `pub const`, specifically so a caller can
/// display it verbatim instead of re-deriving (and likely softening) the
/// same disclosure independently.
///
/// The companion control this disclosure points at —
/// `privacy.clipboardHistory.disable` — already exists
/// (`commander-pro/src/fleet_dispatch.rs:469-470`) and should be surfaced
/// as a recommended pairing wherever this text is shown.
pub const HONEST_LIMITS_DISCLOSURE: &str =
    "Clearing or quarantining the clipboard is a strong practical control, \
not a paste blocker. An app may have already cached the copied data, you \
may paste before the guard reacts, and Windows Clipboard History or a \
cloud clipboard may still retain it. Turning on \"Disable Clipboard \
History\" is the complementary control for that residual risk.";
