// src-tauri/commander-free/src/session_instance.rs
//
// ═══════════════════════════════════════════════════════════════════════════
// PER-SESSION SINGLE-INSTANCE GUARD
// ═══════════════════════════════════════════════════════════════════════════
//
// Problem tauri-plugin-single-instance solved poorly for Windows Server /
// Remote Desktop hosts:
//
//   The plugin creates a named mutex "{bundle-id}-sim".  Although Windows
//   automatically scopes un-prefixed objects to the calling process's logon
//   session, the hidden event window the plugin creates uses FindWindowW,
//   which on some Windows Server + RDS configurations can find windows across
//   sessions.  The net result is that User A's running instance prevents
//   User B from launching the app at all.
//
// This module replaces the plugin with an implementation that:
//
//   • Embeds the Windows Terminal Services session ID in EVERY kernel object
//     name (mutex + named pipe).  This is belt-AND-suspenders: explicit
//     session scoping on top of the OS's default session-namespace routing.
//
//   • Allows each RDP / TS user to run their own independent instance.
//
//   • Preserves the single-instance guarantee WITHIN a session (a user
//     double-launching the app focuses the existing window instead of
//     opening a second one).
//
//   • Preserves full arg forwarding so context-menu launches (--scrub,
//     --shred) still reach the already-running instance.
//
// ── Lifecycle ───────────────────────────────────────────────────────────────
//
//   run() calls acquire() BEFORE the Tauri builder:
//     Primary instance   → owns mutex, returns true,  startup continues.
//     Duplicate instance → forwards args via pipe, returns false → exit(0).
//
//   setup() calls start_pipe_listener(app_handle):
//     Async Tokio loop that accepts one forwarded-args message per connection,
//     focuses the main window, and emits the appropriate Tauri event.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;

use tauri::{Emitter, Manager};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, STILL_ACTIVE},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            CreateMutexW, GetCurrentProcessId, GetExitCodeProcess, OpenProcess, ReleaseMutex,
            TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_TERMINATE,
        },
    },
};

/// Mutex HANDLE kept alive for the whole process lifetime so it stays owned.
static MUTEX_HANDLE: OnceLock<isize> = OnceLock::new();

/// Set true once setup() has finished and the window+frontend can accept a
/// reveal. Until then, forwarded args are queued instead of acted on, so a
/// hotkey/reopen relaunch during a slow cold-start neither no-ops nor races.
static APP_READY: AtomicBool = AtomicBool::new(false);
/// Payloads forwarded over the pipe before the app was ready, replayed on ready.
static PENDING_FORWARDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn is_app_ready() -> bool {
    APP_READY.load(Ordering::SeqCst)
}

/// Called at the very end of setup(). Marks ready and drains queued forwards.
pub fn set_app_ready(app: &tauri::AppHandle) {
    APP_READY.store(true, Ordering::SeqCst);
    let queued: Vec<String> = {
        let mut g = PENDING_FORWARDS.lock().unwrap();
        std::mem::take(&mut *g)
    };
    for payload in queued {
        handle_forwarded_args(app, &payload);
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns the Windows Terminal Services session ID for the current process.
/// On a workstation this is typically 1; on RDP servers each user gets a
/// unique value (e.g. 2, 3, …).
pub fn current_session_id() -> u32 {
    let mut sid: u32 = 0;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) };
    sid
}

