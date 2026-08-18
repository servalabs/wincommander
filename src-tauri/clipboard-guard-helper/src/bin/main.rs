// SPDX-License-Identifier: AGPL-3.0-or-later
//! `wincommander-clipboard-guard.exe` — thin per-user process entry point.
//!
//! Every real behaviour lives in the `clipboard_guard_helper` library
//! (see its crate doc for why this binary is deliberately thin). This
//! file only: fetches policy from `commander-svc`, starts the listener
//! (event-driven, falling back to polling), and runs the observe →
//! act → report loop.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clipboard_guard_helper::actions::{
    execute_local_actions, mark_submission_actions, NullNotifier, Win32Writer,
};
use clipboard_guard_helper::engine::{MatchEngine, MatchOutcome};
use clipboard_guard_helper::health::HelperHealth;
use clipboard_guard_helper::ipc::SvcClient;
use clipboard_guard_helper::listener::{self, ClipboardChangeSource};
use clipboard_guard_helper::policy::PolicyStore;
use clipboard_guard_helper::read::{ClipboardTextSource, ReadOutcome, Win32TextSource};
use clipboard_guard_helper::report::build_report;
use wincmd_clip_rules::Action;

/// Placeholder written onto the clipboard by `QuarantineClipboard`.
/// Deliberately generic — never echoes anything from the flagged
/// clipboard content (plan §8's content-free discipline extends to this
/// literal string too).
const QUARANTINE_PLACEHOLDER: &str =
    "[WinCommander Clipboard Guard: content withheld pending review]";

/// Fallback poll interval when the event-driven listener can't register
/// (plan §4.2: "Keep a slow poll as a fallback").
const POLL_FALLBACK_INTERVAL: Duration = Duration::from_millis(2000);

/// How often to re-fetch policy from `commander-svc` even when nothing
/// else prompts it, so a published rule change reaches an already-running
/// helper without requiring a restart.
const POLICY_REFRESH_INTERVAL: Duration = Duration::from_secs(300);

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    rt.block_on(run());
}

async fn run() {
    let mut health = HelperHealth {
        helper_running: true,
        ..HelperHealth::default()
    };

    let svc = SvcClient::new();
    let mut policy_store = PolicyStore::new();
    refresh_policy(&svc, &mut policy_store, &mut health).await;

    let stop = Arc::new(AtomicBool::new(false));
    let (mut change_source, mode) =
        listener::start(POLL_FALLBACK_INTERVAL, stop.clone(), &mut health);
    eprintln!("[clipboard-guard] listener mode: {mode:?}");

    let mut text_source = Win32TextSource::default();
    let mut writer = Win32Writer::default();
    let mut notifier = NullNotifier;
    let mut engine = MatchEngine::new();
    let mut last_policy_refresh = Instant::now();

    loop {
        // Block on the next clipboard-change signal off the async
        // runtime's own worker thread — `wait_for_change` is a plain
        // blocking call (channel recv or a sleep-poll loop), never an
        // async fn, per `listener::ClipboardChangeSource`'s contract.
        let changed = tokio::task::block_in_place(|| change_source.wait_for_change());
        if !changed {
            // The change source was asked to stop. Nothing in this loop
            // requests that today (no shutdown signal is wired yet for
            // the standalone process), so this is currently unreachable
            // in practice — handled anyway so a future shutdown hook has
            // somewhere correct to land.
            break;
        }

        if last_policy_refresh.elapsed() >= POLICY_REFRESH_INTERVAL {
            refresh_policy(&svc, &mut policy_store, &mut health).await;
            last_policy_refresh = Instant::now();
        }

        let text = match text_source.read_text() {
            ReadOutcome::Text(text) => text,
            ReadOutcome::NoText => continue,
            ReadOutcome::Failed => {
                eprintln!("[clipboard-guard] clipboard read failed after retries");
                continue;
            }
        };

        let MatchOutcome::Emit {
            verdict,
            suppressed_since_last,
        } = engine.observe(policy_store.active(), &text, Instant::now())
        else {
            continue;
        };
        // `text` (and anything derived from it) is never touched again
        // past this point — everything below works from `verdict`, which
        // is structurally content-free (see `wincmd_clip_rules::Verdict`).
        drop(text);

        let mut action_outcome = execute_local_actions(
            &verdict.actions,
            &mut writer,
            &mut notifier,
            verdict.severity,
            QUARANTINE_PLACEHOLDER,
        );

        let touched_clipboard = verdict
            .actions
            .iter()
            .any(|a| matches!(a, Action::ClearClipboard | Action::QuarantineClipboard));
        if touched_clipboard {
            let verified = action_outcome
                .succeeded
                .iter()
                .any(|a| matches!(a, Action::ClearClipboard | Action::QuarantineClipboard));
            health.clear_failing = !verified;
        }

        let report = build_report(
            policy_store.active().policy_version,
            &verdict.rule_id,
            verdict.rule_revision,
            verdict.severity,
            &action_outcome,
            suppressed_since_last,
        );

        let submitted_ok = match svc.report_event(&report).await {
            Ok(()) => true,
            Err(err) => {
                eprintln!("[clipboard-guard] report_event failed: {err:?}");
                false
            }
        };
        health.svc_reachable = submitted_ok;
        // Local bookkeeping only — see `report::build_report`'s doc for
        // why this is never retransmitted as an amended report.
        mark_submission_actions(&verdict.actions, &mut action_outcome, submitted_ok);
    }
}

/// Fetch the current policy from `commander-svc` and install it. Failure
/// (svc absent, refusing, or an uncompilable ruleset) leaves whatever
/// policy was already active untouched — see `policy::PolicyStore::install`
/// — and is recorded honestly in `health` rather than crashing the loop.
async fn refresh_policy(
    svc: &SvcClient,
    policy_store: &mut PolicyStore,
    health: &mut HelperHealth,
) {
    match svc.get_policy().await {
        Ok(response) => {
            health.svc_reachable = true;
            match policy_store.install(&response) {
                Ok(()) => health.policy_current = true,
                Err(errors) => {
                    eprintln!("[clipboard-guard] policy install failed ({} error(s)); keeping previous ruleset", errors.len());
                    health.policy_current = false;
                }
            }
        }
        Err(err) => {
            eprintln!("[clipboard-guard] get_policy failed: {err:?}");
            health.svc_reachable = false;
            health.policy_current = false;
        }
    }
    health.rules_compiled = policy_store.rules_compiled();
}
