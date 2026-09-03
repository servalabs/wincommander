// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/lockdown_words.rs
//
// ═══════════════════════════════════════════════════════════════════════
// COERCION CODE-PHRASE TRIGGER (F-5) — Pro feature
// ═══════════════════════════════════════════════════════════════════════
//
// A-4 module 3 evaluation: this file stays in Free.
//
// The watcher subscribes to crate::services::keyboard_hook, which is
// the same shared system-wide WH_KEYBOARD_LL hook used by the Flows
// engine's KeySequenceTrigger (e.g. the F12 ×3 contingency hotkey).
// Migrating coercion_phrase to Pro would require either:
//   (a) Moving the keyboard hook to Pro — would break Flows in Free,
//   (b) Forwarding every keystroke from Free to Pro over IPC — bad
//       latency for a duress trigger that must fire reliably and
//       silently when the user types the phrase. The buffer + hash
//       compare loop runs per-keystroke; adding a Free→Pro round-trip
//       on each character would make false-negatives (missed
//       phrases under load) and false-positives (delayed commits
//       finding stale buffers) both more likely.
//
// The Pro-tier gate is enforced by license::require_paid in
// start_coercion_phrase and register_coercion_phrase below; a
// non-licensed user can't arm the watcher. The "paid" semantics are
// preserved without moving the watcher itself.
//
// License header flipped to AGPL-3.0 to match the Free tier's
// post-Apache-2.0 relicensing (see commit 2b240b6).
//
// Silent, hands-on duress mode. The user pre-registers one or more
// phrases ("she sells seashells", "my dog ate the leftovers", etc.).
// When any of them is typed system-wide — into a chat app, browser
// address bar, Notepad, Word, anywhere — the panic flow fires
// silently. No UI, no countdown, no notification. The duress-er
// thinks the user is just typing a normal message; meanwhile WC is
// already invoking lockdown.
//
// Privacy guarantees:
//   - Phrases are NEVER stored in plaintext. Only SHA-256 digests
//     (with a per-app salt) are persisted. Comparing against the
//     keyboard-buffer tail is also constant-time via subtle::ConstantTimeEq.
//   - The shared keyboard hook (services::keyboard_hook) does NOT log
//     keystrokes anywhere. We consume KeyEvents and update an in-memory
//     buffer; nothing persists to disk.
//   - Buffer is restricted to lowercase ASCII a-z / 0-9 / space — non-
//     ASCII text never gets buffered (filtered by the hook's
//     `normalized_char`).
//
// Threat model:
//   The duress-er is watching the user type. The user types the
//   phrase as part of an innocuous message; the panic fires
//   invisibly. There is no visible UI feedback.
//
// Implementation (post-refactor 2026-05):
//   - The system-wide WH_KEYBOARD_LL hook is owned by
//     `crate::services::keyboard_hook`. This module subscribes via
//     `keyboard_hook::subscribe()` and consumes `KeyEvent`s. Previously
//     the hook lived here; it moved out because Flows
//     KeySequenceTrigger needed to share it (otherwise F12 ×3
//     couldn't fire the Contingency system flow).
//   - The consumer task lives for the lifetime of the start command.
//     When stop is called the task is aborted; the keyboard-hook
//     subscription drops with it; the shared service uninstalls the
//     Win32 hook iff no other module is subscribed (e.g. Flows).
//
// Pricing: PAID. Lockdown is admin + irreversible; this is a
// trigger for it. Free users see a TierGate.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tauri::{AppHandle, Emitter};

// ── Salt ────────────────────────────────────────────────────────────
//
// Fixed app-level salt. Per-device salt would defend against rainbow-
// table attacks across machines, but the threat model here is local
// observation — the registered hash list is visible to anyone with
// access to the settings file regardless of salt; the salt only
// matters for "given the hash list, can I crack the phrase". A
// strong fixed salt + minimum phrase length already makes that
// impractical.

const SALT: &[u8] = b"WC-F5-coercion-phrase-salt-v1-do-not-rename";
const BUFFER_MAX: usize = 64;
const SNOOZE_AFTER_FIRE_SECS: u64 = 60;

// Rolling-window tail lengths to test. Phrases must be at least 6
// chars to register; we hash every reasonable tail length up to 32.
const CANDIDATE_LENGTHS: &[usize] = &[6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32];

// ── State ───────────────────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);