/// Named-pipe path for arg forwarding in the given session.
/// Named pipes are in a GLOBAL namespace on Windows, so we embed the session
/// ID to give each user their own private channel.
pub fn pipe_path(sid: u32) -> String {
    format!(r"\\.\pipe\WinCommander_S{}_args", sid)
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Attempt to become the primary instance for this Windows logon session.
///
/// Returns `true`  → caller is the primary; proceed with normal startup.
/// Returns `false` → another instance already owns this session; args have
///                   been forwarded; caller should call `std::process::exit(0)`.
pub fn acquire(cli_args: &[String]) -> bool {
    let sid = current_session_id();

    // The name has NO "Global\" prefix (stays in the session-local object
    // directory) AND contains the session ID for belt-and-suspenders safety.
    let mutex_name = encode_wide(&format!("WinCommander_S{}_lock", sid));

    let hmutex = unsafe {
        CreateMutexW(
            std::ptr::null(), // default security — accessible by same user only
            1,                // bInitialOwner = TRUE
            mutex_name.as_ptr(),
        )
    };

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // Another instance is already running in this session.
        // Decide what — if anything — to forward to the primary:
        //
        //  • Startup duplicate (HKCU\Run or HKLM\Run fired alongside each
        //    other at login): args are exactly ["--minimized"] or empty.
        //    Forward nothing — the primary is already running tray-only and
        //    must stay silent.
        //  • Bare double-click (no args at all): user explicitly wants to
        //    open the window. Send --focus so handle_forwarded_args shows it.
        //  • Context-menu launch (--shred / --scrub + paths): forward as-is
        //    so the primary can handle the file operation.
        //
        // The key rule: --minimized is a startup flag, NOT a user-intent
        // signal. Forwarding it caused handle_forwarded_args to call
        // win.show() on every login, defeating tray-only silent startup.
        let had_startup_flag = cli_args[1..].iter().any(|a| a == "--minimized");
        let meaningful: Vec<String> = cli_args[1..]
            .iter()
            .filter(|a| a.as_str() != "--minimized")
            .cloned()
            .collect();

        let payload = if !meaningful.is_empty() {
            // Has real args (paths, --shred, --scrub) — forward them.
            Some(meaningful)
        } else if !had_startup_flag {
            // Bare double-click: no args at all → ask primary to show window.
            Some(vec!["--focus".to_string()])
        } else {
            // --minimized only → startup duplicate → forward nothing.
            None
        };

        if let Some(args) = payload {
            // KT: generous 3s timeout so a busy-but-alive primary is never
            // falsely declared dead. Only after exhausting it do we check PID.
            let (delivered, primary_pid) = forward_args_with_liveness(sid, &args);
            if !delivered {
                crate::log_message_src(
                    "warn",
                    "core",
                    &format!(
                        "[SessionInstance] primary did not answer pipe within 3s \
                        (stored pid={:?}) — checking liveness",
                        primary_pid
                    ),
                );
                unsafe { CloseHandle(hmutex) };

                let can_takeover = match primary_pid {
                    None => {
                        // No stored PID — conservatively decline takeover.
                        crate::log_message_src(
                            "warn",
                            "core",
                            "[SessionInstance] no stored PrimaryPid; cannot verify — aborting takeover",
                        );
                        false
                    }
                    Some(pid) => {
                        if is_pid_alive(pid) {
                            // Alive but not answering the pipe — hung zombie. Kill it.
                            crate::log_message_src(
                                "warn",
                                "core",
                                &format!(
                                    "[SessionInstance] PID {} alive but pipe-silent — terminating hung zombie",
                                    pid
                                ),
                            );
                            kill_and_wait(pid);
                            true
                        } else {
                            // Already dead — OS will release its mutex shortly.
                            crate::log_message_src(
                                "info",
                                "core",
                                &format!(
                                    "[SessionInstance] PID {} is dead — proceeding with takeover",
                                    pid
                                ),
                            );
                            true
                        }
                    }
                };

                if !can_takeover {
                    return false;
                }

                // Brief yield so the OS releases the exited/killed process's mutex.
                std::thread::sleep(std::time::Duration::from_millis(300));
                let hmutex2 = unsafe { CreateMutexW(std::ptr::null(), 1, mutex_name.as_ptr()) };
                if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS && !hmutex2.is_null() {
                    crate::log_message_src(
                        "info",
                        "core",
                        "[SessionInstance] acquired mutex after zombie takeover — we are the new primary",
                    );
                    persist_primary_pid();
                    MUTEX_HANDLE.set(hmutex2 as isize).ok();
                    return true;
                }
                unsafe { CloseHandle(hmutex2) };
                crate::log_message_src(
                    "warn",
                    "core",
                    "[SessionInstance] takeover mutex re-create failed — another instance beat us",
                );
                return false;
            }
        }

        unsafe { CloseHandle(hmutex) };
        return false;
    }

    // We are the primary instance — keep the handle alive for our lifetime.
    // Leaking via OnceLock<isize> is intentional: the OS releases the mutex
    // automatically when the process exits (we also call release() on Exit).
    persist_primary_pid();
    MUTEX_HANDLE.set(hmutex as isize).ok();
    true
}

/// Release the session mutex.  Call from the Tauri Exit event handler or
/// at any clean-shutdown point.  Idempotent — safe to call multiple times.
pub fn release() {
    if let Some(&h) = MUTEX_HANDLE.get() {
        unsafe {
            ReleaseMutex(h as _);
            CloseHandle(h as _);
        }
    }
}

