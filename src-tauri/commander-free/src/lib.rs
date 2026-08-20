use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
mod action_steps;
mod activity_watch_autostart;
mod advisor;
mod appearance;
mod argus;
mod attend_watch;
mod authz;
mod autostart;
mod backend;
mod child_jobs;
pub mod cli;
pub use wincmd_shared::command_strings;
mod auth_anomaly;
mod canary_tokens;
mod context_menu_shred;
mod datastore;
mod devtools;
mod disk_analyzer;
mod downloads_watcher;
mod driver_health;
mod driver_maintenance;
mod duplicate_finder;
mod empty_folder_cleaner;
mod environment_cleaner;
mod evidence;
mod evidence_vault;
mod explorer_context;
mod explorer_context_menu;
mod f6_keystore;
mod f6_orchestrator;
mod f6_provision;
mod f6_verify_boot;
mod file_metadata;
mod file_monitor;
mod file_search;
mod file_watch_trigger;
mod firewall_audit;
mod fleet_agent;
mod flow_bridge;
mod flow_bundle;
mod flow_capabilities;
mod flow_engine;
mod flow_health;
mod gpo_policy;
mod inactivity_timer;
mod investigator_install;
mod license;
mod local_clipboard_rules;
mod log;
mod malware_scan;
mod monitor_util;
mod native_notify;
mod net;
mod net_traffic_alert;
mod network_guard;
mod network_maintenance;
mod network_toggle;
mod package_updates;
mod paste_monitor;
mod paths;
mod payments;
mod port_monitor;
mod print_log;
mod pro_install;
mod ransomware_monitor;
#[cfg(windows)]
mod rdp_session_watch;
mod reboot_usb;
pub mod recovery_wipe_plan;
mod registry_hygiene;
mod remote_sessions;
mod routine_cleaner;
mod runtime_visibility;
mod safe_clip;
mod screen_privacy;
mod search_actions;
mod security_data;
mod selective_erase;
mod server_apps;
mod services;
mod session_assurance;
#[cfg(windows)]
mod session_instance;
mod settings;
mod shield_quota;
mod shortcut_actions;
mod shortcut_cleaner;
mod sidecar;
mod startup_auth;
mod startup_maintenance;
mod storage_probe;
// This is an executable-free admission contract, covered by its unit tests.
// It is intentionally excluded from the shipped binary until the signed
// recovery-environment handoff that consumes it is wired in.
#[cfg(test)]
mod sanitize_plan;
mod system_metrics;
mod uninstall_leftovers;
mod updater;
mod usb_auto_sandbox;
mod usb_hid_guard;
mod usb_metering;
mod usb_monitor;
mod usb_policy;
mod vm_sandbox;
mod vpn_kill_switch;
mod wifi_check;

pub(crate) use log::{
    infer_pro_level, log_message, log_message_src, set_logging_enabled_flag, LOG_SRC_FLOWS,
    LOG_SRC_PRO,
};

struct TrayShieldState {
    running: Mutex<bool>,
    menu_item: MenuItem<tauri::Wry>,
}

impl TrayShieldState {
    fn new(menu_item: MenuItem<tauri::Wry>) -> Self {
        Self {
            running: Mutex::new(false),
            menu_item,
        }
    }
}

/// Tracks the currently registered panic hotkey string so it can be swapped at runtime.
struct PanicHotkeyState(Mutex<String>);

/// Tracks the currently registered search bar hotkey string.
struct SearchHotkeyState(Mutex<String>);

/// Tracks the currently registered hide-peek hotkey string.
/// Empty string means no hotkey is registered.
struct HideHotkeyState(Mutex<String>);

/// Runtime "currently locked as a calculator" flag. TRUE while the calculator
/// gate is showing (locked, pre-PIN); FALSE once the user has authenticated into
/// the real / decoy app. Distinct from "a Real PIN is configured" — both can be
/// true at once, but only this flag decides tray/close/reopen behaviour so an
/// authenticated session doesn't get treated as locked. Seeded in setup() from
/// the cold-start state; flipped by enter_calculator_mode / exit_calculator_mode.
pub(crate) struct CalcModeState(pub Mutex<bool>);

/// Read the runtime calculator-locked flag. Falls back to the configured-PIN
/// check only when the state hasn't been managed yet (should not happen post-setup).
pub(crate) fn calc_mode_active(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    app.try_state::<CalcModeState>()
        .and_then(|s| s.0.lock().ok().map(|g| *g))
        .unwrap_or_else(startup_auth::startup_pin_is_configured_sync)
}

/// Update the runtime calculator-locked flag.
pub(crate) fn set_calc_mode_active(app: &tauri::AppHandle, value: bool) {
    use tauri::Manager;
    if let Some(s) = app.try_state::<CalcModeState>() {
        if let Ok(mut g) = s.0.lock() {
            *g = value;
        }
    }
}

#[tauri::command]
fn update_panic_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let state = app
        .try_state::<PanicHotkeyState>()
        .ok_or_else(|| "PanicHotkeyState not initialised".to_string())?;
    let mut current = state.0.lock().map_err(|e| e.to_string())?;
    // Unregister the old shortcut
    if !current.is_empty() {
        let _ = app.global_shortcut().unregister(current.as_str());
    }
    // Register the new one
    if !hotkey.is_empty() {
        app.global_shortcut()
            .on_shortcut(hotkey.as_str(), |app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    let _ = app.emit("lockdown-trigger", ());
                }
            })
            .map_err(|e| e.to_string())?;
    }
    *current = hotkey;
    Ok(())
}

#[tauri::command]
fn update_search_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let state = app
        .try_state::<SearchHotkeyState>()
        .ok_or_else(|| "SearchHotkeyState not initialised".to_string())?;
    let mut current = state.0.lock().map_err(|e| e.to_string())?;
    if !current.is_empty() {
        let _ = app.global_shortcut().unregister(current.as_str());
    }
    if !hotkey.is_empty() {
        app.global_shortcut()
            .on_shortcut(hotkey.as_str(), |app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    handle_search_hotkey(app);
                }
            })
            .map_err(|e| e.to_string())?;
    }
    *current = hotkey;
    Ok(())
}

/// Register the hide-peek hotkey handler.  Extracted so startup, apply_wincommander_hide_mode,
/// and update_hide_hotkey all share the exact same closure without duplication.
fn register_hide_peek_hotkey(app: &tauri::AppHandle, hotkey: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    app.global_shortcut()
        .on_shortcut(hotkey, |app, _sc, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                if window.is_visible().unwrap_or(false) {
                    // Hide: remove from taskbar and screen; tray stays invisible in
                    // hidden/calc mode (the user re-peeks via the hotkey again).
                    let _ = window.set_skip_taskbar(true);
                    let _ = window.hide();
                    // Keep tray icon hidden regardless of mode — it was already
                    // hidden/shown correctly at startup or apply_wincommander_hide_mode.
                    log_message_src("info", "core", "[Hotkey] peek/hide: hiding window");
                } else {
                    log_message_src("info", "core", "[Hotkey] peek/hide: revealing window");
                    // Single shared reveal implementation — it handles calc-mode
                    // re-entry, the unminimize-before-show ordering, the visible-flag
                    // resync, and the deferred foreground workaround. This branch used
                    // to re-implement all of that with show() before unminimize().
                    reveal_main_window(app);
                }
            }
        })
        .map_err(|e| e.to_string())
}

/// Update the hide-peek hotkey at runtime.
/// Only registers the new hotkey when hidden mode is currently active — if the app
/// is not hidden the hotkey string is stored but not registered until hide is toggled on.
#[tauri::command]
fn update_hide_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let state = app
        .try_state::<HideHotkeyState>()
        .ok_or_else(|| "HideHotkeyState not initialised".to_string())?;

    // Clone current value and drop the lock BEFORE touching global_shortcut().
    // Holding the MutexGuard across shortcut API calls causes a deadlock because
    // Tauri's shortcut plugin acquires its own internal lock during registration.
    let old = {
        let g = state.0.lock().map_err(|e| e.to_string())?;
        g.clone()
    };

    if !old.is_empty() {
        let _ = app.global_shortcut().unregister(old.as_str());
    }
    // Register whenever a hotkey is set so it works as an always-on hide/unhide
    // toggle — the handler hides a visible window and shows a hidden one. (It
    // used to register only while already hidden, so a freshly-set hotkey did
    // nothing until the app was hidden some other way.)
    if !hotkey.is_empty() {
        register_hide_peek_hotkey(&app, &hotkey)?;
    }

    // Re-acquire to store the new value.
    let mut g = state.0.lock().map_err(|e| e.to_string())?;
    *g = hotkey;
    Ok(())
}

/// Force the overlay window to the foreground. Combines several Windows
/// techniques because no single one works in all situations:
///
///   1. Tap Alt — Windows resets its foreground lock when an Alt keystroke
///      arrives, so any thread can SetForegroundWindow during the tap.
///      This is the Microsoft-documented workaround for the same problem.
///   2. AttachThreadInput — briefly attaches our input queue to whatever
///      the current foreground thread is so SetForegroundWindow doesn't
///      hit LockSetForegroundWindow.
///   3. SetForegroundWindow + BringWindowToTop + SetActiveWindow + SetFocus
///      in sequence so both the native HWND and the inner WebView2 child
///      HWND end up active.
///
/// Without this, Tauri's set_focus() can return Ok but Windows silently
/// rejects the foreground promotion — the window paints, the webview
/// renders, but the input never gets a blinking caret because the window
/// isn't truly the foreground.
#[cfg(windows)]
pub(crate) fn force_window_foreground(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::AttachThreadInput;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, SetActiveWindow, SetFocus, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsZoomed,
        SetForegroundWindow, SetWindowPos, ShowWindowAsync, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_RESTORE, SW_SHOWMAXIMIZED,
    };
    let Ok(raw_hwnd) = window.hwnd() else { return };
    let target: HWND = raw_hwnd.0 as HWND;
    unsafe {
        // Do not use the synchronous ShowWindow from a tray callback. At logon
        // Explorer and the app can be on different input queues; ShowWindow can
        // wait on the shell while the shell is waiting for this callback to
        // return. Keep a maximized window maximized: the old unconditional
        // SW_RESTORE ran after reveal_main_window() had maximized the app,
        // making a full-search handoff visibly shrink back to its saved size.
        // SetWindowPos still gives a never-before-visible --minimized launch
        // an explicit visible top-level placement.
        let show_command = if IsZoomed(target) != 0 {
            SW_SHOWMAXIMIZED
        } else {
            SW_RESTORE
        };
        ShowWindowAsync(target, show_command);
        SetWindowPos(
            target,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );

        // Microsoft-documented workaround: a synthetic Alt keystroke resets
        // the foreground-window lock for the current input desktop, so any
        // SetForegroundWindow call right after will actually take effect.
        keybd_event(VK_MENU as u8, 0, 0, 0);
        keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);

        let foreground = GetForegroundWindow();
        let mut foreground_pid: u32 = 0;
        let foreground_thread = if !foreground.is_null() {
            GetWindowThreadProcessId(foreground, &mut foreground_pid as *mut u32)
        } else {
            0
        };

        let target_thread = GetWindowThreadProcessId(target, std::ptr::null_mut());
        let attached = foreground_thread != 0
            && foreground_thread != target_thread
            && AttachThreadInput(target_thread, foreground_thread, 1) != 0;

        BringWindowToTop(target);
        SetForegroundWindow(target);
        SetActiveWindow(target);

        // SetFocus on the WebView2 child HWND (Chrome_WidgetWin_*) while
        // thread input is still attached. This is the critical step that
        // makes the DOM input actually receive the caret — SetActiveWindow
        // on the outer Tauri HWND alone is not enough for WebView2.
        if let Some(wv_hwnd) = find_webview2_hwnd(target) {
            SetFocus(wv_hwnd);
        }

        if attached {
            AttachThreadInput(target_thread, foreground_thread, 0);
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn force_window_foreground(_window: &tauri::WebviewWindow) {}

/// Restore + foreground the main window from a tray reveal (icon click / "Show").
///
/// A bare show()+set_focus() is NOT enough when the reveal originates from the
/// already-running, elevated, BACKGROUND process: clicking the tray makes the
/// shell — not us — the foreground process, so Windows' foreground-activation
/// lock silently refuses SetForegroundWindow and the window stays hidden behind
/// the shell. Routing the tray paths through force_window_foreground — the same
/// Alt-key workaround used for restored windows — makes a reveal reliably bring
/// the window up.
pub(crate) fn reveal_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        log_message_src("warn", "core", "[Reveal] main window not found");
        return;
    };
    // Keyed on the runtime lock flag, not "PIN configured": a locked calculator
    // re-enters calc mode; an authenticated session restores the full window.
    let calculator_mode = calc_mode_active(app);
    log_message_src(
        "info",
        "core",
        &format!(
            "[Reveal] start: label={} calc_mode={} hidden={} visible={}",
            window.label(),
            calculator_mode,
            wincommander_is_hidden(),
            window.is_visible().unwrap_or(false)
        ),
    );
    if calculator_mode {
        let _ = startup_auth::enter_calculator_mode_with(window.clone(), true);
    } else {
        let _ = window.set_skip_taskbar(false);
        let _ = window.unminimize();
        // Tray and peek-hotkey reveals are the primary way a hidden app is
        // reopened. Match the normal startup experience instead of leaving a
        // small/restored window behind after an update relaunch.
        let _ = window.maximize();
        let _ = window.show();
        // A hide/show transition forces a new native visibility transition if
        // Windows still reports the HWND hidden after Tauri's show request.
        if !window.is_visible().unwrap_or(false) {
            log_message_src(
                "warn",
                "core",
                "[Reveal] show() left the window invisible — forcing a hide/show transition",
            );
            let _ = window.hide();
            let _ = window.show();
        }
        set_wincommander_window_icon(&window);
        let _ = window.set_focus();
        // Tray callbacks run on the shell message pump. Yield before activating
        // so Windows has registered the restored window. A cold post-logon
        // WebView2 window can miss the first visibility transition, so retry
        // only while the native HWND remains hidden; never steal focus later
        // after the user has moved to another application.
        let window_for_foreground = window.clone();
        std::thread::spawn(move || {
            for delay_ms in [50_u64, 250, 800] {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                force_window_foreground(&window_for_foreground);
                let _ = window_for_foreground.set_focus();
                if window_for_foreground.is_visible().unwrap_or(true) {
                    break;
                }
                let _ = window_for_foreground.hide();
                let _ = window_for_foreground.show();
            }
        });
        // Signal the (authenticated) app was revealed so the frontend re-arms the
        // update prompt — a dismissed-then-reopened window shows it again. Not
        // emitted on the calc-mode branch: a locked calculator re-entry isn't a
        // reveal of the real app.
        let _ = app.emit("wincommander://window-revealed", ());
    }
    log_message_src(
        "info",
        "core",
        &format!(
            "[Reveal] done: visible={}",
            window.is_visible().unwrap_or(false)
        ),
    );
}

