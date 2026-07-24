// src-tauri/commander-free/src/services/keyboard_hook.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SHARED SYSTEM-WIDE KEYBOARD HOOK
// ═══════════════════════════════════════════════════════════════════════
//
// Single Win32 `WH_KEYBOARD_LL` hook installed once and fanned out to
// any number of subscribers. Replaces:
//
//   - The per-module duplicate that lived inside `coercion_phrase.rs`
//     for phrase-matching. Now coercion_phrase is a thin consumer of
//     this service.
//   - The frontend `keydown` → `flow-key-press` Tauri event bridge that
//     `flow_engine.rs::listen_key_sequence` used. That bridge had two
//     bugs the shared hook eliminates:
//       1. WC had to be focused (the frontend keydown handler only
//          fires when the WebView has focus).
//       2. F12 was eaten by the WebView's DevTools shortcut before the
//          JS handler ever saw it — so the pre-shipped Contingency
//          system flow (F12 ×3) never actually fired.
//
// ── API
//
//   fn subscribe() -> KeyboardSubscription
//
// Returns a guard struct with an `rx` channel. Receive `KeyEvent`s
// until you drop the guard. When the LAST guard is dropped, the
// underlying Win32 hook is uninstalled (zero overhead when nothing
// is listening).
//
// ── KeyEvent shape
//
// Each event carries:
//   - `vk`             — raw Windows VK code (u32)
//   - `key_name`       — browser-`KeyboardEvent.key`-compatible string,
//                        e.g. "F12", "A", "Escape", "ArrowUp", " ".
//                        F12 unambiguously means the F12 function key;
//                        there is no "F1 + 1 + 2" interpretation. The
//                        only place this distinction matters is in the
//                        Flows KeySequenceTrigger config UI helper text.
//   - `normalized_char`— Lowercase ASCII (a-z / 0-9 / space) or None.
//                        Used by coercion-phrase's hashed-tail matcher.
//                        KeySequenceTrigger ignores this and matches on
//                        `key_name`.
//   - `is_commit`      — true for Enter / Tab / Escape. Tells
//                        coercion-phrase to flush its rolling buffer
//                        (phrase must be typed contiguously without a
//                        commit key in the middle).
//   - `is_backspace`   — true for Backspace. Tells coercion-phrase to
//                        pop one char off the buffer (lets the user
//                        mistype-then-correct without losing state).
//
// ── Privacy invariant
//
// The hook proc does NOT log keystrokes anywhere. It only constructs
// `KeyEvent` and sends to subscribers via in-memory mpsc channels.
// Subscribers may choose to do their own logging — that's their call,
// not ours.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use serde::Serialize;
use tokio::sync::mpsc;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

// ── KeyEvent ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct KeyEvent {
    pub vk: u32,
    /// `KeyboardEvent.key`-compatible name. Examples:
    ///   F12, A, Escape, Enter, Tab, Backspace, ArrowUp, " ", 0, 9.
    pub key_name: String,
    /// Lowercase ASCII normalised char (a-z, 0-9, or space) if the key
    /// is printable. None for function keys, arrows, etc.
    pub normalized_char: Option<char>,
    pub is_commit: bool,
    pub is_backspace: bool,
}

// ── Subscription ────────────────────────────────────────────────────

pub struct KeyboardSubscription {
    pub rx: mpsc::UnboundedReceiver<KeyEvent>,
    _drop_guard: SubscriptionGuard,
}

struct SubscriptionGuard;

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        // The corresponding sender is detected as closed (because the
        // rx field of KeyboardSubscription was dropped first per the
        // struct field declaration order). Sweep dead senders out and
        // uninstall if no live subscribers remain.
        let mut subs = SUBSCRIBERS.lock().unwrap();
        subs.retain(|tx| !tx.is_closed());
        let remaining = subs.len();
        drop(subs);
        if remaining == 0 {
            uninstall_hook();
        }
    }
}

/// Subscribe to system-wide keyboard events. Drop the returned struct
/// to unsubscribe. The hook installs on first subscribe and uninstalls
/// when the last subscriber drops — so callers that don't need the
/// hook running don't pay for it.
pub fn subscribe() -> KeyboardSubscription {
    let (tx, rx) = mpsc::unbounded_channel::<KeyEvent>();
    SUBSCRIBERS.lock().unwrap().push(tx);
    ensure_hook_installed();
    KeyboardSubscription {
        rx,
        _drop_guard: SubscriptionGuard,
    }
}