static KEY_BUFFER: Lazy<Mutex<VecDeque<char>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(BUFFER_MAX)));
static REGISTERED: Lazy<Mutex<Vec<RegisteredPhrase>>> = Lazy::new(|| Mutex::new(Vec::new()));
/// Snooze timer for the COERCION path only. Distress has its own timer
/// (LAST_FIRE_DISTRESS) so that a coercion-phrase fire can never suppress a
/// genuine distress-phrase fire (or vice-versa) — sharing one timer meant a
/// coercion match snoozed the higher-priority distress signal for up to a
/// minute, silently swallowing an emergency destroy/decoy trigger.
static LAST_FIRE: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));
static LAST_FIRE_DISTRESS: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));
/// Set during start so handle_match can emit Tauri events. Cleared on stop.
static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);
/// Handle to the spawned consumer task so stop can abort it. Dropping
/// it implicitly drops the keyboard-hook subscription inside.
static CONSUMER_TASK: Lazy<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPhrase {
    /// User-visible label so the user can recognise which phrase is
    /// which without seeing the plaintext. Free-form, can be empty.
    pub label: String,
    /// Hex-encoded SHA-256(SALT || phrase_normalized).
    pub hash: String,
    /// Length of the phrase in normalised characters. Stored so the
    /// matcher only hashes tail-of-this-length, not all candidate
    /// lengths — small CPU win, tiny information leak (an attacker
    /// who steals the settings file learns phrase lengths but not
    /// content).
    pub length: usize,
}

