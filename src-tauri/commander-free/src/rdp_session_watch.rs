// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/rdp_session_watch.rs
//
// ═══════════════════════════════════════════════════════════════════════
// RDP SESSION-END VAULT DISMOUNT — native WTS session-change notification
// ═══════════════════════════════════════════════════════════════════════
//
// Problem: the existing Incoming-RDP dismount path
// (useRdpIncomingDismount.ts + Watch-RdpIncomingSessions) is a 10s poll
// running on the frontend. That is unreliable for exactly the case it
// exists to protect against — the LAST session on the box logging off.
// When the observing app instance IS that last session, Windows can tear
// the whole process down as part of session teardown before the next
// 10s poll tick ever runs, so the "someone left" transition is never
// observed. A per-user (--scope per-user) mount's drive letter already
// dies with the logon session in that scenario, but the DRIVER keeps the
// volume mounted and reachable by any other local user via
// DefineDosDevice (see wc-vault-test/reconcile-orphans.ps1, the boot-time
// sweep this module complements rather than replaces).
//
// Fix: register for native WM_WTSSESSION_CHANGE notifications on a
// dedicated message-only window, scoped to THIS session only
// (NOTIFY_FOR_THIS_SESSION). That message is delivered synchronously as
// part of the session's own teardown — before the poll would get another
// turn — so it closes the reliability gap above. Win32 calls the two
// events of interest WTS_SESSION_LOGOFF and WTS_REMOTE_DISCONNECT (the
// latter is what fires when an RDP client window is closed without
// signing off); there is no literal constant named "WTS_SESSION_DISCONNECT".
//
// This EXTENDS the existing path rather than adding a parallel one: on
// notification we re-run the identical "is anyone else still attended?"
// check the frontend hook makes (same Watch-RdpIncomingSessions command,
// excluding our own about-to-end session) and, only if nobody else
// remains, call the SAME Dismount-LocalVaults command the poll already
// calls. The 10s poll stays in place unchanged as the path for every
// scenario that ISN'T "my own session is ending" (e.g. observing another
// session drain while ours stays up).
//
// Shape follows services::keyboard_hook.rs: a dedicated OS thread owns a
// private Win32 message loop for the app's whole lifetime (WTS
// notifications are only delivered to the thread whose window is
// registered, and that thread must keep pumping messages). Unlike
// keyboard_hook, no class is registered — CreateWindowExW targets the
// predefined system "Static" class (needs no RegisterClassExW / WNDCLASSEXW,
// which would otherwise pull in the Win32_Graphics_Gdi feature for a
// single unused HBRUSH field) and GWLP_WNDPROC subclassing installs our
// handler after creation.

use std::collections::HashMap;
use std::sync::OnceLock;

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GWLP_WNDPROC, HWND_MESSAGE,
    MSG, SetWindowLongPtrW, TranslateMessage, WM_WTSSESSION_CHANGE, WTS_REMOTE_DISCONNECT,
    WTS_SESSION_LOGOFF,
};

/// AppHandle stashed for the window-proc callback (a plain `extern "system"`
/// fn — it can't capture anything) to reach `run_backend_script` from.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Arm the watcher. Call once from setup(), mirroring the
/// `session_instance::start_pipe_listener` call site: spawns a dedicated
/// OS thread with its own Win32 message loop, since WTS notifications
/// are only ever delivered to the thread that registered the window and
/// that thread must keep pumping messages for the process lifetime.
pub fn start(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
    if let Err(e) = std::thread::Builder::new()
        .name("wc-session-watch".to_string())
        .spawn(run_watch_thread)
    {
        crate::log_message_src(
            "warn",
            "core",
            &format!("[SessionWatch] failed to start watch thread: {}", e),
        );
    }
}