// ── Module state ────────────────────────────────────────────────────

static SUBSCRIBERS: Lazy<Mutex<Vec<mpsc::UnboundedSender<KeyEvent>>>> =
    Lazy::new(|| Mutex::new(Vec::new()));
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static HOOK_HANDLE: Mutex<isize> = Mutex::new(0);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
fn ensure_hook_installed() {
    if HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    // The Win32 low-level keyboard hook REQUIRES a message-pump loop on
    // the thread that installed it. We give it a dedicated thread so
    // the rest of the app doesn't have to think about it.
    let (install_tx, install_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
    std::thread::Builder::new()
        .name("wc-kbd-hook".to_string())
        .spawn(move || run_hook_thread(install_tx))
        .ok();
    // Block up to 3 s waiting for SetWindowsHookExW to either succeed or
    // tell us why it failed. Without this we'd race the first event.
    match install_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        Ok(Err(msg)) => {
            crate::log_message("error", &format!("[KeyboardHook] install failed: {}", msg));
        }
        Err(_) => {
            crate::log_message(
                "warn",
                "[KeyboardHook] install timed out — proceeding anyway, the hook may not be live",
            );
        }
    }
}

#[cfg(not(windows))]
fn ensure_hook_installed() {
    // Non-Windows: nothing to install. Subscribers will receive no
    // events. WC ships Windows-only today, so this only matters for
    // `cargo check` on CI that runs other targets.
}

#[cfg(windows)]
fn uninstall_hook() {
    let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
    if tid != 0 {
        // PostThreadMessage WM_QUIT unblocks the GetMessageW loop in
        // run_hook_thread, which then cleanly unhooks.
        unsafe { PostThreadMessageW(tid, WM_QUIT, 0, 0) };
    }
}

#[cfg(not(windows))]
fn uninstall_hook() {}