/// Toggle the normal close-to-tray lifecycle from a single tray click.
/// Delegating the hide branch to `close()` keeps calculator/borrowed-mode
/// re-locking and tray visibility consistent with the window close button.
fn toggle_main_window_from_tray(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        log_message_src("warn", "core", "[Tray] main window not found");
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.close();
    } else {
        reveal_main_window(app);
    }
}

fn set_wincommander_window_icon(window: &tauri::WebviewWindow) {
    if let Ok(app_icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png")) {
        let _ = window.set_icon(app_icon);
    }
}

/// Give the process a calculator-only AppUserModelID before the window is shown.
/// The Windows taskbar resolves a button's hover label and grouping from the
/// process AUMID, not the live window title — so without this the calculator
/// window shows the installed "WinCommander" shortcut name even though its title
/// is "Calculator". A distinct AUMID with no matching pinned shortcut makes the
/// taskbar fall back to the window title + calc icon. Calculator/PIN mode only.
#[cfg(windows)]
fn set_calculator_taskbar_identity() {
    startup_auth::write_calculator_process_identity();
}

#[cfg(not(windows))]
fn set_calculator_taskbar_identity() {}

/// Enumerate child windows of `parent` looking for the first one whose
/// class name starts with "Chrome_" — that is the WebView2 (Chromium) host
/// widget HWND. We pass the target address through lparam so the callback
/// can write the result without needing a closure (Win32 callbacks must be
/// bare extern "system" fns, not Rust closures).
#[cfg(windows)]
unsafe extern "system" fn find_chrome_widget_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: isize, // LPARAM = isize in windows-sys
) -> i32 {
    // BOOL   = i32  in windows-sys
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let out = lparam as *mut windows_sys::Win32::Foundation::HWND;
    let mut buf = [0u16; 256];
    let len = GetClassNameW(hwnd, buf.as_mut_ptr(), 256);
    if len > 0 {
        let name = String::from_utf16_lossy(&buf[..len as usize]);
        if name.starts_with("Chrome_") || name.contains("WebView2") {
            *out = hwnd;
            return 0; // FALSE — stop enumeration
        }
    }
    1 // TRUE — continue
}

/// Walk the child-window tree of `parent` and return the first WebView2
/// host HWND (class name starts with "Chrome_"). This is the HWND that
/// must receive `SetFocus` for a DOM input to show a blinking caret;
/// calling SetFocus on the outer Tauri shell HWND is not sufficient.
#[cfg(windows)]
fn find_webview2_hwnd(
    parent: windows_sys::Win32::Foundation::HWND,
) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumChildWindows;
    let mut found: windows_sys::Win32::Foundation::HWND = std::ptr::null_mut();
    unsafe {
        EnumChildWindows(
            parent,
            Some(find_chrome_widget_proc),
            &mut found as *mut windows_sys::Win32::Foundation::HWND as _,
        );
    }
    if found.is_null() {
        None
    } else {
        Some(found)
    }
}

/// Update the Windows-visible name of the installed app — both the
/// `DisplayName` in the Uninstall registry key (shown in Settings →
/// Apps + Control Panel) and the Start Menu shortcut filename
/// (shown in Start menu, Windows search, taskbar tooltips).
///
/// `label` should be one of:
///   - "WinCommander Pro"   when entitled + Pro EXE installed
///   - "WinCommander Free"  otherwise
///
/// Idempotent: silently no-ops when the requested label is already in
/// place. Failures are returned as a string error but the local app
/// keeps running normally — this is cosmetic OS-level state, not a
/// critical path.
#[tauri::command]
fn set_app_display_label(label: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Trim + sanity-check input — only accept the two labels we
        // own; refuse anything else so a compromised frontend can't
        // graffiti arbitrary text into our uninstall entry.
        let target = label.trim();
        let pro_label = paths::app_display_name_with_edition(true);
        let free_label = paths::app_display_name_with_edition(false);
        let base_label = paths::app_display_name();
        if target != pro_label && target != free_label && target != base_label {
            return Err(format!(
                "set_app_display_label: refusing unrecognised label '{}'",
                target
            ));
        }
        let mut errors: Vec<String> = Vec::new();

        // ── 1. Update the Uninstall registry DisplayName ──
        // We don't know our product GUID at compile time (Tauri/WiX
        // picks one per build), so enumerate Uninstall entries and
        // match by current DisplayName starting with "WinCommander".
        let uninstall_roots = [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ];
        for root in &uninstall_roots {
            let ps = format!(
                "Get-ChildItem -LiteralPath 'HKLM:\\{root}' -ErrorAction SilentlyContinue | ForEach-Object {{ \
                    $p = $_.PSPath; \
                    $name = (Get-ItemProperty -LiteralPath $p -Name DisplayName -ErrorAction SilentlyContinue).DisplayName; \
                    if ($name -and $name -like '{base}*') {{ \
                        Set-ItemProperty -LiteralPath $p -Name DisplayName -Value '{target}' -ErrorAction SilentlyContinue \
                    }} \
                }}",
                base = base_label
            );
            let mut cmd = std::process::Command::new("powershell");
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &ps,
            ]);
            match cmd.output() {
                Ok(out) if out.status.success() => {}
                Ok(out) => errors.push(format!(
                    "{}: ps exit {}: {}",
                    root,
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                )),
                Err(e) => errors.push(format!("{}: spawn: {}", root, e)),
            }
        }

        // ── 2. Rename the Start Menu shortcut(s) ──
        // The MSI places one .lnk at ProgramData (machine-wide) and
        // potentially one at APPDATA (per-user). Both filenames track
        // the productName at install time. Rename them in place so
        // Start menu + Windows Search reflect the new edition.
        let mut shortcut_dirs: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(pd) = std::env::var("ProgramData") {
            shortcut_dirs.push(
                std::path::PathBuf::from(pd)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs"),
            );
        }
        if let Ok(ad) = std::env::var("APPDATA") {
            shortcut_dirs.push(
                std::path::PathBuf::from(ad)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs"),
            );
        }
        for dir in &shortcut_dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = match path.file_name().and_then(|f| f.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                // Look for *.lnk whose basename starts with "WinCommander "
                // (with or without an edition suffix) — exclude unrelated apps.
                if !file_name.ends_with(".lnk") {
                    continue;
                }
                let stem = &file_name[..file_name.len() - 4];
                let matches_ours = stem == base_label
                    || stem.starts_with(&free_label)
                    || stem.starts_with(&pro_label);
                if !matches_ours {
                    continue;
                }
                let new_name = format!("{}.lnk", target);
                if file_name == new_name {
                    continue;
                } // already correct
                let new_path = path.with_file_name(&new_name);
                if let Err(e) = std::fs::rename(&path, &new_path) {
                    errors.push(format!(
                        "rename {} → {}: {}",
                        path.display(),
                        new_path.display(),
                        e
                    ));
                }
            }
        }

        if errors.is_empty() {
            return Ok(());
        }

        // ── UAC auto-elevation path ──────────────────────────────────
        // If any failure mentions access/permission, the .lnk lives in
        // %ProgramData% (admin-owned) or HKLM (registry, admin-only).
        // Spawn an elevated PowerShell child via `Start-Process -Verb
        // RunAs` that performs JUST the rename + registry write. The
        // outer (unelevated) PS triggers the UAC prompt; user clicks
        // Yes → elevated child runs → shortcut + DisplayName updated
        // in one shot. User clicks No → leave Start Menu as-is; the
        // running app's title bar / tray still reflect the new label.
        let needs_elevation = errors.iter().any(|e| {
            e.to_lowercase().contains("denied")
                || e.to_lowercase().contains("permission")
                || e.to_lowercase().contains("access")
        });
        if needs_elevation {
            if let Err(e) = elevate_display_label(target) {
                log_message(
                    "warn",
                    &format!("[DisplayLabel] elevation kickoff failed: {}", e),
                );
            } else {
                log_message(
                    "info",
                    "[DisplayLabel] UAC elevation requested for shortcut + registry rename",
                );
                // Treat as success from the caller's perspective — the
                // elevated child runs async; we don't block on its
                // completion. Worst case (user cancels UAC) the
                // shortcut stays mislabelled but the app works.
                return Ok(());
            }
        }

        // Soft failure — log + return the first error string. The
        // frontend treats this as best-effort; UI doesn't break.
        let summary = errors.join(" | ");
        log_message(
            "warn",
            &format!("[DisplayLabel] partial failures: {}", summary),
        );
        Err(summary)
    }
    #[cfg(not(windows))]
    {
        let _ = label;
        Err("set_app_display_label is Windows-only".to_string())
    }
}

/// Explicitly scoped Privacy Shield extension for the designated main window.
/// Windows does not provide a general, enforceable screenshot block: supported
/// capture APIs may render this HWND black, but privileged capture, remote
/// desktop, cameras, and older Windows versions can still bypass it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureProtectionStatus {
    enabled: bool,
    scope: &'static str,
    mode: &'static str,
    limitation: &'static str,
}

#[tauri::command]
fn set_capture_protection(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<CaptureProtectionStatus, String> {
    crate::license::require_paid("capture protection")?;
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowDisplayAffinity, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
        };
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| "main window not found".to_string())?;
        let raw = window.hwnd().map_err(|e| format!("hwnd: {}", e))?;
        let hwnd: HWND = raw.0 as HWND;
        let affinity = if enabled {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        let ok = unsafe { SetWindowDisplayAffinity(hwnd, affinity) };
        if ok == 0 {
            return Err("SetWindowDisplayAffinity failed (needs Windows 10 2004+)".into());
        }
        let mut observed = 0;
        if unsafe { GetWindowDisplayAffinity(hwnd, &mut observed) } == 0 || observed != affinity {
            return Err("Windows did not retain the requested capture-protection state".into());
        }
        Ok(CaptureProtectionStatus {
            enabled,
            scope: "wincommander-main-window",
            mode: if enabled { "exclude-from-capture" } else { "none" },
            limitation: "Best effort only: privileged capture, remote desktop, cameras, and unsupported Windows capture paths can bypass this setting.",
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (app, enabled);
        Err("capture protection is Windows-only".into())
    }
}

/// Employee-facing "My Monitoring Mirror" (PAID, Pro snapshot). Read-only,
/// NON-DRAINING view of exactly what Argus has queued and last sent to the
/// fleet: pending aggregate signals (kind/class/magnitude/severity + window),
/// the last-sent productivity summary, and the local consent standing. Proves
/// the PII-free privacy claim to the monitored user; never mutates or drains a
/// collector. Resolved in Pro by `fleet_push::monitoring_mirror`.
///
/// NOTE (build-agent scope): this thin wrapper mirrors the `argus.rs`
/// require_paid + dispatch_paid_command pattern exactly. It lives here rather
/// than in `argus.rs` only because this slice owns `lib.rs` (for the registration
/// line) but not `argus.rs`; it can be relocated to `argus.rs` verbatim and the
/// registration switched to `argus::argus_monitoring_mirror`.
#[tauri::command]
async fn argus_monitoring_mirror() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus monitoring mirror")?;
    crate::sidecar::dispatch_paid_command("argus_monitoring_mirror", serde_json::Value::Null).await
}

/// Spawn an elevated PowerShell child (via UAC prompt) that renames
/// the Start Menu .lnk and rewrites the HKLM uninstall DisplayName.
/// Returns Ok the moment the outer (unelevated) launcher process is
/// spawned — we don't wait for the elevated child since the UAC
/// dialog is owned by the user and may sit modal for a while.
#[cfg(windows)]
fn elevate_display_label(target: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // Defense-in-depth — only the three labels we own.
    let pro_l = paths::app_display_name_with_edition(true);
    let free_l = paths::app_display_name_with_edition(false);
    let base_l = paths::app_display_name();
    if target != pro_l && target != free_l && target != base_l {
        return Err(format!("refusing unrecognised label '{}'", target));
    }
    // Single PS line; multi-line stdin scripts are silently swallowed
    // by `powershell -Command -` on Windows PS 5.1 with CRLF source.
    let inner = format!(
        "$t='{}'; $base='{}'; foreach ($d in @(\"$env:ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\",\"$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\")) {{ if (Test-Path $d) {{ Get-ChildItem -Path $d -Filter \"$($base)*.lnk\" -ErrorAction SilentlyContinue | ForEach-Object {{ $n=\"$t.lnk\"; if ($_.Name -ne $n) {{ try {{ Rename-Item -LiteralPath $_.FullName -NewName $n -ErrorAction Stop }} catch {{}} }} }} }} }}; foreach ($r in @('HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall','HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall')) {{ Get-ChildItem -LiteralPath $r -ErrorAction SilentlyContinue | ForEach-Object {{ $p=$_.PSPath; $n=(Get-ItemProperty -LiteralPath $p -Name DisplayName -ErrorAction SilentlyContinue).DisplayName; if ($n -and $n -like \"$($base)*\") {{ Set-ItemProperty -LiteralPath $p -Name DisplayName -Value $t -ErrorAction SilentlyContinue }} }} }}",
        target, base_l
    );
    // Base64-encode the inner script and hand it to a child via
    // `-EncodedCommand`. This sidesteps all the quoting nightmares of
    // nested `Start-Process -ArgumentList` quoting.
    use base64::{engine::general_purpose, Engine as _};
    let utf16: Vec<u8> = inner.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let encoded = general_purpose::STANDARD.encode(&utf16);
    let outer = format!(
        "Start-Process powershell -Verb RunAs -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{}'",
        encoded
    );

    let mut cmd = std::process::Command::new("powershell");
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW for the outer launcher
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &outer]);
    cmd.spawn().map_err(|e| format!("elevation spawn: {}", e))?;
    Ok(())
}