fn run_watch_thread() {
    // "Static" is a predefined system window class — no RegisterClassExW
    // needed. HWND_MESSAGE parents it as message-only (invisible, no
    // taskbar entry, no WS_VISIBLE required).
    let class_name = encode_wide("Static");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        crate::log_message_src(
            "warn",
            "core",
            "[SessionWatch] CreateWindowExW failed — session-end dismount disabled",
        );
        return;
    }

    // Subclass: install our handler in place of Static's default proc so
    // WM_WTSSESSION_CHANGE reaches wnd_proc. Nothing else about this
    // window is ever shown or interacted with, so there is no need to
    // preserve/forward to the original proc for any other message.
    // (Cast via an explicitly-typed fn pointer first — a direct fn-item-to-
    // integer cast is a clippy/rustc warning, and CI runs clippy -D warnings.)
    let proc_ptr: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = wnd_proc;
    // clippy::fn_to_numeric_cast_any wants the unsigned cast first.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc_ptr as usize as isize) };

    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        crate::log_message_src(
            "warn",
            "core",
            "[SessionWatch] WTSRegisterSessionNotification failed — session-end dismount disabled",
        );
        return;
    }
    crate::log_message_src(
        "info",
        "core",
        "[SessionWatch] armed — watching for this session's logoff/disconnect",
    );

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        // hwnd filter = 0 ("any window owned by this thread") — this
        // thread owns exactly the one window created above, same idiom
        // services::keyboard_hook.rs uses for its own message loop.
        let ret = unsafe { GetMessageW(&mut msg, 0 as HWND, 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_WTSSESSION_CHANGE {
        let reason = wparam as u32;
        if reason == WTS_SESSION_LOGOFF || reason == WTS_REMOTE_DISCONNECT {
            on_session_ending(reason);
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn on_session_ending(reason: u32) {
    let Some(app) = APP_HANDLE.get().cloned() else {
        return;
    };
    let label = if reason == WTS_SESSION_LOGOFF {
        "logoff"
    } else {
        "disconnect"
    };
    crate::log_message_src(
        "info",
        "core",
        &format!("[SessionWatch] this session is ending ({})", label),
    );
    tauri::async_runtime::spawn(async move {
        maybe_dismount_on_session_end(app, label).await;
    });
}

async fn maybe_dismount_on_session_end(app: AppHandle, label: &str) {
    // Same gate the frontend hook (App.tsx) applies before starting its
    // poll: both the master incoming-RDP toggle and "dismount on empty"
    // must be on. Keeps this native path strictly opt-in, matching the
    // existing feature's default-off behaviour.
    let armed = match crate::settings::read_settings() {
        Ok(settings) => {
            let rdp = &settings.ideal.tweaks.rdp;
            rdp.incoming_idle_timeout_enabled.unwrap_or(false)
                && rdp.incoming_dismount_on_empty.unwrap_or(false)
        }
        Err(e) => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[SessionWatch] settings read failed: {}", e),
            );
            false
        }
    };
    if !armed {
        return;
    }

    // Re-run the SAME "anyone else still attended?" check the 10s poll
    // makes, so one employee's own logoff/disconnect never dismounts a
    // vault other employees are actively using — only fire when NO OTHER
    // incoming RDP session remains attended once ours is excluded.
    let sessions = match crate::backend::run_backend_script(
        app.clone(),
        "Watch-RdpIncomingSessions".to_string(),
        HashMap::new(),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[SessionWatch] session query failed: {}", e),
            );
            return;
        }
    };

    if other_attended_session_remains(&sessions) {
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[SessionWatch] {} — another RDP session is still attended, not dismounting",
                label
            ),
        );
        return;
    }

    crate::log_message_src(
        "info",
        "core",
        &format!(
            "[SessionWatch] {} — no other attended session, dismounting local vaults",
            label
        ),
    );
    match crate::backend::run_backend_script(
        app,
        "Dismount-LocalVaults".to_string(),
        HashMap::new(),
    )
    .await
    {
        Ok(v) if v.get("success").and_then(|s| s.as_bool()) == Some(false) => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[SessionWatch] Dismount-LocalVaults reported failure: {:?}", v),
            );
        }
        Ok(_) => {}
        Err(e) => {
            crate::log_message_src("warn", "core", &format!("[SessionWatch] dismount failed: {}", e));
        }
    }
}

/// True if `payload` (the raw JSON `Watch-RdpIncomingSessions` returns —
/// `{ sessions, rawLines, queryError }`) contains an attended session that
/// ISN'T the one currently ending. A "Disc" session counts as unattended,
/// matching useRdpIncomingDismount.ts's ATTENDED_STATES set exactly — keep
/// the two in sync if that set ever changes.
fn other_attended_session_remains(payload: &serde_json::Value) -> bool {
    let raw = &payload["sessions"];
    // PowerShell's ConvertTo-Json collapses a single-element array to a
    // bare object — same normalisation useRdpIncomingDismount.ts applies.
    let list: Vec<&serde_json::Value> = if let Some(arr) = raw.as_array() {
        arr.iter().collect()
    } else if raw.is_object() {
        vec![raw]
    } else {
        Vec::new()
    };

    list.iter().any(|s| {
        let is_current = s["isCurrentSession"].as_bool().unwrap_or(false);
        if is_current {
            return false;
        }
        let state = s["state"].as_str().unwrap_or("").to_ascii_lowercase();
        matches!(state.as_str(), "active" | "conn" | "connected")
    })
}

#[cfg(test)]
mod tests {
    use super::other_attended_session_remains;
    use serde_json::json;

    #[test]
    fn no_sessions_means_nobody_else_remains() {
        let payload = json!({ "sessions": [], "rawLines": [], "queryError": "" });
        assert!(!other_attended_session_remains(&payload));
    }

    #[test]
    fn only_the_ending_session_present_is_not_someone_else() {
        let payload = json!({
            "sessions": [
                { "sessionId": 2, "state": "Active", "isCurrentSession": true }
            ]
        });
        assert!(!other_attended_session_remains(&payload));
    }

    #[test]
    fn another_active_session_blocks_dismount() {
        let payload = json!({
            "sessions": [
                { "sessionId": 2, "state": "Active", "isCurrentSession": true },
                { "sessionId": 3, "state": "Active", "isCurrentSession": false }
            ]
        });
        assert!(other_attended_session_remains(&payload));
    }

    #[test]
    fn another_disconnected_session_does_not_block_dismount() {
        // Matches useRdpIncomingDismount.ts: "Disc" is unattended, so an
        // abandoned-but-not-signed-off session must never block the
        // last-attended-user-leaves dismount.
        let payload = json!({
            "sessions": [
                { "sessionId": 2, "state": "Active", "isCurrentSession": true },
                { "sessionId": 3, "state": "Disc", "isCurrentSession": false }
            ]
        });
        assert!(!other_attended_session_remains(&payload));
    }

    #[test]
    fn single_element_array_collapsed_to_bare_object_is_handled() {
        let payload = json!({
            "sessions": { "sessionId": 3, "state": "Conn", "isCurrentSession": false }
        });
        assert!(other_attended_session_remains(&payload));
    }
}