/// Spawn the async named-pipe server in the Tauri async runtime.
/// Must be called from `setup()` so we have a valid `AppHandle`.
///
/// The server runs for the process lifetime, accepting one client per
/// loop iteration.  Each client sends a `|`-delimited arg string; the
/// handler focuses the main window and re-emits the appropriate Tauri event.
pub fn start_pipe_listener(app: tauri::AppHandle) {
    let sid = current_session_id();
    let path = pipe_path(sid);

    tauri::async_runtime::spawn(async move {
        use tokio::io::AsyncReadExt as _;
        use tokio::net::windows::named_pipe::ServerOptions;

        // KT: first_pipe_instance(true) on the INITIAL create only. If another
        // listener already owns the pipe name, CreateNamedPipe returns
        // ERROR_ALREADY_EXISTS — meaning a prior start_pipe_listener is alive
        // (e.g. a zombie from a previous process that hasn't fully exited).
        // Bail loudly rather than creating a silent duplicate server.
        let first_server = match ServerOptions::new().first_pipe_instance(true).create(&path) {
            Ok(s) => s,
            Err(e) => {
                crate::log_message_src(
                    "error",
                    "core",
                    &format!(
                        "[SessionInstance] pipe listener startup failed (another listener alive?): {}",
                        e
                    ),
                );
                return;
            }
        };

        // Hand the already-created first instance into the loop so the loop body
        // only needs to handle "wait for client → read → recreate".
        let mut server = first_server;
        loop {
            // Wait for a client to connect; recreate on error and try again.
            if server.connect().await.is_err() {
                match ServerOptions::new().create(&path) {
                    Ok(s) => {
                        server = s;
                    }
                    Err(e) => {
                        crate::log_message(
                            "error",
                            &format!("[SessionInstance] pipe recreate failed: {}", e),
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                }
                continue;
            }

            // Read the forwarded payload (max 8 KiB — more than enough for
            // a list of file paths).
            let mut buf = vec![0u8; 8192];
            match server.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    let payload = String::from_utf8_lossy(&buf[..n]);
                    handle_forwarded_args(&app, &payload);
                }
                _ => {}
            }
            // Recreate the server instance so we're ready for the next client.
            // Successive iterations are NOT the first instance.
            match ServerOptions::new().create(&path) {
                Ok(s) => {
                    server = s;
                }
                Err(e) => {
                    crate::log_message(
                        "error",
                        &format!("[SessionInstance] pipe recreate failed after client: {}", e),
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    // Try once more before looping — the pipe name should still be ours.
                    if let Ok(s) = ServerOptions::new().create(&path) {
                        server = s;
                    }
                }
            }
        }
    });
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Persist our own PID to HKCU\SOFTWARE\WinCommander as REG_DWORD "PrimaryPid"
/// so a future duplicate-instance can verify whether the mutex-holder is alive.
/// HKCU avoids any elevation requirement. Non-fatal on failure — logged only.
fn persist_primary_pid() {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD,
        REG_OPTION_NON_VOLATILE,
    };
    let key_path: Vec<u16> = "SOFTWARE\\WinCommander\0".encode_utf16().collect();
    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    unsafe {
        let r = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        );
        if r != 0 {
            crate::log_message_src(
                "warn",
                "core",
                &format!(
                    "[SessionInstance] persist_primary_pid: RegCreateKeyEx failed: {}",
                    r
                ),
            );
            return;
        }
        let pid = GetCurrentProcessId();
        let vn: Vec<u16> = "PrimaryPid\0".encode_utf16().collect();
        let _ = RegSetValueExW(
            hkey,
            vn.as_ptr(),
            0,
            REG_DWORD,
            &pid as *const u32 as *const u8,
            4,
        );
        let _ = RegCloseKey(hkey);
    }
    crate::log_message_src(
        "info",
        "core",
        &format!("[SessionInstance] persisted PrimaryPid={}", unsafe {
            GetCurrentProcessId()
        }),
    );
}