fn handle_search_hotkey(app: &tauri::AppHandle) {
    if let Some(overlay) = app.get_webview_window("search-overlay") {
        if overlay.is_visible().unwrap_or(false) {
            // The quick-search text lives in the overlay WebView. Register
            // the acknowledgement before asking for it; a fixed delay here
            // used to lose the query on busy Windows/WebView2 instances.
            let handoff_received = Arc::new(AtomicBool::new(false));
            let handoff_received_listener = Arc::clone(&handoff_received);
            let app_for_handoff = app.clone();
            let handoff_listener = app.once("search-query-handoff-ready", move |event| {
                handoff_received_listener.store(true, Ordering::Release);
                let query = serde_json::from_str::<serde_json::Value>(event.payload())
                    .ok()
                    .and_then(|payload| payload.get("query")?.as_str().map(str::to_owned))
                    .unwrap_or_default();
                // A second Ctrl+Space is an explicit request to leave the
                // compact overlay and use full Search Files. The event alone
                // changes React state in the hidden main WebView; it does not
                // restore/maximize the native window. Reveal first so dev and
                // release builds both make the full search actually visible.
                reveal_main_window(&app_for_handoff);
                if let Some(main) = app_for_handoff.get_webview_window("main") {
                    let _ = main.emit("open-search-files-panel", query);
                }
            });

            let overlay_clone = overlay.clone();
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = overlay_clone.emit("handoff-search-query", ());
                // This is only a no-query fallback for a renderer that has
                // crashed or is still starting. Normal operation continues
                // from the acknowledgement above, with no timing race.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let _ = overlay_clone.hide();
                if !handoff_received.load(Ordering::Acquire) {
                    app_clone.unlisten(handoff_listener);
                    open_search_files_panel(&app_clone);
                }
            });
            return;
        }
        let _ = overlay.show();
        force_window_foreground(&overlay);

        let overlay_clone = overlay.clone();
        tauri::async_runtime::spawn(async move {
            // 200 ms gives Windows enough time to fully promote the window
            // to foreground before we re-assert focus and fire the frontend
            // focus event. 100 ms was occasionally too short on busy systems.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let _ = overlay_clone.set_focus();
            let _ = overlay_clone.emit("focus-search-input", ());
        });
        return;
    }

    if let Ok(overlay) = tauri::WebviewWindowBuilder::new(
        app,
        "search-overlay",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Quick Search")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .inner_size(760.0, 520.0)
    .center()
    .build()
    {
        let _ = overlay.show();
        force_window_foreground(&overlay);

        let overlay_clone = overlay.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let _ = overlay_clone.set_focus();
            let _ = overlay_clone.emit("focus-search-input", ());
        });
    }
}

fn open_search_files_panel(app: &tauri::AppHandle) {
    // Match the acknowledged query handoff above. This fallback is also
    // reached if the overlay renderer failed to answer, so it must reveal the
    // main native window instead of merely changing a hidden React panel.
    reveal_main_window(app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("open-search-files-panel", ());
    }
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// No-op stub kept for backwards compatibility (ErrorBoundary calls this).
/// Log viewing is now in-app only; logs are encrypted on disk.
#[tauri::command]
fn open_log_file() -> Result<(), String> {
    Ok(())
}

// ── DoH-aware update check ──────────────────────────────────────────
//
// Replaces the frontend's `@tauri-apps/plugin-updater` `check()` with a
// flow that resolves its configured update host via Cloudflare DoH first.
// Safety: the artifact is still minisign-verified by the Tauri updater
// downstream, so a hostile DoH response can't smuggle a fake update.

#[derive(serde::Serialize)]
pub struct DohUpdateInfo {
    pub available: bool,
    pub version: Option<String>,
    pub current_version: Option<String>,
    pub body: Option<String>,
    pub date: Option<String>,
}

#[derive(serde::Serialize)]
pub struct PublicIpTrace {
    pub ip: Option<String>,
    pub country: Option<String>,
    pub colo: Option<String>,
    pub source: String,
}

fn parse_cloudflare_trace(raw: &str) -> PublicIpTrace {
    let mut ip = None;
    let mut country = None;
    let mut colo = None;

    for line in raw.lines() {
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            match key {
                "ip" if !value.is_empty() => ip = Some(value.to_string()),
                "loc" if !value.is_empty() => country = Some(value.to_string()),
                "colo" if !value.is_empty() => colo = Some(value.to_string()),
                _ => {}
            }
        }
    }

    PublicIpTrace {
        ip,
        country,
        colo,
        source: "cloudflare-trace".to_string(),
    }
}

#[tauri::command]
async fn get_public_ip_trace() -> Result<PublicIpTrace, String> {
    let client = crate::net::doh_http_client()?;

    let trace_result = async {
        let raw = client
            .get("https://cloudflare.com/cdn-cgi/trace")
            .send()
            .await
            .map_err(|e| format!("Cloudflare trace request failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Cloudflare trace returned {}", e))?
            .text()
            .await
            .map_err(|e| format!("Cloudflare trace read failed: {}", e))?;
        let parsed = parse_cloudflare_trace(&raw);
        parsed
            .ip
            .as_ref()
            .ok_or_else(|| "Cloudflare trace did not include an IP".to_string())?;
        Ok::<PublicIpTrace, String>(parsed)
    }
    .await;

    let trace_error = match trace_result {
        Ok(trace) => return Ok(trace),
        Err(err) => err,
    };

    let fallback = client
        .get("https://api.ipify.org?format=json")
        .send()
        .await
        .map_err(|e| {
            format!(
                "Public IP lookup failed: {}; fallback failed: {}",
                trace_error, e
            )
        })?
        .error_for_status()
        .map_err(|e| format!("Public IP fallback returned {}", e))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Public IP fallback parse failed: {}", e))?;

    let ip = fallback
        .get("ip")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.to_string())
        .ok_or_else(|| "Public IP fallback did not include an IP".to_string())?;

    Ok(PublicIpTrace {
        ip: Some(ip),
        country: None,
        colo: None,
        source: "ipify".to_string(),
    })
}

#[tauri::command]
async fn app_check_for_updates_doh(app: tauri::AppHandle) -> Result<DohUpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;
    // Hard 20 s ceiling so a stalled DNS / TLS / HTTP layer can never leave
    // the UI stuck on "Checking for updates…" forever — the resolver itself
    // already has its own short timeouts, this is the seatbelt of last resort.
    //
    // Cache-buster: append `?t=<unix_ms>` to every manifest URL so any CDN /
    // intermediate proxy between us and the manifest host can't serve
    // a stale latest.json. The frontend retry in 3ef0cbe flushed our own
    // in-process cache; this closes the remaining HTTP-layer hole.
    let updater = app
        .updater_builder()
        .endpoints(cache_busted_endpoints(&app)?)
        .map_err(|e| format!("Updater endpoints failed: {}", e))?
        .configure_client(|cb| cb.dns_resolver(crate::net::doh_resolver()))
        .build()
        .map_err(|e| format!("Updater build failed: {}", e))?;
    let check_fut = updater.check();
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), check_fut)
        .await
        .map_err(|_| "Update check timed out (network/DNS slow or blocked)".to_string())?;
    match result {
        Ok(Some(update)) => Ok(DohUpdateInfo {
            available: true,
            version: Some(update.version.clone()),
            current_version: Some(update.current_version.clone()),
            body: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
        }),
        Ok(None) => Ok(DohUpdateInfo {
            available: false,
            version: None,
            current_version: None,
            body: None,
            date: None,
        }),
        Err(e) => Err(format!("Update check failed: {}", e)),
    }
}

#[tauri::command]
async fn app_install_update_doh(app: tauri::AppHandle) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    use tauri_plugin_updater::UpdaterExt;

    // Acquire the shared install lock (same AtomicBool used by
    // app_install_staged_update) so a double-click or race between the two
    // install paths can never start two concurrent installers.
    {
        let state = app
            .try_state::<crate::updater::StagedState>()
            .ok_or_else(|| "Updater state unavailable".to_string())?;
        if state.installing.swap(true, Ordering::AcqRel) {
            return Err("Update install already in progress".to_string());
        }
    }

    let result: Result<(String, String, Option<String>), String> = async {
        let updater = app
            .updater_builder()
            .endpoints(cache_busted_endpoints(&app)?)
            .map_err(|e| format!("Updater endpoints failed: {}", e))?
            .configure_client(|cb| cb.dns_resolver(crate::net::doh_resolver()))
            .build()
            .map_err(|e| format!("Updater build failed: {}", e))?;
        let update = updater
            .check()
            .await
            .map_err(|e| format!("Update re-check failed: {}", e))?
            .ok_or_else(|| "No update available".to_string())?;
        // Captured before download_and_install consumes `update`.
        let version = update.version.clone();
        let current = update.current_version.clone();
        let body = update.body.clone();
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| format!("Install failed: {}", e))?;
        Ok((version, current, body))
    }
    .await;

    match &result {
        Ok((version, current, body)) => {
            // Installed to disk — surface a persistent Restart affordance in case
            // the user dismisses instead of relaunching immediately.
            crate::updater::emit_ready(
                &app,
                Some(version.clone()),
                Some(current.clone()),
                body.clone(),
            );
        }
        Err(_) => {
            if let Some(s) = app.try_state::<crate::updater::StagedState>() {
                s.installing.store(false, Ordering::Release);
            }
        }
    }
    result.map(|_| ())
}

/// Build the updater endpoint list with a `?t=<unix_ms>` cache-buster
/// appended to each URL. Reads the configured endpoints from
/// tauri.conf.json so the source-of-truth stays in one place; we just
/// add the query param at call time. If an endpoint already has a
/// query string, we use `&` instead of `?`.
pub(crate) fn cache_busted_endpoints(app: &tauri::AppHandle) -> Result<Vec<url::Url>, String> {
    let cfg = app.config();
    let plugins = &cfg.plugins;
    let updater = plugins
        .0
        .get("updater")
        .ok_or_else(|| "Updater plugin config missing".to_string())?;
    let endpoints = updater
        .get("endpoints")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Updater endpoints missing or wrong type".to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut out = Vec::with_capacity(endpoints.len());
    for ep in endpoints {
        let raw = ep
            .as_str()
            .ok_or_else(|| "Updater endpoint not a string".to_string())?;
        let busted = if raw.contains('?') {
            format!("{}&t={}", raw, now_ms)
        } else {
            format!("{}?t={}", raw, now_ms)
        };
        let parsed =
            url::Url::parse(&busted).map_err(|e| format!("Bad updater URL '{}': {}", raw, e))?;
        out.push(parsed);
    }
    Ok(out)
}

#[tauri::command]
fn update_tray_shield_label(app: tauri::AppHandle, running: bool) -> Result<(), String> {
    let state = app
        .try_state::<TrayShieldState>()
        .ok_or_else(|| "Tray state not initialized".to_string())?;
    *state.running.lock().map_err(|e| e.to_string())? = running;
    let label = if running {
        "Disable Privacy Shield"
    } else {
        "Enable Privacy Shield"
    };
    state.menu_item.set_text(label).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(debug_assertions)]
#[tauri::command]
fn open_devtools(window: tauri::WebviewWindow) {
    window.open_devtools();
}

// ── WinCommander hidden-mode helpers ────────────────────────────────────────

fn wc_hide_flag_path() -> Option<std::path::PathBuf> {
    paths::hide_flag_path().ok()
}

/// Returns true if the hidden-mode flag file exists (written by the
/// Set-WinCommanderVisibility PS function or by apply_wincommander_hide_mode).
/// pub(crate) so session_instance.rs can check hidden state when showing the window.
pub(crate) fn wincommander_is_hidden() -> bool {
    wc_hide_flag_path().map(|p| p.exists()).unwrap_or(false)
}

/// Report the ACTUAL WinCommander hidden state (the hide-flag file is the truth,
/// written by apply_wincommander_hide_mode). The Secret-panel toggle reconciles
/// against this because the settings flag (ideal.identity.hideWinCommander) can
/// lag reality after startup — the app stays hidden but the toggle reads "off".
#[tauri::command]
fn wincommander_hidden_status() -> bool {
    wincommander_is_hidden()
}

/// Write a REG_DWORD to HKLM\SOFTWARE\WinCommander.
/// Contract: HiddenMode=1/0 (R1→R5), CalculatorMode=1/0 (R1→R5).
/// Non-fatal — failures are logged but never abort startup.
/// pub(crate) so startup_auth.rs can clear CalculatorMode after PIN success.
#[cfg(windows)]
pub(crate) fn write_wc_registry_dword(value_name: &str, data: u32) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_DWORD,
        REG_OPTION_NON_VOLATILE,
    };
    let key_path: Vec<u16> = "SOFTWARE\\WinCommander\0".encode_utf16().collect();
    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    unsafe {
        let r = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
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
            log_message_src(
                "warn",
                "core",
                &format!(
                    "[Registry] RegCreateKeyEx HKLM\\SOFTWARE\\WinCommander failed: {}",
                    r
                ),
            );
            return;
        }
        let vn: Vec<u16> = format!("{}\0", value_name).encode_utf16().collect();
        let _ = RegSetValueExW(
            hkey,
            vn.as_ptr(),
            0,
            REG_DWORD,
            &data as *const u32 as *const u8,
            4,
        );
        let _ = RegCloseKey(hkey);
    }
}