#[derive(Debug, Serialize, Clone)]
struct CoercionFiredEvent {
    /// Label of the matched phrase (so the frontend can choose which
    /// flow to fire if multiple are registered to different actions).
    label: String,
    detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistressPhrase {
    pub label: String,
    pub hash: String,
    pub length: usize,
    /// "decoy" | "destroy"
    pub mode: String,
}

static DISTRESS_REGISTERED: Lazy<Mutex<Vec<DistressPhrase>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Serialize, Clone)]
struct DistressFiredEvent {
    label: String,
    mode: String,
    detected_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistressPhraseMatch {
    mode: String,
}

// ── Hashing ─────────────────────────────────────────────────────────

fn normalise_char(c: char) -> Option<char> {
    let lc = c.to_ascii_lowercase();
    if lc.is_ascii_alphanumeric() || lc == ' ' {
        Some(lc)
    } else {
        None
    }
}

fn normalise_phrase(s: &str) -> String {
    s.chars().filter_map(normalise_char).collect()
}

fn hash_phrase(normalised: &str) -> String {
    let mut h = Sha256::new();
    h.update(SALT);
    h.update(normalised.as_bytes());
    let result: [u8; 32] = h.finalize().into();
    hex::encode(result)
}

fn hashes_match_ct(a: &str, b: &str) -> bool {
    // Both are hex-encoded SHA-256 (64 chars). Constant-time compare.
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

// ── Match check ─────────────────────────────────────────────────────

fn check_match() -> Option<String> {
    let buffer = KEY_BUFFER.lock().unwrap();
    let buffer_str: String = buffer.iter().collect();
    drop(buffer);

    let registered = REGISTERED.lock().unwrap().clone();
    if registered.is_empty() {
        return None;
    }

    // Optimisation: only test tail lengths that match a registered
    // phrase length. Saves hashing 11 candidate lengths per keystroke
    // when the user has only a couple of phrases registered.
    let mut lengths: Vec<usize> = registered
        .iter()
        .map(|r| r.length)
        .filter(|&n| (6..=BUFFER_MAX).contains(&n))
        .collect();
    lengths.sort_unstable();
    lengths.dedup();
    // Fall back to canonical set if something weird is registered.
    if lengths.is_empty() {
        lengths.extend_from_slice(CANDIDATE_LENGTHS);
    }

    for length in lengths {
        if buffer_str.chars().count() < length {
            continue;
        }
        // Use char_indices so we slice on a UTF-8 boundary (the buffer
        // is restricted to ASCII so each char is 1 byte, but be
        // defensive).
        let suffix: String = buffer_str
            .chars()
            .rev()
            .take(length)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let candidate_hash = hash_phrase(&suffix);
        for r in &registered {
            if r.length == length && hashes_match_ct(&r.hash, &candidate_hash) {
                return Some(r.label.clone());
            }
        }
    }
    None
}

fn handle_match(label: String) {
    // Snooze guard so a held-down match (or accidental re-type) doesn't
    // re-fire panic 50 times.
    let now = Instant::now();
    {
        let mut last = LAST_FIRE.lock().unwrap();
        if let Some(prev) = *last {
            if now.duration_since(prev).as_secs() < SNOOZE_AFTER_FIRE_SECS {
                return;
            }
        }
        *last = Some(now);
    }

    // Clear the buffer so the same keystrokes don't re-fire next keypress.
    KEY_BUFFER.lock().unwrap().clear();

    // Emit silently — no toast, no notification. The frontend listener
    // invokes self_destruct directly. The duress-er sees nothing.
    let app_opt = APP_HANDLE.lock().unwrap().clone();
    if let Some(app) = app_opt {
        let payload = CoercionFiredEvent {
            label,
            detected_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = app.emit("coercion-phrase-fired", &payload);
        crate::authz::execute_trusted_lockdown(app);
    }
    // Intentionally NO log_message — the duress-er could later see the
    // log file as evidence of what fired. Log a generic message instead.
    crate::log_message("info", "[CoercionPhrase] trigger fired");
}

fn check_distress_match() -> Option<DistressPhrase> {
    let buffer = KEY_BUFFER.lock().unwrap();
    let buf_str: String = buffer.iter().collect();
    drop(buffer);
    let registered = DISTRESS_REGISTERED.lock().unwrap().clone();
    if registered.is_empty() {
        return None;
    }
    let mut lengths: Vec<usize> = registered
        .iter()
        .map(|r| r.length)
        .filter(|&n| (6..=BUFFER_MAX).contains(&n))
        .collect();
    lengths.sort_unstable();
    lengths.dedup();
    if lengths.is_empty() {
        lengths.extend_from_slice(CANDIDATE_LENGTHS);
    }
    for length in lengths {
        if buf_str.chars().count() < length {
            continue;
        }
        let suffix: String = buf_str
            .chars()
            .rev()
            .take(length)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let candidate = hash_phrase(&suffix);
        for r in &registered {
            if r.length == length && hashes_match_ct(&r.hash, &candidate) {
                return Some(r.clone());
            }
        }
    }
    None
}

fn handle_distress_match(phrase: DistressPhrase) {
    let now = Instant::now();
    {
        // Distress uses its OWN snooze timer (never the coercion one) so a
        // recent coercion fire can't suppress a genuine distress trigger.
        let mut last = LAST_FIRE_DISTRESS.lock().unwrap();
        if let Some(prev) = *last {
            if now.duration_since(prev).as_secs() < SNOOZE_AFTER_FIRE_SECS {
                return;
            }
        }
        *last = Some(now);
    }
    KEY_BUFFER.lock().unwrap().clear();
    // Arm the backend decoy backstop IN-PROCESS before emitting, so the real
    // config is write-protected even if the frontend never completes the
    // round-trip. A typed distress phrase is the most likely under-coercion
    // path (the app is already unlocked), so the guarantee must not depend on
    // the WebView reacting to the event.
    if phrase.mode == "decoy" {
        crate::settings::set_decoy_mode(true);
    }

    // F6 reboot-to-USB wipe: fire the orchestrator in a background task.
    // The orchestrator is NOT a freely-callable IPC command — it is reachable
    // only through this gated distress path.  See SAFETY CONTRACT in
    // f6_orchestrator.rs.
    if phrase.mode == "reboot_usb" {
        tauri::async_runtime::spawn(async {
            // Read arming flags fresh from settings at fire time.
            let (enabled, rtu_enabled) = crate::settings::read_settings()
                .map(|s| {
                    let sd = &s.ideal.privacy.self_destruct;
                    (sd.enabled, sd.reboot_to_usb_enabled)
                })
                .unwrap_or((None, None));

            match crate::f6_orchestrator::build_production_deps() {
                Ok(deps) => {
                    match crate::f6_orchestrator::execute_reboot_to_usb_wipe(
                        enabled,
                        rtu_enabled,
                        deps,
                    ) {
                        Ok(_) => {
                            crate::log_message(
                                "info",
                                "[Distress] F6 reboot-to-USB wipe completed",
                            );
                        }
                        Err(e) => {
                            crate::log_message(
                                "error",
                                &format!("[Distress] F6 reboot-to-USB wipe failed: {e}"),
                            );
                        }
                    }
                }
                Err(e) => {
                    crate::log_message(
                        "error",
                        &format!("[Distress] F6 build_production_deps failed: {e}"),
                    );
                }
            }
        });
    }

    if let Some(app) = APP_HANDLE.lock().unwrap().clone() {
        let payload = DistressFiredEvent {
            label: phrase.label.clone(),
            mode: phrase.mode.clone(),
            detected_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = app.emit("distress-phrase-fired", &payload);
        if phrase.mode == "destroy" {
            crate::authz::execute_trusted_lockdown(app);
        }
    }
    crate::log_message("info", "[Distress] trigger fired");
}

// ── Test-fire helper ────────────────────────────────────────────────
//
// Manually emits the panic-trigger-test event. Useful for the user to
// confirm the wiring works end-to-end without actually typing their
// phrase. Doesn't fire the lockdown flow — emits a separate "test"
// event the UI explicitly subscribes to. Stays free / unprivileged —
// safe to call while debugging.
#[tauri::command]
pub async fn test_fire_lockdown_words(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    let payload = serde_json::json!({
        "source": "panic_phrase",
        "detected_at": chrono::Utc::now().to_rfc3339(),
    });
    app.emit("panic-trigger-test", &payload)
        .map_err(|e| format!("emit failed: {}", e))?;
    Ok(())
}

// ── Tauri command surface ───────────────────────────────────────────

#[tauri::command]
pub async fn start_lockdown_words(app: AppHandle) -> Result<(), String> {
    crate::license::require_paid("lockdown words trigger")?;

    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(()); // idempotent
    }

    *APP_HANDLE.lock().unwrap() = Some(app);
    KEY_BUFFER.lock().unwrap().clear();
    *LAST_FIRE.lock().unwrap() = None;
    *LAST_FIRE_DISTRESS.lock().unwrap() = None;

    // Spawn the consumer task. It subscribes to the shared keyboard
    // service; the service installs the WH_KEYBOARD_LL hook iff this
    // is the first subscriber (otherwise reuses the existing one
    // shared with e.g. the Flows KeySequenceTrigger listener).
    //
    // When stop is called we abort this task, which drops the
    // subscription, which the service then uses to decide whether to
    // uninstall the Win32 hook.
    let task = tauri::async_runtime::spawn(async move {
        let mut sub = crate::services::keyboard_hook::subscribe();
        crate::log_message("debug", "[CoercionPhrase] consumer started");

        while RUNNING.load(Ordering::SeqCst) {
            let event = match sub.rx.recv().await {
                Some(e) => e,
                None => break, // hook uninstalled out from under us
            };

            if event.is_commit {
                // Enter / Tab / Escape — phrase has to be typed contiguously
                // without a commit key. Avoids accidental fires from earlier
                // text that happens to contain the substring.
                KEY_BUFFER.lock().unwrap().clear();
                continue;
            }
            if event.is_backspace {
                // Backspace — pop the last char. Lets the user
                // mistype-then-correct without losing buffer state.
                let mut buf = KEY_BUFFER.lock().unwrap();
                buf.pop_back();
                continue;
            }
            if let Some(c) = event.normalized_char {
                {
                    let mut buf = KEY_BUFFER.lock().unwrap();
                    if buf.len() == BUFFER_MAX {
                        buf.pop_front();
                    }
                    buf.push_back(c);
                }
                if let Some(label) = check_match() {
                    handle_match(label);
                }
                if let Some(phrase) = check_distress_match() {
                    handle_distress_match(phrase);
                }
            }
            // Function keys, arrows, modifiers, etc. → ignored. The
            // hook's `normalized_char` is None for those — they don't
            // belong in a phrase buffer.
        }

        crate::log_message("debug", "[CoercionPhrase] consumer stopped");
    });
    *CONSUMER_TASK.lock().unwrap() = Some(task);

    Ok(())
}

#[tauri::command]
pub async fn stop_lockdown_words() -> Result<(), String> {
    if !RUNNING.swap(false, Ordering::SeqCst) {
        return Ok(()); // already stopped
    }

    if let Some(task) = CONSUMER_TASK.lock().unwrap().take() {
        task.abort();
    }

    KEY_BUFFER.lock().unwrap().clear();
    *APP_HANDLE.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub async fn lockdown_words_status() -> Result<bool, String> {
    Ok(RUNNING.load(Ordering::SeqCst))
}

/// Hashes the plaintext phrase and registers the digest. Plaintext
/// is dropped at function exit; nothing persists to disk.
///
/// Returns the new RegisteredPhrase the frontend should add to
/// settings (it has to know the hash + length to render the UI row).
#[tauri::command]
pub async fn register_lockdown_words(
    label: String,
    plaintext: String,
) -> Result<RegisteredPhrase, String> {
    crate::license::require_paid("lockdown words trigger")?;

    let normalised = normalise_phrase(&plaintext);
    if normalised.chars().count() < 6 {
        return Err("Phrase must be at least 6 characters (a-z, 0-9, spaces — other characters are ignored).".to_string());
    }
    if normalised.chars().count() > BUFFER_MAX {
        return Err(format!(
            "Phrase too long ({} chars after normalisation; max {}).",
            normalised.chars().count(),
            BUFFER_MAX
        ));
    }
    let hash = hash_phrase(&normalised);
    let length = normalised.chars().count();

    let entry = RegisteredPhrase {
        label: if label.trim().is_empty() {
            format!("Phrase #{}", REGISTERED.lock().unwrap().len() + 1)
        } else {
            label.trim().to_string()
        },
        hash: hash.clone(),
        length,
    };

    // Reject duplicate hashes (same phrase registered twice).
    {
        let mut reg = REGISTERED.lock().unwrap();
        if reg.iter().any(|r| r.hash == hash) {
            return Err("That phrase is already registered.".to_string());
        }
        reg.push(entry.clone());
    }
    Ok(entry)
}

/// Replaces the registered set wholesale — frontend calls this on
/// every settings change to sync the runtime state.
#[tauri::command]
pub async fn set_lockdown_words(phrases: Vec<RegisteredPhrase>) -> Result<(), String> {
    *REGISTERED.lock().unwrap() = phrases;
    Ok(())
}

#[tauri::command]
pub async fn list_lockdown_words() -> Result<Vec<RegisteredPhrase>, String> {
    Ok(REGISTERED.lock().unwrap().clone())
}

// ── Distress phrase commands ────────────────────────────────────────

#[tauri::command]
pub async fn register_distress_phrase(
    mode: String,
    label: String,
    plaintext: String,
) -> Result<DistressPhrase, String> {
    crate::license::require_paid("distress phrase")?;
    if mode != "decoy" && mode != "destroy" && mode != "reboot_usb" {
        return Err(format!("Unknown mode: {mode}"));
    }
    let normalised = normalise_phrase(&plaintext);
    if normalised.chars().count() < 6 {
        return Err("Phrase must be at least 6 characters.".into());
    }
    if normalised.chars().count() > BUFFER_MAX {
        return Err(format!("Phrase too long (max {BUFFER_MAX} chars)."));
    }
    let hash = hash_phrase(&normalised);
    let length = normalised.chars().count();
    let entry = DistressPhrase {
        label: if label.trim().is_empty() {
            format!("Phrase #{}", DISTRESS_REGISTERED.lock().unwrap().len() + 1)
        } else {
            label.trim().to_string()
        },
        hash: hash.clone(),
        length,
        mode,
    };
    {
        let mut reg = DISTRESS_REGISTERED.lock().unwrap();
        if reg.iter().any(|r| r.hash == hash) {
            return Err("That phrase is already registered.".into());
        }
        reg.push(entry.clone());
    }
    Ok(entry)
}

#[tauri::command]
pub async fn set_distress_phrases(phrases: Vec<DistressPhrase>) -> Result<(), String> {
    *DISTRESS_REGISTERED.lock().unwrap() = phrases;
    Ok(())
}

#[tauri::command]
pub async fn list_distress_phrases() -> Result<Vec<DistressPhrase>, String> {
    Ok(DISTRESS_REGISTERED.lock().unwrap().clone())
}

/// Check a phrase against the distress registry — returns "decoy"|"destroy" or null.
/// Called by the command palette on Enter so hashes never leave Rust.
#[tauri::command]
pub async fn check_distress_phrase(
    app: tauri::AppHandle,
    phrase: String,
) -> Result<Option<DistressPhraseMatch>, String> {
    let normalised = normalise_phrase(&phrase);
    for r in DISTRESS_REGISTERED.lock().unwrap().iter() {
        if r.length == normalised.chars().count()
            && hashes_match_ct(&r.hash, &hash_phrase(&normalised))
        {
            let mode = r.mode.clone();
            if mode == "destroy" {
                crate::authz::execute_trusted_lockdown(app);
            }
            return Ok(Some(DistressPhraseMatch { mode }));
        }
    }
    Ok(None)
}