/// Read the stored PrimaryPid from HKCU\SOFTWARE\WinCommander.
/// Returns None if the key or value doesn't exist.
fn read_stored_primary_pid() -> Option<u32> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_DWORD,
    };
    let key_path: Vec<u16> = "SOFTWARE\\WinCommander\0".encode_utf16().collect();
    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    unsafe {
        let r = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        );
        if r != 0 {
            return None;
        }
        let vn: Vec<u16> = "PrimaryPid\0".encode_utf16().collect();
        let mut data: u32 = 0;
        let mut data_size: u32 = 4;
        let mut reg_type: u32 = 0;
        let r2 = RegQueryValueExW(
            hkey,
            vn.as_ptr(),
            std::ptr::null_mut(),
            &mut reg_type,
            &mut data as *mut u32 as *mut u8,
            &mut data_size,
        );
        let _ = RegCloseKey(hkey);
        if r2 == 0 && reg_type == REG_DWORD {
            Some(data)
        } else {
            None
        }
    }
}

/// Check whether a process with the given PID is still alive.
fn is_pid_alive(pid: u32) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(h, &mut exit_code);
        let _ = CloseHandle(h);
        ok != 0 && exit_code == STILL_ACTIVE as u32
    }
}

/// Terminate a process by PID and wait up to ~2s for it to exit.
fn kill_and_wait(pid: u32) {
    unsafe {
        let h = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if h.is_null() {
            return;
        }
        let _ = TerminateProcess(h, 1);
        // Wait up to 2 000 ms for the process to fully exit.
        let _ = WaitForSingleObject(h, 2000);
        let _ = CloseHandle(h);
    }
}

/// Write the CLI args to the running instance's named pipe so it can handle
/// them (e.g. open the scrubber / shredder with the supplied paths).
/// Returns (delivered, stored_primary_pid).
/// Delivered=false means the pipe never answered within ~3 000 ms.
fn forward_args_with_liveness(sid: u32, args: &[String]) -> (bool, Option<u32>) {
    use std::io::Write as _;
    let path = pipe_path(sid);
    let stored_pid = read_stored_primary_pid();
    // KT: 30 × 100ms = 3 000ms so a busy-but-alive primary (e.g. loading a
    // large dataset on startup) is not falsely declared dead.
    for _ in 0..30 {
        match std::fs::OpenOptions::new().write(true).open(&path) {
            Ok(mut f) => {
                let _ = f.write_all(args.join("|").as_bytes());
                return (true, stored_pid);
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
    // Pipe did not open within 3s → primary is silent; return stored PID for
    // the caller to decide whether to take over.
    (false, stored_pid)
}

/// Single source of truth for context-menu verb → Tauri event resolution.
/// BOTH the warm-forward path (`handle_forwarded_args` below, parsed from the
/// pipe payload) and the cold-start path (`lib.rs`'s `setup()`, parsed from
/// process argv) MUST route through this — a verb flag added to one call
/// site's arg-parsing but not the other used to be able to silently regress
/// to the default. The default for an unrecognized/absent verb is
/// intentionally "shred-requested" (the plain in-app secure-delete flow), so
/// every non-shred verb MUST be listed explicitly here or it silently
/// shreds — see AGENTS.md's "a missing branch SECURE-DELETES the target"
/// gotcha. `has_flag` lets each caller supply its own membership check
/// (`Vec<&str>::contains` here, `Vec<String>::iter().any` in lib.rs) without
/// this function caring about the caller's storage type.
pub(crate) fn resolve_context_menu_event(has_flag: impl Fn(&str) -> bool) -> &'static str {
    if has_flag("--safe-paste") {
        // Safe Paste forwards the destination FOLDER as a path — it must
        // never fall through to shred-requested, which would secure-delete
        // the paste target.
        "safe-paste-requested"
    } else if has_flag("--scrub") {
        "scrub-requested"
    } else {
        "shred-requested"
    }
}

/// Parse a forwarded arg payload and emit the appropriate Tauri event.
/// Format: `path1|--flag|path2` (same separator as the original plugin used).
fn handle_forwarded_args(app: &tauri::AppHandle, payload: &str) {
    // If the app is still initializing, queue the payload — showing the window
    // or re-entering calc mode before the frontend has booted produces a black
    // frame and races the startup window-decision. set_app_ready() replays it.
    if !is_app_ready() {
        PENDING_FORWARDS.lock().unwrap().push(payload.to_string());
        crate::log_message_src(
            "info",
            "core",
            "[SessionInstance] forward queued — app not ready yet",
        );
        return;
    }
    let parts: Vec<&str> = payload.split('|').collect();
    // Win32 ShowWindow (which desynced Tauri's window state and crashed on
    // maximize). A dead app can't toggle — it cold-starts and reveals instead.
    // --focus is the sentinel sent by a bare double-click of the exe — an
    // explicit "show me", never a hide.
    let is_focus = parts.contains(&"--focus");
    // Safe Paste must never bring the window forward — see lib.rs's setup()
    // cold-start dispatch for the mirrored guard. The event still emits below
    // (paths carries the destination folder) so the frontend can run the
    // paste headlessly; only the toast in RightSidebar.tsx's
    // safe-paste-requested listener surfaces (success/error), and it doesn't
    // need the window shown.
    let is_safe_paste = parts.contains(&"--safe-paste");
    let paths: Vec<String> = parts
        .iter()
        .filter(|&&p| !p.starts_with("--"))
        .map(|&p| p.to_string())
        .collect();

    if let Some(win) = app.get_webview_window("main") {
        // Keyed on the runtime lock flag, not "PIN configured": a locked
        // calculator gate is never hidden by the hotkey (its visible state IS
        // the disguise) and reveals re-show the calc UI, not the real window.
        let calculator_mode = crate::calc_mode_active(app);
        if is_safe_paste {
            // Skip both the hide and reveal branches below entirely — the
            // window must stay hidden/backgrounded for the whole Safe Paste
            // operation, success or failure.
        } else if is_focus || !paths.is_empty() {
            // Reveal branch: explicit user intent to see the window.
            if calculator_mode {
                // enter_calculator_mode handles AUMID, icon, tray hide, and show.
                let _ = crate::startup_auth::enter_calculator_mode_with(win.clone(), true);
            } else {
                // KT: set_skip_taskbar(false) before show() so the window immediately
                // appears in the taskbar. Without it the window shows visually but
                // remains absent from Alt+Tab until the user interacts with it.
                let _ = win.set_skip_taskbar(false);
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
                // Tray should be visible in non-calculator, non-hidden mode.
                let is_hidden = crate::wincommander_is_hidden();
                if let Some(tray) = app.tray_by_id("tray") {
                    let _ = tray.set_visible(!is_hidden);
                }
                // KT: defer force_window_foreground — show()/unminimize() post
                // WM_SHOWWINDOW/SW_RESTORE to the message queue and return before
                // Windows has made the window visible. SetForegroundWindow called
                // synchronously here is silently rejected (target not visible yet),
                // leaving the window unactivated and black until the user clicks.
                // 50 ms lets the pump drain; set_focus follows 150 ms later.
                let win_clone = win.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    crate::force_window_foreground(&win_clone);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let _ = win_clone.set_focus();
                });
            }
            crate::log_message_src(
                "info",
                "core",
                &format!(
                    "[SessionInstance] relaunch show: calculator={} focus={}",
                    calculator_mode, is_focus
                ),
            );
        }
    }

    if !paths.is_empty() {
        let event = resolve_context_menu_event(|flag| parts.contains(&flag));
        let _ = app.emit(event, &paths);
    }
}