#[cfg(not(windows))]
pub(crate) fn write_wc_registry_dword(_value_name: &str, _data: u32) {}

/// Apply hidden mode immediately to the running app: hide/show the window and
/// tray icon, and write/delete the flag file so the next cold-start is also
/// affected. Called from the Settings concealment toggle.
#[tauri::command]
fn apply_wincommander_hide_mode(app: tauri::AppHandle, hidden: bool) -> Result<(), String> {
    // Write or delete the flag file.
    if let Some(flag) = wc_hide_flag_path() {
        if hidden {
            let dir = flag.parent().unwrap();
            std::fs::create_dir_all(dir).ok();
            std::fs::write(&flag, b"").map_err(|e| e.to_string())?;
        } else {
            let _ = std::fs::remove_file(&flag);
        }
    }

    // Write HiddenMode DWORD to HKLM\SOFTWARE\WinCommander (contract: read by R5/NSIS).
    write_wc_registry_dword("HiddenMode", if hidden { 1 } else { 0 });
    log_message_src(
        "info",
        "core",
        &format!("[Hide] apply_wincommander_hide_mode hidden={}", hidden),
    );

    let calculator_mode = startup_auth::startup_pin_is_configured_sync();

    // Window: hide immediately so there's no WinCommander window on screen.
    // skip_taskbar removes the entry from the taskbar and Alt+Tab switcher too.
    if let Some(window) = app.get_webview_window("main") {
        if hidden {
            let _ = window.set_skip_taskbar(true);
            let _ = window.hide();
        } else {
            let _ = window.set_skip_taskbar(false);
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }

    // Tray: invisible when hidden (or calculator mode active — calc never has tray).
    // The tray object still exists so the quit/show handlers keep working,
    // and a re-launch of the exe will restore the window via single-instance
    // forwarding even when the tray is invisible.
    if let Some(tray) = app.tray_by_id("tray") {
        let _ = tray.set_visible(!hidden && !calculator_mode);
    }

    // KT: the Settings toggle restarts Explorer right before this. When Explorer's
    // taskbar re-creates (TaskbarCreated broadcast), the tray icon is re-added
    // VISIBLE — undoing the hide above and causing the "tray icon sometimes
    // reappears when hidden" bug. Re-assert set_visible(false) a few times over
    // the next several seconds to catch the post-restart re-add whatever its
    // timing; each tick re-checks the live state so a quick un-hide stops it.
    if hidden || calculator_mode {
        let app_re = app.clone();
        tauri::async_runtime::spawn(async move {
            for delay_ms in [500u64, 1500, 3000, 5000, 8000] {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                if wincommander_is_hidden() || startup_auth::startup_pin_is_configured_sync() {
                    if let Some(tray) = app_re.tray_by_id("tray") {
                        let _ = tray.set_visible(false);
                        if let Ok(icon) =
                            tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                        {
                            let _ = tray.set_icon(Some(icon));
                        }
                    }
                } else {
                    break;
                }
            }
        });
    }

    // Peek hotkey: register when entering hidden mode, unregister when leaving.
    // Clone + drop the lock BEFORE calling global_shortcut() to avoid a deadlock
    // with Tauri's shortcut plugin internal lock.
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        let hotkey_str: String = app
            .try_state::<HideHotkeyState>()
            .and_then(|s| s.0.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();

        if !hotkey_str.is_empty() {
            // Always unregister first — makes this call idempotent if invoked twice.
            let _ = app.global_shortcut().unregister(hotkey_str.as_str());
            // Keep it registered in BOTH states so it stays an always-on
            // hide/unhide toggle (not only while already hidden).
            if let Err(e) = register_hide_peek_hotkey(&app, &hotkey_str) {
                log_message(
                    "warn",
                    &format!(
                        "[Hotkey] Hide peek '{}' registration failed: {}",
                        hotkey_str, e
                    ),
                );
            }
        }
    }

    // Safe Copy/Paste Explorer verbs are a WinCommander install tell. Remove
    // them while hidden/borrowed; restore them on un-hide IF the user had them
    // enabled. Best-effort + off-thread so a slow reg.exe never delays the hide.
    {
        let safe_copy_enabled = crate::settings::read_settings()
            .map(|s| s.app.safe_copy_context_menu_enabled)
            .unwrap_or(false);
        tauri::async_runtime::spawn(async move {
            if hidden {
                let _ = backend::toggle_safe_copy_context_menu(false).await;
            } else if safe_copy_enabled {
                let _ = backend::toggle_safe_copy_context_menu(true).await;
            }
        });
    }

    Ok(())
}

/// FREE Tauri command — enters calculator mode immediately (no PIN required to
/// enter; the PIN gate is still shown when the window is next shown).
/// Hides tray, sets AUMID, resizes to calculator dimensions, writes CalculatorMode=1.
/// Consumed by F1 (dashboard lock button) via invoke('lock_to_calculator').
#[tauri::command]
fn lock_to_calculator(app: tauri::AppHandle) -> Result<(), String> {
    log_message_src("info", "core", "[Calculator] lock_to_calculator invoked");
    // Set AUMID before window.show() so the taskbar reads "Calculator" from first paint.
    set_calculator_taskbar_identity();
    // Write CalculatorMode=1 to the contract registry key (read by R5/NSIS).
    write_wc_registry_dword("CalculatorMode", 1);
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    // enter_calculator_mode handles resize, icon, tray visibility, and show/hide.
    startup_auth::enter_calculator_mode_with(window, true)
}

#[cfg(wincommander_dev_profile)]
fn dev_startup_trace(stage: &str) {
    eprintln!("[wincommander-dev-startup] {stage}");
}

#[cfg(not(wincommander_dev_profile))]
fn dev_startup_trace(_stage: &str) {}

fn setup_tauri_cli_runtime(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Keep the CLI runtime deliberately quiet: no startup registry writes,
    // scheduled tasks, monitors, hotkey registration, updater polling, or
    // visible windows. Commands still receive a real AppHandle, WebviewWindow,
    // plugins, managed state, and the exact production invoke handler.
    sidecar::set_app_handle(app.handle().clone());

    let shield_i = MenuItem::with_id(
        app,
        "shield_toggle",
        "Enable Privacy Shield",
        true,
        None::<&str>,
    )?;
    app.manage(TrayShieldState::new(shield_i));
    app.manage(PanicHotkeyState(Mutex::new("Ctrl+Shift+Q".to_string())));
    app.manage(SearchHotkeyState(Mutex::new("Ctrl+Space".to_string())));
    app.manage(Mutex::new(
        file_watch_trigger::FileWatchTriggerState::default(),
    ));
    app.manage(HideHotkeyState(Mutex::new("Ctrl+Shift+G".to_string())));
    app.manage(CalcModeState(Mutex::new(false)));

    updater::init_headless(app.handle());
    flow_engine::init_headless(app.handle());

    let initialization_script = cli::tauri_initialization_script()?;
    let page_load_script = initialization_script.clone();
    let runner_window = tauri::WebviewWindowBuilder::new(
        app,
        "main",
        tauri::WebviewUrl::App("cli-runtime.html".into()),
    )
    .title("WinCommander CLI Runtime")
    .visible(false)
    .focused(false)
    .skip_taskbar(true)
    .initialization_script(initialization_script)
    .on_page_load(move |window, _payload| {
        if let Err(error) = window.eval(&page_load_script) {
            eprintln!("[wincommander-cli] failed to evaluate runner: {error}");
        }
    })
    .build()?;
    let delayed_window = runner_window.clone();
    let delayed_script = cli::tauri_initialization_script()?;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if let Err(error) = delayed_window.eval(&delayed_script) {
            eprintln!("[wincommander-cli] delayed runner failed: {error}");
        }
    });

    cli::start_tauri_watchdog(app.handle().clone())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli_mode = cli::tauri_runtime_active();
    dev_startup_trace("process start");
    if !cli_mode {
        log_message("info", "[System] WinCommander process starting...");
    }
    // Route every panic — including ones in spawned always-on monitor threads
    // (ransomware/decoy/clipboard) — into the unified log before the default
    // handler runs. The release bin is windows_subsystem="windows", so a bare
    // panic's stderr goes nowhere; without this a detector thread can unwind
    // silently while the user still believes protection is live. KT: log only,
    // never abort, so one monitor's panic can't tear down the whole app.
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "unknown".to_string());
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            log_message("error", &format!("[Panic] {} at {}", msg, location));
            default_hook(info);
        }));
    }
    // Safe Copy (right-click → "Safe Copy") is a HEADLESS task: it records the
    // selection to the safe clipboard and exits, never opening a window. Handle
    // it BEFORE the single-instance guard so it always acts locally instead of
    // being forwarded to (and ignored by) a running instance. Multi-select
    // launches this once per item; safe_clip coalesces them under a mutex.
    // Skipped in CLI mode: `run` has already committed to one specific command,
    // and this scan looks at the whole argv. A `--safe-copy` token that reached
    // argv as some option's value would otherwise hijack the requested command
    // and exit 0 without ever running it.
    #[cfg(windows)]
    if !cli_mode {
        let cli_args: Vec<String> = std::env::args().collect();
        if cli_args.iter().any(|a| a == "--safe-copy") {
            safe_clip::handle_safe_copy_cli(&cli_args);
            std::process::exit(0);
        }
    }

    // Create the kill-on-close Job Object as early as possible so any
    // child process spawned later (Privacy Shield Python in particular)
    // can be added to it and will die when WinCommander dies — even
    // under Task Manager "End task" where the tray-quit cleanup never
    // runs. See src/child_jobs.rs for the why + how.
    child_jobs::init();
    backend::register_p2_commands();
    backend::register_p3_commands();
    backend::register_file_search_commands();

    // Per-session single-instance guard.
    // Each Windows logon session (RDP user) gets exactly one running instance.
    // A duplicate launch in the same session forwards its args to the primary
    // and exits; launches from different sessions are completely independent.
    #[cfg(windows)]
    {
        let cli_args: Vec<String> = std::env::args().collect();
        if !cli_mode && !session_instance::acquire(&cli_args) {
            // Primary instance already running in this session and has received
            // the forwarded args.  Nothing more to do.
            std::process::exit(0);
        }
    }
    // Fail loud if the Edge WebView2 runtime is missing — otherwise the webview
    // window silently never paints (the #1 portable failure mode on a clean
    // host). ReviOS 11 keeps WebView2, so this is a guardrail for other hosts.
    #[cfg(windows)]
    if tauri::webview_version().is_err() {
        if cli_mode {
            cli::abort_tauri_cli(
                9,
                "runtime_prerequisite",
                "Microsoft Edge WebView2 Runtime is required",
            );
        }
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let text: Vec<u16> =
                "Microsoft Edge WebView2 Runtime is required but isn't installed.\r\n\r\nInstall it from https://go.microsoft.com/fwlink/p/?LinkId=2124703 and relaunch.\0"
                    .encode_utf16()
                    .collect();
            let title: Vec<u16> = "WebView2 Runtime Required\0".encode_utf16().collect();
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }

    // Release builds keep encrypted records on disk. Tauri dev/debug builds do
    // the inverse conversion so ordinary tools can inspect the same log while
    // developing; this code is compiled out of release artifacts.
    if !cli_mode {
        if let Ok(log_dir) = paths::user_logs_dir() {
            let log_file = log_dir.join("wincommander.log");
            #[cfg(debug_assertions)]
            log::migrate_logs_to_plaintext_for_debug(&log_file);
            #[cfg(not(debug_assertions))]
            log::migrate_plaintext_logs(&log_file);
            log::purge_old_log_records(&log_file, 7);
        }
    }
    dev_startup_trace("pre-builder work complete");
    let mut context = tauri::generate_context!();
    if cli_mode {
        // The configured desktop window is visible and carries the full React
        // UI. CLI mode builds its own invisible main webview in setup instead.
        context.config_mut().app.windows.clear();
        // Debug builds normally resolve App URLs through Vite. The CLI must be
        // self-contained, so force Tauri's trusted bundled-asset protocol.
        context.config_mut().build.dev_url = None;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        // Autostart at Windows login uses the same maximized window as a manual
        // launch so the Windows taskbar remains available.
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name(paths::app_display_name())
                .args(["--autostart"])
                .build(),
        )
        // Single-instance + arg-forwarding is handled by session_instance::acquire()
        // (called above in run()) and session_instance::start_pipe_listener()
        // (called inside setup() below).  Each Windows logon session gets its
        // own independent instance, so multiple RDP users can run simultaneously.
        .setup(move |app| {
            if cli_mode {
                // Tauri turns a setup Err into `panic!("Failed to setup app")`,
                // which exits 101 with nothing on stdout — the one thing the CLI
                // contract promises never happens. Report it as JSON ourselves.
                return match setup_tauri_cli_runtime(app) {
                    Ok(()) => Ok(()),
                    Err(error) => cli::abort_tauri_cli(
                        9,
                        "runtime_error",
                        &format!("the CLI runtime failed to start: {error}"),
                    ),
                };
            }
            dev_startup_trace("setup entered");
            log_message(
                "info",
                "[System] setup() starting: Initializing tray and hotkeys...",
            );
            // Allow application to start without admin; frontend handles Privilege Guard

            // Install the global AppHandle used by sidecar reader tasks
            // to re-emit Pro-originated notifications (decoy-accessed,
            // voice-lockdown-fired, etc.) back to the frontend after the
            // paid Rust modules move to commander-pro. Must run before
            // any sidecar spawn — placing it at the top of setup() is
            // the simplest way to guarantee that.
            sidecar::set_app_handle(app.handle().clone());

            // Heal a webcam that a prior session's Privacy Shield left denied
            // (app killed / crashed / rebooted while looked-away). The shield is
            // never auto-started at boot, so a present deny-marker means an
            // orphaned deny — restore camera access before anything else can
            // block on it (with the camera denied the shield itself can't start).
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    backend::reconcile_shield_webcam_on_startup(app_handle).await;
                });
            }

            // Start the per-session named-pipe server so secondary instances
            // launched in the same Windows logon session (e.g. context-menu
            // "Scrub" / "Shred" on already-running app) can forward their args.
            // Must run at the top of setup() — before any blocking work — so a
            // hotkey relaunch during cold-start finds a live pipe immediately.
            #[cfg(windows)]
            session_instance::start_pipe_listener(app.handle().clone());

            // Native session-end vault dismount — a reliable backstop for the
            // existing 10s-poll incoming-RDP dismount (useRdpIncomingDismount.ts):
            // that poll can miss its own session ending, since Windows may tear
            // this very process down before the next tick runs. See
            // rdp_session_watch.rs for the full rationale.
            #[cfg(windows)]
            rdp_session_watch::start(app.handle().clone());

            // ── Startup registry flags (contract: read by R5/NSIS) ──────────
            // Write HiddenMode and CalculatorMode DWORDs before any window shows
            // so NSIS / external readers see the correct state from cold-start.
            {
                let is_hidden = wincommander_is_hidden();
                let is_calc = startup_auth::startup_pin_is_configured_sync();
                write_wc_registry_dword("HiddenMode", if is_hidden { 1 } else { 0 });
                write_wc_registry_dword("CalculatorMode", if is_calc { 1 } else { 0 });
                log_message_src(
                    "info",
                    "core",
                    &format!("[Startup] flags: hidden={} calculator={}", is_hidden, is_calc),
                );
            }
            dev_startup_trace("startup flags loaded");

            // ── R2: re-enforce hidden state + start reapply watcher ─────────
            // runtime_visibility::actions::reenforce_hidden_on_startup() and
            // spawn_reapply_watcher() are provided by R2 (no-arg, return ()).
            // They handle their own internal error logging; we just call them.
            // NOTE: reenforce_hidden_on_startup() is now deferred (see below);
            // spawn_reapply_watcher() stays synchronous (it's a background watcher, cheap).
            runtime_visibility::actions::spawn_reapply_watcher();
            dev_startup_trace("runtime visibility watcher started");

            // ── R4: context menus ─────────────────────────────────────────────
            // NOTE: reregister_context_menus_if_enabled() is now deferred (see below).

            // ── Unattended-session guard (SYSTEM task) ──────────────────────
            // Deploy/refresh the SYSTEM task that dismounts local vaults once no
            // session is attending, so the policy holds even when this GUI is
            // closed. Off-thread so it never blocks startup; idempotent + gated
            // on incoming_dismount_on_empty (removes the task when disabled).
            std::thread::spawn(|| {
                if let Err(e) = attend_watch::ensure_attend_watch_task() {
                    crate::log_message("warn", &format!("[AttendWatch] task ensure failed: {e}"));
                }
            });

            // System Tray Setup
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let open_label = format!("Open {}", paths::app_display_name());
            let show_i = MenuItem::with_id(app, "show", &open_label, true, None::<&str>)?;
            let shield_i = MenuItem::with_id(
                app,
                "shield_toggle",
                "Enable Privacy Shield",
                true,
                None::<&str>,
            )?;
            let dismount_all_i = MenuItem::with_id(
                app,
                "dismount_all",
                "Dismount all",
                true,
                None::<&str>,
            )?;
            let menu = Menu::with_items(app, &[&show_i, &shield_i, &dismount_all_i, &quit_i])?;

            app.manage(TrayShieldState::new(shield_i.clone()));
            app.manage(PanicHotkeyState(Mutex::new("Ctrl+Shift+Q".to_string())));
            app.manage(SearchHotkeyState(Mutex::new("Ctrl+Space".to_string())));
            app.manage(Mutex::new(
                file_watch_trigger::FileWatchTriggerState::default(),
            ));
            // Seed the hide hotkey from persisted settings; fall back to the default.
            let saved_hide_hotkey = settings::read_settings()
                .ok()
                .and_then(|s| s.ideal.identity.hide_win_commander_hotkey)
                .unwrap_or_else(|| "Ctrl+Shift+G".to_string());
            app.manage(HideHotkeyState(Mutex::new(saved_hide_hotkey)));

            // Load the tray icon explicitly from the embedded PNG so it
            // always renders correctly. default_window_icon() can return a
            // blank image on Windows when decorations:false is set because
            // the native window has no system-managed icon slot.
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                .expect("tray icon PNG missing");
            let _tray = TrayIconBuilder::with_id("tray")
                .icon(tray_icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        // Spawn so the tray callback returns immediately — block_on()
                        // here stalls the OS message pump and hangs the window.
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = backend::kill_privacy_shield_process().await;
                            // Shield is being torn down from the tray — the
                            // in-app reader won't run to restore the camera,
                            // so restore it here before exiting.
                            backend::restore_shield_webcam_access(app.clone()).await;
                            app.exit(0);
                        });
                    }
                    "show" => {
                        // Shared reveal path: applies the foreground-lock workaround
                        // so a cold --minimized autostart opens on the first click.
                        // Calc-mode lock is handled inside (re-enters the gate).
                        reveal_main_window(app);
                    }
                    "dismount_all" => {
                        // Keep a single source of truth: the WebView listener
                        // invokes the exact Right Sidebar Dismount action, which
                        // owns entitlement checks, progress, toasts, and the
                        // post-operation vault refresh.
                        let _ = app.emit("tray-dismount-all-requested", ());
                    }
                    "shield_toggle" => {
                        if let Some(state) = app.try_state::<TrayShieldState>() {
                            if let Ok(mut running) = state.running.lock() {
                                if *running {
                                    let fleet_owns_shield = settings::read_settings()
                                        .map(|s| s.app.fleet.privacy_shield_session_owned)
                                        .unwrap_or(false);
                                    if fleet_owns_shield {
                                        // The endpoint may still close the application,
                                        // but it cannot stop a shield session that Fleet
                                        // started. Keep the tray state/menu unchanged.
                                        let _ = app.emit("fleet-privacy-shield-control-denied", ());
                                        return;
                                    }
                                    // Flip state + update menu text while the lock is
                                    // held, then drop it before spawning the async kill
                                    // so we never block the callback.
                                    *running = false;
                                    let _ = state.menu_item.set_text("Enable Privacy Shield");
                                    let app = app.clone();
                                    tauri::async_runtime::spawn(async move {
                                        let _ = backend::kill_privacy_shield_process().await;
                                        // Restore the camera the shield denied
                                        // on look-away — the reader can't (its
                                        // process is being killed here).
                                        backend::restore_shield_webcam_access(app).await;
                                    });
                                } else {
                                    let _ = app.emit("tray-shield-toggle-requested", ());
                                }
                            }
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } => toggle_main_window_from_tray(tray.app_handle()),
                        // Double-click is deliberately open-only. On Windows it is
                        // delivered independently from single-click events, so it
                        // must not turn a successful reveal straight back into hide.
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        } => reveal_main_window(tray.app_handle()),
                        _ => {}
                    }
                });

            let calculator_mode_on_startup = startup_auth::startup_pin_is_configured_sync();
            // Cache the startup calc-mode flag so CloseRequested avoids a disk read per close.
            app.manage(CalcModeState(Mutex::new(calculator_mode_on_startup)));
            let tray = _tray.build(app)?;
            // Hide tray icon immediately if WinCommander is in hidden mode.
            // The tray is still built so its event handlers (quit, show) remain
            // functional — it's just invisible.
            if wincommander_is_hidden() || calculator_mode_on_startup {
                let _ = tray.set_visible(false);
            }
            // Keep the TrayIcon alive for the app's lifetime. Without this,
            // the local `tray` variable would drop when setup() returns,
            // triggering Tauri's cleanup path and logging
            // "Error removing system tray icon".
            app.manage(tray);
            log_message("debug", "[System] Tray setup complete.");
            dev_startup_trace("tray setup complete");

            // KT: search-overlay is created lazily on first Ctrl+Space press — NOT at startup.
            // Creating it here caused a Windows window-activation race: the always_on_top
            // transparent overlay stole focus from the main window as it was being shown,
            // which caused the main window to immediately hide itself (CloseRequested→hide
            // path was NOT the cause; the OS auto-minimized a background window).
            // The fix: build the overlay in ensure_search_overlay() on first hotkey use.

            // Get the main window and set it up
            if let Some(window) = app.get_webview_window("main") {
                // Entitlement-FREE startup title. Computing the Pro label here called
                // has_paid_entitlement() -> current_device_hash() -> 2x PowerShell on the
                // main thread BEFORE the window could appear (the 45-70s post-login hang).
                // The React TitleBar effect re-applies the correct edition title on mount,
                // and appearance::apply_on_startup() below still applies any decoy/rebrand
                // label synchronously. A brief default-name title for Pro users until React
                // mounts is the accepted trade-off for a fast cold start.
                let _ = window.set_title(paths::app_display_name());

                // Explicitly set the window icon. The window is decorations:false, so
                // Windows does NOT auto-populate an icon slot from the PE resource —
                // without this the taskbar/Alt-Tab fall back to the generic exe icon
                // (the "icon randomly disappears" bug) in Normal mode and after the
                // auto-reveal. Calculator/restore paths set their own icons separately.
                set_wincommander_window_icon(&window);

                // Appearance mode: if the user left it enabled, override the title
                // (and tray tooltip) with the alternate label before first paint so
                // the real name never flashes in the taskbar / Task Manager.
                appearance::apply_on_startup(app.handle());
                // Re-enforce tray hidden state after apply_on_startup: calling
                // set_tooltip() via Shell_NotifyIcon(NIM_MODIFY) without
                // NIF_STATE|NIS_HIDDEN can un-hide an already-hidden icon on Windows.
                if wincommander_is_hidden() || calculator_mode_on_startup {
                    if let Some(tray) = app.tray_by_id("tray") {
                        let _ = tray.set_visible(false);
                    }
                }

                // Check command line args for a secure-delete path.
                let args: Vec<String> = std::env::args().collect();
                let hidden_mode = wincommander_is_hidden();
                let not_minimized = !args.contains(&"--minimized".to_string());
                // Safe Paste must never bring the window forward for any part of
                // the operation — see session_instance.rs::handle_forwarded_args
                // for the mirrored warm-forward guard. The only UI surfaces are
                // the toast in RightSidebar.tsx's safe-paste-requested listener
                // (success/error), neither of which needs the window shown.
                let is_safe_paste = args.iter().any(|a| a == "--safe-paste");
                let is_direct_context_shred = args.iter().any(|a| a == "--context-shred");
                // Only set skip_taskbar for hidden-mode non-calculator launches; the
                // calculator block below explicitly resets it to false, so setting it
                // here would be immediately undone and leaves the window in a bad state.
                if hidden_mode && !calculator_mode_on_startup {
                    let _ = window.set_skip_taskbar(true);
                }
                if is_safe_paste || is_direct_context_shred {
                    // Skip every window-reveal call below entirely — the window
                    // must stay hidden/backgrounded for the whole Safe Paste
                    // operation, success or failure.
                } else if not_minimized && calculator_mode_on_startup {
                    // The calculator PIN gate must always appear on a manual (non-minimized)
                    // launch, even when hidden mode is active. Without it the app becomes
                    // completely inaccessible — the PIN is the only entry path. Hidden mode
                    // suppression applies to the authenticated WinCommander window, not
                    // to the gate itself.
                    //
                    log_message_src("info", "core", "[Calculator] startup: entering calculator mode");
                    // Keep cold-start and every later calculator entry on the
                    // same native path. Divergent setup here caused a second
                    // transition after React mounted in packaged builds.
                    let _ = startup_auth::enter_calculator_mode_with(window.clone(), true);
                } else if !hidden_mode {
                    let _ = window.set_skip_taskbar(false);
                    let _ = window.maximize();
                    let _ = window.show();
                    set_wincommander_window_icon(&window);
                    let _ = window.set_focus();
                }
                dev_startup_trace("main window reveal requested");

                // Deferred: nothing here gates the window appearing. license/entitlement probes
                // and hide re-enforcement spawn PowerShell (cold WMI = tens of seconds on a
                // fresh boot) — running them on the setup thread is what made the window take
                // 45-70s to appear. Do them off-thread once the window is already up.
                let app_for_deferred = app.handle().clone();
                std::thread::spawn(move || {
                    let _ = crate::license::has_paid_entitlement();        // warms the cache off-thread
                    crate::runtime_visibility::actions::reenforce_hidden_on_startup();
                    crate::backend::reregister_context_menus_if_enabled();
                    crate::log_message_src("info", "core", "[System] deferred startup work complete");
                    crate::log::start_log_sweeper();
                    let _ = &app_for_deferred; // keep the handle alive in case a moved call needs it
                });

                // Cold-start context-menu launches: if launched with a path arg
                // (e.g. from a right-click verb when the app wasn't running), this
                // IS the primary instance so session_instance::acquire() returned
                // true and we reach here. Emit the matching event after a short
                // delay to let the frontend mount its listener.
                //
                // Event resolution routes through session_instance::resolve_context_menu_event
                // — the single shared verb→event map used by both this cold-start path
                // and the warm-forward path, so a new verb only needs to be taught once
                // (see that function's doc comment and AGENTS.md's "a missing branch
                // SECURE-DELETES the target" gotcha).
                let menu_paths: Vec<String> = args[1..]
                    .iter()
                    .filter(|p| !p.starts_with("--"))
                    .cloned()
                    .collect();
                if is_direct_context_shred {
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        context_menu_shred::log_result(
                            context_menu_shred::execute(app_handle.clone(), menu_paths).await,
                        );
                        app_handle.exit(0);
                    });
                } else if !menu_paths.is_empty() {
                    let event = session_instance::resolve_context_menu_event(|flag| {
                        args.iter().any(|a| a == flag)
                    });
                    let app_handle = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1800));
                        let _ = app_handle.emit(event, &menu_paths);
                    });
                }

                // Window is ready

                // Re-apply any manifest hides from a previous session.
                // Runs in the background so it doesn't delay the first paint.
                // Idempotent: hide_runtime skips entries that are already in
                // the correct registry state.
                std::thread::spawn(|| {
                    if let Ok(st) = runtime_visibility::state::load() {
                        for entry in st.entries.into_iter().filter(|e| e.applied) {
                            let _ = runtime_visibility::actions::hide_runtime(entry.key);
                        }
                    }
                });

                // Handle Close Event.
                //   • Calculator lock ARMED → re-lock: hide window, mark locked,
                //     tell the frontend to show the gate, NO tray. The next reveal
                //     (peek hotkey / relaunch) is the calculator gate, never the real
                //     UI — even if this close came from an unlocked (disguised) session.
                //   • Not armed → hide-to-tray: keep a tray to reopen from (unless full
                //     hidden mode is on, where the peek hotkey is the only re-entry).
                let window_clone = window.clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                    use tauri::Emitter;
                    api.prevent_close();
                    // Read settings ONCE to avoid a TOCTOU race: if a concurrent
                    // patch_settings_cmd removes the PIN between two separate reads,
                    // armed=true+lock_on_close=true could enter calculator mode with
                    // no real PIN set → user lockout. One snapshot drives both.
                    let snap = settings::read_settings().ok();
                    let armed = snap.as_ref()
                        .map(|s| startup_auth::gate_enabled(&s.ideal.privacy.startup_pin))
                        // Fail CLOSED on a settings-read failure: if the snapshot
                        // can't be read at close time (transient datastore/decode
                        // error, or a race with a concurrent write), fall back to
                        // the independent PIN-configured probe instead of defaulting
                        // to "not armed" — which would silently drop the calculator
                        // disguise for this session and reveal the real window on
                        // the next open, defeating the coercion/deniability guard.
                        .unwrap_or_else(startup_auth::startup_pin_is_configured_sync);
                    // "Lock panel on close" (Secret Setting). Resolved default is PIN-aware:
                    // None => ON when a calculator PIN is armed, else OFF. ON+armed = show the
                    // calculator only; OFF (or no PIN) = hide to tray (the next reveal is
                    // Borrowed-locked when locked panels are configured, via persisted lockedPanelIds).
                    let lock_on_close = snap.as_ref()
                        .and_then(|s| s.app.lock_panel_on_close)
                        .unwrap_or(armed);
                    let _ = window_clone.set_skip_taskbar(true);
                    let _ = window_clone.hide();
                    if lock_on_close && armed {
                        set_calc_mode_active(window_clone.app_handle(), true);
                        let _ = window_clone.app_handle().emit("calculator-mode-entered", ());
                    } else {
                        // Re-arm Borrowed Mode. The webview persists across a hide
                        // (we prevent_close + hide, never destroy), so a session
                        // that unlocked borrow via the unlock keyword would leak
                        // that unlocked state through to the next reveal. Emitting
                        // hidden-panels-lock resets the runtime override so the
                        // next reveal shows Borrowed Mode whenever locked panels
                        // are configured — matching "lock off ⇒ borrow on reopen".
                        let has_borrow = snap.as_ref()
                            .and_then(|s| s.app.locked_panel_ids.as_ref())
                            .map(|ids| !ids.is_empty())
                            .unwrap_or(false);
                        if has_borrow {
                            let _ = window_clone.app_handle().emit("hidden-panels-lock", ());
                        }
                        let is_hidden = wincommander_is_hidden();
                        if !is_hidden {
                            if let Some(tray) = window_clone.app_handle().tray_by_id("tray") {
                                let _ = tray.set_visible(true);
                            }
                        }
                    }
                    log_message_src(
                        "info",
                        "core",
                        &format!("[Hide] CloseRequested — armed={} lock_on_close={}", armed, lock_on_close),
                    );
                    }
                    // Self-heal the taskbar / Alt-Tab icon. A decorations:false
                    // window has no frame to hold the icon, so Windows can drop the
                    // WM_SETICON on DPI / theme changes or after the window is
                    // marked unresponsive during a hang — falling back to the
                    // generic exe icon ("icon randomly disappears"). Re-assert it
                    // whenever the window regains focus, matched to the current
                    // title so the calculator disguise keeps its calc icon
                    // (title == "Calculator") and the real app keeps the
                    // WinCommander icon.
                    WindowEvent::Focused(true) => {
                        let is_calc = window_clone.title().map(|t| t == "Calculator").unwrap_or(false);
                        let icon = if is_calc {
                            tauri::image::Image::from_bytes(include_bytes!("../icons/calc.png"))
                        } else {
                            tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))
                        };
                        if let Ok(icon) = icon {
                            let _ = window_clone.set_icon(icon);
                        }
                    }
                    _ => {}
                });
            }
            // Register panic hotkey: Ctrl+Shift+Q fires immediate lockdown.
            // Non-fatal: another app (or a zombie instance) may already hold
            // the shortcut. Log + continue so the app starts; user can pick
            // a different hotkey from Settings.
            if let Err(e) =
                app.global_shortcut()
                    .on_shortcut("Ctrl+Shift+Q", |app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            let _ = app.emit("lockdown-trigger", ());
                        }
                    })
            {
                log_message(
                    "warn",
                    &format!("[Hotkey] Ctrl+Shift+Q registration failed: {}", e),
                );
            }

            // Register search bar hotkey: restore main window and toggle the in-app search bar.
            if let Err(e) =
                app.global_shortcut()
                    .on_shortcut("Ctrl+Space", |app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            handle_search_hotkey(app);
                        }
                    })
            {
                log_message(
                    "warn",
                    &format!("[Hotkey] Ctrl+Space registration failed: {}", e),
                );
            }

            // Register the hide-peek hotkey at startup whenever one is configured
            // (default Ctrl+Shift+G), so it's an always-on hide/unhide toggle rather
            // than only working once the app is already hidden. Clone + drop the
            // lock before register_hide_peek_hotkey to avoid the deadlock that
            // affects update_hide_hotkey / apply_wincommander_hide_mode.
            {
                let startup_hotkey: String = app
                    .try_state::<HideHotkeyState>()
                    .and_then(|hs| hs.0.lock().ok().map(|g| g.clone()))
                    .unwrap_or_default();
                if !startup_hotkey.is_empty() {
                    log_message_src(
                        "info",
                        "core",
                        &format!("[Hotkey] Registering hide-peek hotkey '{}' at startup", startup_hotkey),
                    );
                    if let Err(e) = register_hide_peek_hotkey(app.handle(), &startup_hotkey) {
                        // KT: hotkey registration can fail if another app or a zombie instance
                        // holds the shortcut; retry once after a short yield then log loudly.
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        if let Err(e2) = register_hide_peek_hotkey(app.handle(), &startup_hotkey) {
                            log_message_src(
                                "error",
                                "core",
                                &format!(
                                    "[Hotkey] Hide peek '{}' startup registration failed (first: {}; retry: {})",
                                    startup_hotkey, e, e2
                                ),
                            );
                        } else {
                            log_message_src(
                                "warn",
                                "core",
                                &format!("[Hotkey] Hide peek '{}' registered on retry (first attempt: {})", startup_hotkey, e),
                            );
                        }
                    }
                }
            }

            // Start the Recent Downloads watcher (Downloads folder → dashboard card).
            dev_startup_trace("starting background services");
            activity_watch_autostart::init();
            downloads_watcher::init(app.handle());

            // Start the network-traffic sampler (emits metrics://network every 1s
            // and evaluates the smart alert when enabled).
            net_traffic_alert::init(app.handle());

            // Re-arm the VPN-drop kill switch if it was armed before restart.
            vpn_kill_switch::init_if_armed(app.handle());

            // Initialize the Flow Engine — starts trigger listeners for all enabled flows
            flow_engine::init(app.handle());

            // Initialize the v2 flows bridge (Pro-backed engine): subscribe the
            // Free-side event sources (settings-changed, gaze, monitors) and
            // forward them to the Pro engine. Paid-gated inside; harmless on Free.
            flow_bridge::init(app.handle());

            // Bootstrap the inactivity-timer watchdog. Idempotent —
            // safe to call repeatedly. Auto-resets the timer on startup.
            inactivity_timer::init(app.handle());
            dev_startup_trace("background services initialized");

            // Fleet agent auto-connect on startup: if the user previously
            // enrolled this device (app.fleet.enabled + non-empty serverUrl),
            // forward the saved config to Pro so the agent reconnects without
            // the user re-entering the URL. Spawned as a background task so
            // a slow Pro startup never blocks the UI. Best-effort — a failure
            // is logged as a warning; the user can reconnect via the UI.
            if let Ok(s) = settings::read_settings() {
                // Cheap flag reads only on the setup thread. has_paid_entitlement()
                // calls PowerShell on a cold boot (can take seconds) — gate it
                // inside the spawned task so the setup thread is never blocked.
                if s.app.fleet.enabled && !s.app.fleet.server_url.is_empty() {
                    // Clone the cheap string fields needed inside the task.
                    let fleet_server_url   = s.app.fleet.server_url.clone();
                    let fleet_dispatch     = s.app.fleet.dispatch;
                    let fleet_signing_key  = s.app.fleet.signing_key_pub.clone();
                    let fleet_device_id    = s.device_id.clone();
                    tauri::async_runtime::spawn(async move {
                        // Gate the startup path on the explicit Fleet service claim — the
                        // command wrappers use the same central service gate, but this path
                        // goes straight to the sidecar, so a lapsed-Fleet user with
                        // fleet.enabled in settings
                        // must NOT silently re-enroll. Checked off-thread so PowerShell
                        // (current_device_hash) never runs on the setup thread.
                        if license::require_service_feature("fleet").is_err() { return; }
                        let args = serde_json::json!({
                            "serverUrl": fleet_server_url,
                            "dispatch": fleet_dispatch,
                            "signingKeyPub": fleet_signing_key,
                            // Stable device id so the runtime auto-connect re-enrolls
                            // the SAME fleet row (dedup — matches fleet_connect).
                            "deviceId": fleet_device_id,
                        });
                        if let Err(e) =
                            sidecar::dispatch_paid_command("fleet_agent_configure", args).await
                        {
                            log_message(
                                "warn",
                                &format!("[Fleet] startup auto-connect failed: {e}"),
                            );
                        }
                    });

                    // P2: periodic fleet policy apply. Pull the latest signed
                    // epoch the agent holds and apply it (values + locked paths)
                    // through the signature-verifying apply path.
                    //
                    // Happy-path cadence: 60s (2× the agent heartbeat).
                    // On consecutive Pro-sidecar failures: exponential backoff
                    // capped at 15 min, plus deterministic jitter derived from
                    // the failure count so parallel restarts don't thunderherd.
                    // Resets to 60s on the first successful round-trip.
                    tauri::async_runtime::spawn(async move {
                        // Base delay and cap in seconds.
                        const BASE_SECS: u64 = 60;
                        const MAX_SECS: u64 = 900; // 15 min
                        let mut fail_streak: u32 = 0;

                        loop {
                            // Compute next sleep: 60s normally; backed-off on failures.
                            let sleep_secs = if fail_streak == 0 {
                                BASE_SECS
                            } else {
                                // 60 * 2^(streak-1) capped at 900, plus jitter
                                // seeded from the streak count so it's deterministic
                                // and avoids wall-clock-dependent panics.
                                let exp = BASE_SECS.saturating_mul(1u64 << fail_streak.min(10).saturating_sub(1));
                                let capped = exp.min(MAX_SECS);
                                // Jitter: ±20% of the window, derived from fail_streak
                                // using a simple LCG (no rand dep needed for this range).
                                let jitter_range = capped / 5;
                                let jitter = if jitter_range > 0 {

                                    (u64::from(fail_streak).wrapping_mul(6364136223846793005)
                                        .wrapping_add(1442695040888963407))
                                        % jitter_range
                                } else {
                                    0
                                };
                                capped + jitter
                            };
                            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;

                            let still_on = settings::read_settings()
                                .map(|s| s.app.fleet.enabled && !s.app.fleet.server_url.is_empty())
                                .unwrap_or(false);
                            if !still_on || license::require_service_feature("fleet").is_err() {
                                // Not enrolled or unlicensed — reset streak and keep
                                // checking at the normal cadence.
                                if fail_streak > 0 {
                                    fleet_agent::clear_sidecar_error();
                                }
                                fail_streak = 0;
                                continue;
                            }

                            let posture_result =
                                fleet_agent::fleet_update_posture_snapshot_typed().await;
                            let epoch_result =
                                fleet_agent::fleet_apply_pending_epoch_typed().await;

                            // KT: only genuine IPC/transport failures (Pro unreachable,
                            // pipe dead, spawn failed) count toward fail_streak and the
                            // "pro_unreachable" status. Policy errors (no pinned key,
                            // bad signature, epoch not newer) mean Pro IS reachable but
                            // the epoch can't/shouldn't apply — escalating the unreachable
                            // backoff for these would falsely show "Pro unreachable" while
                            // Pro is fully functional.
                            let posture_transport_fail = matches!(
                                &posture_result,
                                Err(e) if e.is_transport()
                            );
                            let epoch_transport_fail = matches!(
                                &epoch_result,
                                Err(e) if e.is_transport()
                            );
                            let is_sidecar_failure = posture_transport_fail || epoch_transport_fail;

                            if is_sidecar_failure {
                                fail_streak = fail_streak.saturating_add(1);
                                // KT: surface the ACTUAL transport error (e.g.
                                // "PRO_NOT_INSTALLED: ...", "Pro spawn failed: ...",
                                // "Pro response timeout") instead of a generic label —
                                // it's already specific, don't throw it away. Prefer the
                                // posture failure's text; fall back to the epoch failure's.
                                let detail = match (&posture_result, &epoch_result) {
                                    (Err(e), _) if e.is_transport() => e.to_string(),
                                    (_, Err(e)) if e.is_transport() => e.to_string(),
                                    _ => "pro_unreachable".to_string(),
                                };
                                let msg = format!("{detail} (streak {})", fail_streak);
                                fleet_agent::set_sidecar_error(msg.clone());
                                log_message(
                                    if fail_streak >= 3 { "error" } else { "warn" },
                                    &format!("[Fleet] {msg}"),
                                );
                            } else {
                                if fail_streak > 0 {
                                    log_message("info", "[Fleet] Pro sidecar recovered");
                                    fleet_agent::clear_sidecar_error();
                                }
                                fail_streak = 0;
                            }

                            // Log policy-level errors separately (no backoff impact).
                            if let Err(ref e) = posture_result {
                                if !e.is_transport() {
                                    log_message(
                                        "warn",
                                        &format!("[Fleet] posture update skipped: {e}"),
                                    );
                                }
                            }

                            match epoch_result {
                                Ok(v)
                                    if v.get("applied")
                                        .and_then(|b| b.as_bool())
                                        .unwrap_or(false) =>
                                {
                                    log_message(
                                        "info",
                                        &format!(
                                            "[Fleet] applied policy epoch v{}",
                                            v.get("version").and_then(|x| x.as_i64()).unwrap_or(0)
                                        ),
                                    );
                                }
                                Ok(_) => {}
                                // KT: policy errors (no key, bad sig) are logged as warn
                                // but do NOT latch pro_unreachable — the condition above
                                // already handled the transport branch.
                                Err(e) => log_message(
                                    "warn",
                                    &format!("[Fleet] policy apply skipped: {e}"),
                                ),
                            }
                        }
                    });
                }
            }

            // Clean up the one-shot post-install launch task written by the
            // NSIS POSTINSTALL hook. It fires 5s after install to work around
            // WMIC removal on Windows 11 24H2+ (where RunAsUser silently
            // no-ops). Once the app is running we no longer need it.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                use std::process::Command as SysCmd;
                let _ = SysCmd::new("powershell.exe")
                    .args([
                        "-NonInteractive",
                        "-NoProfile",
                        "-WindowStyle",
                        "Hidden",
                        "-Command",
                        &format!(
                            "Unregister-ScheduledTask -TaskName '{}' \
                         -Confirm:$false -ErrorAction SilentlyContinue",
                            paths::scheduled_task_launch_name()
                        ),
                    ])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .spawn();
            }

            // Start the background self-update scheduler (7-day cadence, ~5min
            // adaptive retry while offline). Runs regardless of window state,
            // so it recovers when a machine that booted offline reconnects and
            // keeps checking across long uptimes — neither of which the old
            // dashboard-mounted JS interval guaranteed.
            updater::init(app.handle());

            #[cfg(windows)]
            session_instance::set_app_ready(&app.handle().clone());

            log_message("info", "[System] setup() finished. Application ready.");
            dev_startup_trace("setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Internal response sink for the invisible CLI runtime. It is
            // inert during a normal desktop launch.
            cli::mark_tauri_cli_ready,
            cli::complete_tauri_cli,
            exit_app,
            open_log_file,
            // ── Destructive-action capability (mints a token for catastrophic ops) ──
            authz::request_destructive_confirmation,
            // ── Dev build sentinel (always present; false in release) ──
            devtools::is_dev_build,
            #[cfg(debug_assertions)]
            open_devtools,
            // ── Debug-only dev panel commands (compiled out of release) ──
            #[cfg(debug_assertions)]
            devtools::dev_reset_state,
            #[cfg(debug_assertions)]
            devtools::dev_simulate_event,
            log::write_log_record,
            log::get_log_records,
            log::clear_log_records,
            native_notify::show_native_test_notification,
            native_notify::dismiss_notification_toast,
            native_notify::dismiss_notification_toast_id,
            native_notify::show_test_notification_kind,
            native_notify::show_rdp_idle_warning_native,
            // ── DoH-aware update flow (circumvents DNS blocks of update host) ──
            get_public_ip_trace,
            app_check_for_updates_doh,
            app_install_update_doh,
            updater::app_install_staged_update,
            updater::updater_current_state,
            update_tray_shield_label,
            set_app_display_label,
            backend::run_backend_script,
            activity_watch_autostart::activity_watch_request,
            backend::toggle_context_menu,
            backend::get_context_menu_status,
            backend::toggle_scrub_context_menu,
            backend::get_scrub_context_menu_status,
            backend::toggle_safe_copy_context_menu,
            backend::get_safe_copy_context_menu_status,
            safe_clip::safe_copy_record,
            safe_clip::safe_clip_status,
            safe_clip::safe_paste_prepare,
            backend::connect_rdp,
            backend::set_rdp_credentials,
            backend::kill_privacy_shield_process,
            backend::kill_mstsc_processes,
            backend::get_system_idle_seconds,
            license::get_license_status,
            license::activate_license,
            license::refresh_license,
            license::clear_license_cache,
            license::deactivate_license,
            license::start_trial,
            payments::create_purchase,
            payments::resume_purchase_checkout,
            payments::get_purchase_catalog,
            payments::get_pending_purchase,
            payments::poll_purchase_status,
            payments::reconcile_purchase_status,
            payments::resend_purchase_license,
            payments::forget_pending_purchase,
            // ── Privacy Shield Quota (15 min/day for free tier) ──
            shield_quota::get_shield_quota,
            shield_quota::consume_shield_minutes,
            shield_quota::reset_shield_quota,
            // ── Pro sidecar IPC ──
            sidecar::test_pro_handshake,
            // QA/dev-only round-trip surface — compiled out of release so a
            // compromised WebView cannot reach every Pro command ungated (C3).
            #[cfg(debug_assertions)]
            sidecar::test_pro_dispatch,
            // ── Pro install path resolution + flow ──
            pro_install::get_pro_install_status,
            pro_install::install_pro_binary,
            pro_install::get_defender_status,
            pro_install::fetch_pro_manifest,
            pro_install::delete_pro_binary,
            investigator_install::fetch_investigator_manifest,
            investigator_install::get_investigator_install_status,
            investigator_install::install_investigator_product,
            investigator_install::launch_investigator_product,
            investigator_install::delete_investigator_product,
            // ── F-1 Paste monitor (clipboard credential watcher) ──
            paste_monitor::start_paste_monitor,
            paste_monitor::stop_paste_monitor,
            paste_monitor::paste_monitor_status,
            paste_monitor::set_paste_monitor_categories,
            paste_monitor::get_paste_monitor_categories,
            paste_monitor::set_local_clipboard_guard_rules,
            paste_monitor::get_local_clipboard_guard_rules,
            local_clipboard_rules::load_local_clipboard_guard_rules,
            local_clipboard_rules::save_local_clipboard_guard_rules,
            paste_monitor::get_managed_clipboard_guard_rules,
            paste_monitor::snooze_paste_monitor,
            paste_monitor::paste_monitor_snooze_remaining,
            paste_monitor::cancel_paste_monitor_snooze,
            paste_monitor::get_paste_monitor_recent,
            paste_monitor::clear_paste_monitor_recent,
            paste_monitor::set_paste_monitor_crypto_swap,
            paste_monitor::get_paste_monitor_crypto_swap,
            paste_monitor::set_paste_monitor_auto_clear,
            paste_monitor::get_paste_monitor_auto_clear,
            paste_monitor::set_paste_monitor_auto_clear_on_lock,
            paste_monitor::get_paste_monitor_auto_clear_on_lock,
            // Clipboard Guard health/report surface (Ink Receipt & Clipboard
            // Guard plan §4.2) — content-free health snapshot plus the
            // bounded pending-report queue a future check-in integrator
            // drains into CheckinBody.clipboard_events.
            paste_monitor::get_paste_monitor_health,
            paste_monitor::get_paste_monitor_pending_reports,
            paste_monitor::drain_paste_monitor_pending_reports,
            // ── F-2 File monitor (filesystem sentinel) ──
            file_monitor::start_decoy_monitor,
            file_monitor::stop_decoy_monitor,
            file_monitor::decoy_monitor_status,
            file_monitor::enroll_decoy,
            file_monitor::remove_decoy,
            file_monitor::list_decoys,
            file_monitor::drop_standard_decoys,
            file_monitor::delete_decoy,
            file_monitor::get_decoy_recent,
            file_monitor::clear_decoy_recent,
            file_monitor::set_decoy_read_audit_enabled,
            file_monitor::decoy_read_audit_status,
            file_monitor::get_last_access_tracking_status,
            file_monitor::enable_last_access_tracking,
            // ── F-3 Mass-modify detector ──
            ransomware_monitor::start_ransomware_monitor,
            ransomware_monitor::stop_ransomware_monitor,
            ransomware_monitor::ransomware_monitor_status,
            ransomware_monitor::set_ransomware_config,
            ransomware_monitor::get_ransomware_config,
            ransomware_monitor::get_ransomware_recent,
            ransomware_monitor::clear_ransomware_recent,
            ransomware_monitor::get_ransomware_watched_dirs,
            ransomware_monitor::get_ransomware_extra_dirs,
            ransomware_monitor::set_ransomware_watch_dirs,
            // ── Port monitor (internal recon detector, PAID) ──
            port_monitor::start_network_honeypot,
            port_monitor::stop_network_honeypot,
            port_monitor::network_honeypot_status,
            port_monitor::get_network_honeypot_recent,
            port_monitor::clear_network_honeypot_recent,
            port_monitor::set_network_honeypot_bind_all_interfaces,
            port_monitor::get_network_honeypot_bind_all_interfaces,
            port_monitor::get_network_honeypot_ports,
            port_monitor::set_network_honeypot_port_enabled,
            port_monitor::add_network_honeypot_custom_port,
            port_monitor::remove_network_honeypot_custom_port,
            // ── Wi-Fi Guard (rogue AP detector, PAID) ──
            wifi_check::start_wifi_guard,
            wifi_check::stop_wifi_guard,
            wifi_check::wifi_guard_status,
            wifi_check::get_wifi_guard_recent,
            wifi_check::clear_wifi_guard_recent,
            wifi_check::get_wifi_guard_known,
            wifi_check::clear_wifi_guard_known,
            wifi_check::add_wifi_guard_ssid,
            // ── #4 Remote-session monitor (PAID, Pro detector via B2) ──
            remote_sessions::start_remote_access_monitor,
            remote_sessions::stop_remote_access_monitor,
            remote_sessions::remote_access_monitor_status,
            remote_sessions::get_remote_access_recent,
            remote_sessions::clear_remote_access_recent,
            remote_sessions::get_remote_access_tools,
            remote_sessions::set_remote_access_tool_enabled,
            // ── #5 Screen privacy (PAID detect via B2) + own-window protect ──
            set_capture_protection,
            screen_privacy::start_screen_capture_watch,
            screen_privacy::stop_screen_capture_watch,
            screen_privacy::screen_capture_watch_status,
            screen_privacy::get_recent_screen_capture,
            screen_privacy::clear_recent_screen_capture,
            // ── #6 Driver health (PAID scan/watch via B2) + FREE Device Manager ──
            driver_health::get_driver_health,
            driver_health::start_driver_watch,
            driver_health::stop_driver_watch,
            driver_health::driver_watch_status,
            driver_health::open_device_manager,
            // ── A3 BYOVD / loldrivers detection (PAID read, ungated) ──
            driver_health::get_vulnerable_drivers,
            // ── Disposable isolation — Hyper-V VM + Windows Sandbox (PAID via Pro) ──
            vm_sandbox::vm_capabilities,
            vm_sandbox::vm_list,
            vm_sandbox::vm_enable_feature,
            vm_sandbox::vm_create,
            vm_sandbox::vm_start,
            vm_sandbox::vm_stop,
            vm_sandbox::vm_destroy,
            vm_sandbox::sandbox_launch,
            vm_sandbox::sandbox_close,
            // ── Access & session monitor (auth-anomaly, PAID via Pro) ──
            auth_anomaly::start_auth_anomaly_monitor,
            auth_anomaly::stop_auth_anomaly_monitor,
            auth_anomaly::auth_anomaly_status,
            auth_anomaly::get_auth_anomaly_recent,
            auth_anomaly::clear_auth_anomaly_recent,
            // ── Session Assurance (insider-risk / attention monitor, PAID via Pro) ──
            session_assurance::start_session_monitor,
            session_assurance::stop_session_monitor,
            session_assurance::session_monitor_status,
            session_assurance::get_session_score,
            session_assurance::get_active_alerts,
            // ── Argus — app-usage / productivity monitor (PAID, Pro collector) ──
            argus::argus_app_usage_start,
            argus::argus_app_usage_stop,
            argus::argus_app_usage_status,
            argus::argus_app_usage_recent,
            // ── Argus — print + removable-media monitor (PAID, Pro collector) ──
            argus::argus_print_usb_start,
            argus::argus_print_usb_stop,
            argus::argus_print_usb_status,
            argus::argus_print_usb_recent,
            // ── Argus — tamper/evasion detector (PAID, Pro collector) ──────────
            argus::argus_tamper_start,
            argus::argus_tamper_stop,
            argus::argus_tamper_status,
            argus::argus_tamper_recent,
            // ── Argus — DLP / exfil monitor (PAID, Pro collector) ──────────────
            argus::argus_dlp_start,
            argus::argus_dlp_stop,
            argus::argus_dlp_status,
            argus::argus_dlp_recent,
            argus_monitoring_mirror,
            // ── Canary tokens (paid; self-host HTTP beacon canaries) ──
            canary_tokens::generate_canary,
            canary_tokens::list_canaries,
            canary_tokens::delete_canary,
            canary_tokens::start_canary_listener,
            canary_tokens::stop_canary_listener,
            canary_tokens::canary_listener_status,
            canary_tokens::get_canary_recent,
            canary_tokens::clear_canary_recent,
            // ── USB control suite — U-A device-attach/detach timeline (paid) ──
            usb_monitor::start_usb_monitor,
            usb_monitor::stop_usb_monitor,
            usb_monitor::usb_monitor_status,
            usb_monitor::get_usb_timeline,
            usb_monitor::get_usb_storage_volumes,
            usb_monitor::clear_usb_timeline,
            usb_monitor::set_usb_monitor_notify,
            // ── USB U-B: data-transfer metering (paid · observability) ──
            usb_metering::start_usb_metering,
            usb_metering::stop_usb_metering,
            usb_metering::usb_metering_status,
            usb_metering::get_usb_transfer_stats,
            usb_metering::clear_usb_transfer_stats,
            usb_metering::set_usb_metering_config,
            // ── USB U-C: HID-injection / BadUSB guard (paid · detection only) ──
            usb_hid_guard::start_usb_hid_guard,
            usb_hid_guard::stop_usb_hid_guard,
            usb_hid_guard::usb_hid_guard_status,
            usb_hid_guard::get_usb_hid_alerts,
            usb_hid_guard::clear_usb_hid_alerts,
            usb_hid_guard::usb_hid_guard_allow_device,
            usb_hid_guard::usb_hid_guard_disallow_device,
            usb_hid_guard::usb_hid_guard_allow_list,
            // ── USB U-D trust policy + U-E per-volume read-only (paid · Pro enforcement) ──
            usb_policy::block_usb_device,
            usb_policy::allow_usb_device,
            usb_policy::set_usb_volume_readonly,
            usb_policy::quarantine_usb_device,
            usb_policy::usb_device_trust_score,
            // ── USB U-F: auto-sandbox / quarantine orchestration (Free decision layer) ──
            usb_auto_sandbox::start_usb_autosandbox,
            usb_auto_sandbox::stop_usb_autosandbox,
            usb_auto_sandbox::usb_autosandbox_status,
            usb_auto_sandbox::set_usb_autosandbox_config,
            usb_auto_sandbox::get_usb_autosandbox_recent,
            usb_auto_sandbox::clear_usb_autosandbox_recent,
            // ── #10 AI Security Advisor — FREE context assembler ──
            advisor::advisor_build_context,
            // ── Network guard quick-toggles (PAID) ──
            network_guard::get_ping_block_status,
            network_guard::set_ping_block,
            // ── Startup PIN gate (Calculator cover, PAID) ──
            startup_auth::verify_startup_pin,
            startup_auth::startup_pin_is_configured,
            startup_auth::register_startup_pin,
            startup_auth::set_startup_pin_enabled,
            startup_auth::clear_startup_pin,
            startup_auth::enter_calculator_mode,
            startup_auth::exit_calculator_mode,
            // ── Calculator lock (FREE — dashboard lock button, contract with F1) ──
            lock_to_calculator,
            // ── F-5 Shortcut actions trigger (PAID) ──
            shortcut_actions::start_lockdown_words,
            shortcut_actions::stop_lockdown_words,
            shortcut_actions::lockdown_words_status,
            shortcut_actions::register_lockdown_words,
            shortcut_actions::set_lockdown_words,
            shortcut_actions::list_lockdown_words,
            shortcut_actions::test_fire_lockdown_words,
            // ── Distress phrase (method C keyboard hook + method D palette) ──
            shortcut_actions::register_distress_phrase,
            shortcut_actions::set_distress_phrases,
            shortcut_actions::list_distress_phrases,
            shortcut_actions::check_distress_phrase,
            backend::lockdown,
            backend::full_lockdown,
            backend::run_bleachbit_clean,
            // ── F9 Group Policy / ADMX managed-policy overlay (Free-side, read-only) ──
            gpo_policy::get_managed_policy,
            // ── Unified Settings ──
            settings::get_settings,
            settings::set_settings,
            settings::patch_settings_cmd,
            settings::set_decoy_mode,
            autostart::ensure_autostart_task,
            attend_watch::ensure_attend_watch_task,
            attend_watch::remove_attend_watch_task,
            autostart::remove_autostart_task,
            autostart::update_autostart_task_identity,
            settings::get_setting,
            settings::get_settings_hash_cmd,
            settings::get_device_identity,
            settings::apply_admin_config_cmd,
            settings::is_setting_locked,
            settings::export_settings_cmd,
            settings::import_settings_cmd,
            settings::write_settings_export_file,
            settings::read_settings_import_file,
            // ── Convergence Engine ──
            settings::get_drift_report,
            settings::update_current_state,
            // ── Server Apps (native webviews) ──
            server_apps::open_server_app,
            server_apps::hide_all_server_apps,
            server_apps::resize_server_app,
            server_apps::close_server_app,
            server_apps::close_all_server_apps,
            update_panic_hotkey,
            update_search_hotkey,
            update_hide_hotkey,
            backend::list_search_storage_roots,
            backend::list_search_known_folders,
            backend::search_everything,
            backend::search_everything_count,
            // Folder the user was last looking at in Explorer — powers the
            // quick-search overlay's "in this folder" scope suggestion.
            explorer_context::get_foreground_explorer_folder,
            // ── File-content search (FTS, Free tier) ──
            file_search::search_content,
            file_search::content_index_status,
            file_search::content_index_configure,
            file_search::content_rescan,
            file_search::content_reindex,
            file_search::content_get_doc,
            backend::get_file_icon_data,
            backend::open_path,
            backend::open_event_log,
            search_actions::search_copy_path,
            search_actions::search_set_file_clipboard,
            search_actions::search_open_containing_folder,
            search_actions::search_open_in_vscode,
            search_actions::search_delete_to_recycle_bin,
            search_actions::search_shred_direct,
            search_actions::search_rename_file,
            search_actions::search_show_properties,
            backend::is_path_dir,
            // ── Disk Space Analyzer ──
            disk_analyzer::run_disk_scan,
            disk_analyzer::get_disk_children,
            disk_analyzer::get_large_disk_items,
            disk_analyzer::disk_delete_item,
            // ── Preview-first routine cache/database cleaner ──
            routine_cleaner::routine_cleaner_scan,
            routine_cleaner::routine_cleaner_clean,
            routine_cleaner::routine_cleaner_cancel,
            // ── Duplicate and empty-folder tools ──
            duplicate_finder::duplicate_finder_scan,
            duplicate_finder::duplicate_finder_remove,
            duplicate_finder::duplicate_finder_cancel,
            empty_folder_cleaner::empty_folder_cleaner_scan,
            empty_folder_cleaner::empty_folder_cleaner_remove,
            empty_folder_cleaner::empty_folder_cleaner_cancel,
            // ── Conservative registry/shell hygiene ──
            registry_hygiene::registry_cleaner_scan,
            registry_hygiene::registry_cleaner_remove,
            explorer_context_menu::explorer_context_menu_scan,
            explorer_context_menu::explorer_context_menu_remediate,
            // ── Pro malware scanning (trusted safety-context wrappers) ──
            malware_scan::malware_scan_start,
            malware_scan::malware_scan_status,
            malware_scan::malware_allowlist_add,
            malware_scan::malware_allowlist_remove,
            malware_scan::malware_quarantine,
            malware_scan::malware_quarantine_restore,
            malware_scan::malware_quarantine_delete,
            malware_scan::malware_quarantine_list,
            security_data::security_threat_snapshot,
            security_data::security_cve_snapshot,
            // ── Shortcut/environment/uninstall hygiene ──
            shortcut_cleaner::shortcut_cleaner_scan,
            shortcut_cleaner::shortcut_cleaner_remove,
            shortcut_cleaner::shortcut_cleaner_cancel,
            environment_cleaner::environment_cleaner_scan,
            environment_cleaner::environment_cleaner_repair,
            uninstall_leftovers::uninstall_leftovers_scan,
            uninstall_leftovers::uninstall_leftovers_remove,
            uninstall_leftovers::uninstall_leftovers_cancel,
            // ── Preview-first ARP cache maintenance ──
            network_maintenance::arp_cache_scan,
            network_maintenance::arp_cache_clear,
            // ── Startup impact + safe driver maintenance seam ──
            startup_maintenance::startup_impact_scan,
            driver_maintenance::driver_maintenance_inventory,
            driver_maintenance::driver_update_seam,
            // ── Package-manager and firewall maintenance ──
            package_updates::package_updates_inventory,
            package_updates::package_updates_apply,
            package_updates::package_updates_cancel,
            firewall_audit::firewall_audit_preview,
            firewall_audit::firewall_audit_remediate,
            firewall_audit::firewall_audit_cancel,
            // ── Network toggle (free, in-process firewall block) ──
            network_toggle::internet_kill_switch_set,
            network_toggle::internet_kill_switch_get,
            // ── VPN-drop kill switch (free, tunnel watchdog) ──
            vpn_kill_switch::vpn_kill_switch_arm,
            vpn_kill_switch::vpn_kill_switch_status,
            // ── Recent downloads (free, Downloads-folder watcher) ──
            downloads_watcher::get_recent_downloads,
            // ── Live metric sampler + reusable per-metric alerting (paid setter) ──
            net_traffic_alert::metric_alerts_set_config,
            net_traffic_alert::metric_alerts_get_config,
            // ── Appearance mode (paid setter — rebrand app) ──
            appearance::decoy_mode_set,
            appearance::decoy_mode_get,
            // ── F-6 File-watch lockdown trigger ──
            file_watch_trigger::start_file_watch_triggers,
            file_watch_trigger::stop_file_watch_triggers,
            // ── Live System Metrics (Rust-native, replaces PS poll) ──
            system_metrics::get_live_metrics,
            system_metrics::get_top_processes,
            system_metrics::get_drive_smart_health,
            system_metrics::get_wipe_drive_list,
            // ── Storage capability probe (free, READ-ONLY secure-erase detection) ──
            storage_probe::probe_drive_capabilities,
            // ── Flow Engine ──
            flow_engine::get_flows,
            flow_engine::save_flow,
            flow_engine::delete_flow,
            flow_engine::toggle_flow,
            flow_engine::get_flow_executions,
            flow_engine::fire_flow,
            flow_engine::list_usb_devices,
            flow_engine::list_backend_commands,
            flow_engine::preflight_validate_flow,
            flow_engine::reload_flows,
            flow_bundle::export_flow_bundle,
            flow_bundle::verify_flow_bundle,
            flow_bundle::import_flow_bundle,
            flow_bundle::get_flow_signer_pubkey,
            flow_capabilities::probe_flow_capabilities,
            flow_health::get_flow_health,
            // ── Flows v2 (Pro-backed engine bridge) ──
            flow_bridge::flow_list_rules,
            flow_bridge::flow_save_rule,
            flow_bridge::flow_delete_rule,
            flow_bridge::flow_set_enabled,
            flow_bridge::flow_fire_now,
            file_metadata::scrub_metadata_paths,
            file_metadata::get_metadata_scrubber_status,
            print_log::get_print_audit_status,
            print_log::set_print_audit_enabled,
            print_log::get_print_audit_log,
            evidence::evidence_record,
            evidence::evidence_read,
            evidence::evidence_clear,
            evidence_vault::export_evidence_vault,
            evidence_vault::verify_evidence_vault,
            evidence_vault::export_evidence_affidavit,
            // Feature 5 — crypto-erase: delete TPM vault key (irreversible, paid)
            evidence_vault::delete_vault_tpm_key,
            inactivity_timer::get_dead_mans_switch_config,
            inactivity_timer::set_dead_mans_switch_config,
            inactivity_timer::reset_dead_mans_switch_timer,
            inactivity_timer::clear_dead_mans_switch_fired,
            apply_wincommander_hide_mode,
            wincommander_hidden_status,
            // ── Runtime Visibility Manager (Phase 1+2) ──
            runtime_visibility::scanner::scan_runtimes,
            runtime_visibility::enumerate::enumerate_services,
            runtime_visibility::enumerate::enumerate_scheduled_tasks,
            runtime_visibility::state::runtime_visibility_state,
            runtime_visibility::actions::hide_runtime,
            runtime_visibility::actions::hide_runtime_list,
            runtime_visibility::actions::restore_runtime,
            runtime_visibility::actions::restore_all_runtimes,
            runtime_visibility::actions::set_global_runtime_visibility,
            // ── Fleet agent onboarding (paid) ────────────────────────────
            fleet_agent::fleet_connect,
            fleet_agent::fleet_disconnect,
            fleet_agent::fleet_status,
            fleet_agent::fleet_apply_pending_epoch,
            fleet_agent::fleet_update_posture_snapshot,
            fleet_agent::fleet_report_privacy_shield_status,
            fleet_agent::fleet_sync_shield_state,
            fleet_agent::fleet_report_local_alert,
            fleet_agent::fleet_request_unenroll,
            fleet_agent::fleet_unenroll_status,
            // ── F6 Phase-1 Piece 2 — BootNext UEFI setter (non-destructive) ──
            // NOT wired to any trigger/lockdown path here. The F6 orchestrator
            // (Piece 3) calls these explicitly after stage-1 crypto-erase succeeds.
            reboot_usb::f6_get_boot_next_usb_entry,
            reboot_usb::f6_set_boot_next_usb,
            reboot_usb::f6_get_boot_next,
            reboot_usb::f6_clear_boot_next,
            f6_provision::f6_list_removable_volumes,
            f6_provision::f6_provision_wipe_usb,
            // ── F6 Phase-1 Piece 3 — USB boot chain self-test (paid). Arms a REAL
            // wipe token with a live TTL — see f6_verify_boot.rs module header
            // DANGER section. Not wired to any trigger/lockdown path.
            f6_verify_boot::f6_verify_usb_boot_arm,
            f6_verify_boot::f6_verify_usb_boot_disarm,
            f6_verify_boot::f6_verify_usb_boot_check,
            f6_verify_boot::f6_verify_usb_boot_status,
            // ── Track A — selective in-Windows crypto-erase orchestrator (paid) ──
            selective_erase::erase_encrypted_container,
        ])
        .run(context)
        .expect("error while running tauri application");

    // Release the per-session mutex so a fast restart in the same session
    // can acquire it without waiting for the OS to clean up the exited process.
    #[cfg(windows)]
    if !cli_mode {
        session_instance::release();
    }
}