#[cfg(windows)]
fn run_hook_thread(install_signal: std::sync::mpsc::SyncSender<Result<(), String>>) {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    HOOK_THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0) };
    if hook.is_null() {
        let err = unsafe { GetLastError() };
        let msg = format!(
            "SetWindowsHookExW failed (GetLastError={}). Likely: anti-cheat / AV blocking low-level keyboard hooks, or another tool already owns the hook chain.",
            err
        );
        HOOK_INSTALLED.store(false, Ordering::SeqCst);
        let _ = install_signal.send(Err(msg));
        return;
    }
    *HOOK_HANDLE.lock().unwrap() = hook as isize;
    crate::log_message("debug", "[KeyboardHook] installed");
    let _ = install_signal.send(Ok(()));

    let mut msg = MSG {
        hwnd: 0 as _,
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
    };
    loop {
        let ret = unsafe { GetMessageW(&mut msg, 0 as _, 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let h = *HOOK_HANDLE.lock().unwrap();
    if h != 0 {
        unsafe { UnhookWindowsHookEx(h as HHOOK) };
        *HOOK_HANDLE.lock().unwrap() = 0;
    }
    HOOK_INSTALLED.store(false, Ordering::SeqCst);
    HOOK_THREAD_ID.store(0, Ordering::SeqCst);
    crate::log_message("debug", "[KeyboardHook] uninstalled");
}

// ── Hook proc ───────────────────────────────────────────────────────

#[cfg(windows)]
unsafe extern "system" fn hook_proc(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && (w_param == WM_KEYDOWN as WPARAM || w_param == WM_SYSKEYDOWN as WPARAM)
    {
        let kb = unsafe { *(l_param as *const KBDLLHOOKSTRUCT) };
        let vk = kb.vkCode;
        let event = KeyEvent {
            vk,
            key_name: vk_to_key_name(vk),
            normalized_char: vk_to_normalized_char(vk),
            is_commit: matches!(vk, VK_RETURN | VK_TAB | VK_ESCAPE),
            is_backspace: vk == VK_BACK,
        };
        // Fan out — clone is cheap (one String + ints). Drop dead
        // subscribers in the same pass so the list doesn't grow.
        let mut subs = SUBSCRIBERS.lock().unwrap();
        subs.retain(|tx| tx.send(event.clone()).is_ok());
    }
    unsafe { CallNextHookEx(0 as HHOOK, code, w_param, l_param) }
}

// ── VK → key name / char mappings ───────────────────────────────────
//
// `key_name` matches the browser `KeyboardEvent.key` spec wherever
// reasonable so that strings already used in flows ("F12", "Escape")
// continue to work end-to-end without translation.

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_PAGE_UP: u32 = 0x21;
const VK_PAGE_DOWN: u32 = 0x22;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_INSERT: u32 = 0x2D;
const VK_DELETE: u32 = 0x2E;

pub(crate) fn vk_to_key_name(vk: u32) -> String {
    match vk {
        // Letters A-Z. Uppercase per browser spec for the key identity
        // (case-folding is the consumer's problem).
        0x41..=0x5A => ((vk as u8) as char).to_string(),
        // Digits 0-9
        0x30..=0x39 => ((vk as u8) as char).to_string(),
        // F1-F24 — F-keys are the most common flow-trigger choice
        // (Contingency uses F12 ×3). Browser spec is "F1", "F2", ... "F24".
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        VK_RETURN => "Enter".to_string(),
        VK_TAB => "Tab".to_string(),
        VK_ESCAPE => "Escape".to_string(),
        VK_BACK => "Backspace".to_string(),
        // Browser spec: space.key is " " (a single space char), not
        // "Space". Match that so the configured key string is portable.
        VK_SPACE => " ".to_string(),
        VK_LEFT => "ArrowLeft".to_string(),
        VK_UP => "ArrowUp".to_string(),
        VK_RIGHT => "ArrowRight".to_string(),
        VK_DOWN => "ArrowDown".to_string(),
        VK_PAGE_UP => "PageUp".to_string(),
        VK_PAGE_DOWN => "PageDown".to_string(),
        VK_HOME => "Home".to_string(),
        VK_END => "End".to_string(),
        VK_INSERT => "Insert".to_string(),
        VK_DELETE => "Delete".to_string(),
        // Modifier keys — Shift / Ctrl / Alt / Win. Browser spec.
        0xA0 | 0xA1 | 0x10 => "Shift".to_string(),
        0xA2 | 0xA3 | 0x11 => "Control".to_string(),
        0x12 | 0xA4 | 0xA5 => "Alt".to_string(),
        0x5B | 0x5C => "Meta".to_string(), // Win key
        // Unmapped — fallback. Allows debugging "what VK is this key?"
        // without crashing.
        _ => format!("VK#0x{:02X}", vk),
    }
}

pub(crate) fn vk_to_normalized_char(vk: u32) -> Option<char> {
    match vk {
        // Letters → lowercase
        0x41..=0x5A => Some(((vk + 0x20) as u8) as char),
        // Digits
        0x30..=0x39 => Some((vk as u8) as char),
        VK_SPACE => Some(' '),
        _ => None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_keys_map_to_fn_strings() {
        assert_eq!(vk_to_key_name(0x70), "F1");
        assert_eq!(vk_to_key_name(0x7B), "F12");
        assert_eq!(vk_to_key_name(0x87), "F24");
    }

    #[test]
    fn letters_uppercase() {
        assert_eq!(vk_to_key_name(0x41), "A");
        assert_eq!(vk_to_key_name(0x5A), "Z");
    }

    #[test]
    fn digits_pass_through() {
        assert_eq!(vk_to_key_name(0x30), "0");
        assert_eq!(vk_to_key_name(0x39), "9");
    }

    #[test]
    fn specials_match_browser_spec() {
        assert_eq!(vk_to_key_name(0x0D), "Enter");
        assert_eq!(vk_to_key_name(0x09), "Tab");
        assert_eq!(vk_to_key_name(0x1B), "Escape");
        assert_eq!(vk_to_key_name(0x08), "Backspace");
        assert_eq!(vk_to_key_name(0x20), " ");
        assert_eq!(vk_to_key_name(0x26), "ArrowUp");
    }

    #[test]
    fn normalized_lowercase_for_letters() {
        assert_eq!(vk_to_normalized_char(0x41), Some('a'));
        assert_eq!(vk_to_normalized_char(0x5A), Some('z'));
    }

    #[test]
    fn normalized_none_for_function_keys() {
        // Coercion-phrase shouldn't accidentally treat F12 as a typed char.
        assert_eq!(vk_to_normalized_char(0x7B), None);
        assert_eq!(vk_to_normalized_char(0x1B), None);
    }

    #[test]
    fn unknown_vk_falls_back_to_hex_label() {
        // Useful for "what's this key?" debugging without crashing.
        let s = vk_to_key_name(0x99);
        assert!(s.starts_with("VK#0x"));
    }
}