#[cfg(test)]
mod resolve_context_menu_event_tests {
    use super::resolve_context_menu_event;

    #[test]
    fn safe_paste_flag_maps_to_safe_paste_event() {
        assert_eq!(
            resolve_context_menu_event(|f| f == "--safe-paste"),
            "safe-paste-requested"
        );
    }

    #[test]
    fn scrub_flag_maps_to_scrub_event() {
        assert_eq!(
            resolve_context_menu_event(|f| f == "--scrub"),
            "scrub-requested"
        );
    }

    #[test]
    fn no_recognized_flag_falls_through_to_shred() {
        assert_eq!(resolve_context_menu_event(|_| false), "shred-requested");
    }

    #[test]
    fn unrelated_flags_fall_through_to_shred() {
        // --focus / --minimized are handled by separate logic in the callers
        // and must NOT be mistaken for scrub/safe-paste here.
        assert_eq!(
            resolve_context_menu_event(|f| f == "--focus"),
            "shred-requested"
        );
    }

    #[test]
    fn safe_paste_takes_priority_if_both_flags_present() {
        // Should never occur in practice (mutually exclusive verbs), but if
        // it ever did, pasting into a folder must never shred it.
        assert_eq!(
            resolve_context_menu_event(|f| f == "--safe-paste" || f == "--scrub"),
            "safe-paste-requested"
        );
    }
}
