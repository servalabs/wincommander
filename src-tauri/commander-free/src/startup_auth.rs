// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/startup_auth.rs
//
// Three-mode startup PIN gate + calculator-window resize helpers.
//
// verify_pin_mode() → "real" | "decoy" | "destroy" | "open" | "wrong"
// "open" = no PINs configured, app starts immediately.
// All three PINs are stored as Argon2id(normalised_pin, salt = per-device
// hash); compared with constant-time equality. The per-device salt + slow KDF
// makes an off-disk brute-force of a short numeric PIN expensive and binds the
// hash to this machine (was fixed-salt single SHA-256 — cracked in ms off-disk).

use argon2::{Argon2, Params};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

const MIN_PIN_LEN: usize = 4;

#[derive(Default)]
struct PinAttemptState {
    consecutive_failures: u32,
    blocked_until: Option<Instant>,
}

impl PinAttemptState {
    fn ensure_allowed(&self, now: Instant) -> Result<(), String> {
        if self.blocked_until.is_some_and(|deadline| now < deadline) {
            return Err("PIN verification temporarily rate limited".to_string());
        }
        Ok(())
    }

    fn record_result(&mut self, now: Instant, mode: &'static str) {
        if mode == "wrong" {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            if self.consecutive_failures >= 5 {
                let exponent = (self.consecutive_failures - 5).min(4);
                self.blocked_until = Some(now + Duration::from_secs(1 << exponent));
            }
        } else {
            *self = Self::default();
        }
    }
}

static PIN_ATTEMPTS: LazyLock<Mutex<PinAttemptState>> =
    LazyLock::new(|| Mutex::new(PinAttemptState::default()));

pub(crate) fn verify_pin_mode_limited(
    pin: &str,
    real_hash: Option<&str>,
    decoy_hash: Option<&str>,
    destroy_hash: Option<&str>,
) -> Result<&'static str, String> {
    let mut attempts = PIN_ATTEMPTS
        .lock()
        .map_err(|_| "PIN verification unavailable".to_string())?;
    let now = Instant::now();
    attempts.ensure_allowed(now)?;
    let mode = verify_pin_mode(pin, real_hash, decoy_hash, destroy_hash);
    attempts.record_result(now, mode);
    Ok(mode)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn normalise_pin(pin: &str) -> String {
    pin.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

pub fn hash_pin(pin: &str) -> String {
    let normalised = normalise_pin(pin);
    // Per-device salt (the machine's device hash) + Argon2id. Cost is identical
    // for every PIN, so the constant-time compare in verify_pin_mode leaks no
    // which-PIN timing signal. Params mirror the datastore (64 MiB, t=2) — ~50ms,
    // imperceptible on the calculator '=' but expensive to brute-force at scale.
    let salt = crate::license::current_device_hash();
    let params = Params::new(65_536, 2, 1, Some(32)).expect("static argon2 params are valid");
    let argon = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(normalised.as_bytes(), salt.as_bytes(), &mut out)
        .expect("argon2 with fixed params + 64-byte salt cannot fail");
    hex::encode(out)
}

/// Reject PINs the calculator can never reproduce on its display, so a user
/// can't lock themselves out with an un-enterable PIN.
///
/// The gate verifies whatever the calculator *shows* after `=`. The display
/// only ever produces a canonical decimal number, which means:
///   • letters can't appear        → digits only
///   • leading zeros are dropped    → no leading "0" ("0000" shows "0", "012" shows "12")
///   • a bare "0" is the idle state → would unlock on any first `=`, so it's banned
fn validate_pin_enterable(plaintext: &str) -> Result<(), String> {
    let n = normalise_pin(plaintext);
    if n.chars().count() < MIN_PIN_LEN {
        return Err(format!("PIN must be at least {MIN_PIN_LEN} digits."));
    }
    if !n.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN must contain only digits — the calculator can't display letters.".into());
    }
    if n.starts_with('0') {
        return Err(
            "PIN can't start with 0 — the calculator drops leading zeros, so it could never be re-entered."
                .into(),
        );
    }
    let digits: Vec<u8> = n.bytes().collect();
    if digits.windows(2).all(|w| w[0] == w[1]) {
        return Err("PIN can't use the same digit repeated.".into());
    }
    if digits.windows(2).all(|w| w[1] == w[0] + 1) || digits.windows(2).all(|w| w[0] == w[1] + 1) {
        return Err("PIN can't be a simple ascending or descending sequence.".into());
    }
    Ok(())
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub(crate) fn gate_enabled(sp: &crate::settings::StartupPinSettings) -> bool {
    sp.enabled.unwrap_or(true) && sp.real_hash.is_some()
}

/// "real" | "decoy" | "destroy" | "open" | "wrong"
pub fn verify_pin_mode(
    pin: &str,
    real_hash: Option<&str>,
    decoy_hash: Option<&str>,
    destroy_hash: Option<&str>,
) -> &'static str {
    if real_hash.is_none() && decoy_hash.is_none() && destroy_hash.is_none() {
        return "open";
    }
    let candidate = hash_pin(pin);
    // Destroy checked first — prevents a destroy PIN accidentally matching
    // a decoy/real hash with identical content (impossible with a good hash,
    // but ordering eliminates the theoretical case).
    if let Some(h) = destroy_hash {
        if ct_eq(&candidate, h) {
            return "destroy";
        }
    }
    if let Some(h) = decoy_hash {
        if ct_eq(&candidate, h) {
            return "decoy";
        }
    }
    if let Some(h) = real_hash {
        if ct_eq(&candidate, h) {
            return "real";
        }
    }
    "wrong"
}

// ── Tauri commands ──────────────────────────────────────────────────

/// Returns "real" | "decoy" | "destroy" | "open" | "wrong".
#[tauri::command]
pub async fn verify_startup_pin(pin: String) -> Result<String, String> {
    let s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    let sp = &s.ideal.privacy.startup_pin;
    if !gate_enabled(sp) {
        return Ok("open".to_string());
    }
    Ok(verify_pin_mode_limited(
        &pin,
        sp.real_hash.as_deref(),
        sp.decoy_hash.as_deref(),
        sp.destroy_hash.as_deref(),
    )?
    .to_string())
}

/// Returns true if the calculator gate should engage. The gate requires a
/// **Real** PIN — a decoy/destroy PIN alone must NOT trap the user behind the
/// calculator with no way into the full app.
#[tauri::command]
pub async fn startup_pin_is_configured() -> Result<bool, String> {
    // Off the async worker: read_settings() is blocking disk IO, and the frontend
    // renders nothing until this resolves — a stalled worker pool would leave the
    // window shown but blank.
    tokio::task::spawn_blocking(|| {
        crate::settings::read_settings()
            .map(|s| gate_enabled(&s.ideal.privacy.startup_pin))
            .map_err(|e| format!("read settings: {e}"))
    })
    .await
    .map_err(|e| format!("startup pin check failed: {e}"))?
}

/// Synchronous version used by lib.rs setup() before the async runtime is active.
pub fn startup_pin_is_configured_sync() -> bool {
    crate::settings::read_settings()
        .map(|s| gate_enabled(&s.ideal.privacy.startup_pin))
        .unwrap_or(false)
}

/// mode: "real" | "decoy" | "destroy". Paid — PIN gate is a Pro feature.
#[tauri::command]
pub async fn register_startup_pin(mode: String, plaintext: String) -> Result<(), String> {
    crate::license::require_paid("startup PIN gate")?;
    validate_pin_enterable(&plaintext)?;
    let hash = hash_pin(&plaintext);
    let mut s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    let sp = &s.ideal.privacy.startup_pin;
    let duplicate_mode = match mode.as_str() {
        "real" => {
            if sp.decoy_hash.as_deref() == Some(hash.as_str()) {
                Some("Decoy PIN")
            } else if sp.destroy_hash.as_deref() == Some(hash.as_str()) {
                Some("Destroy PIN")
            } else {
                None
            }
        }
        "decoy" => {
            if sp.real_hash.as_deref() == Some(hash.as_str()) {
                Some("Real PIN")
            } else if sp.destroy_hash.as_deref() == Some(hash.as_str()) {
                Some("Destroy PIN")
            } else {
                None
            }
        }
        "destroy" => {
            if sp.real_hash.as_deref() == Some(hash.as_str()) {
                Some("Real PIN")
            } else if sp.decoy_hash.as_deref() == Some(hash.as_str()) {
                Some("Decoy PIN")
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(existing) = duplicate_mode {
        return Err(format!(
            "PIN must be different from the existing {existing}."
        ));
    }
    match mode.as_str() {
        "real" => {
            s.ideal.privacy.startup_pin.real_hash = Some(hash);
            s.ideal.privacy.startup_pin.enabled = Some(true);
        }
        "decoy" => s.ideal.privacy.startup_pin.decoy_hash = Some(hash),
        "destroy" => s.ideal.privacy.startup_pin.destroy_hash = Some(hash),
        _ => return Err(format!("Unknown mode: {mode}")),
    }
    crate::settings::write_settings(&s).map_err(|e| format!("write settings: {e}"))
}

#[tauri::command]
pub async fn set_startup_pin_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri::Manager;
    if enabled {
        crate::license::require_paid("startup PIN gate")?;
    }
    let mut s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    if enabled && s.ideal.privacy.startup_pin.real_hash.is_none() {
        return Err("Set a Real PIN before turning on the calculator lock.".into());
    }
    s.ideal.privacy.startup_pin.enabled = Some(enabled);
    let result = crate::settings::write_settings(&s).map_err(|e| format!("write settings: {e}"));
    // Keep CalculatorMode registry DWORD in sync (contract: read by R5/NSIS).
    if result.is_ok() {
        let is_calc = gate_enabled(&s.ideal.privacy.startup_pin);
        crate::write_wc_registry_dword("CalculatorMode", if is_calc { 1 } else { 0 });
        // Turning the lock OFF drops the Calculator disguise live (the session
        // is no longer armed, so exit_calculator_mode wouldn't have restored
        // branding). Hide/unhide via the peek hotkey keeps working — the tray
        // restore here honours Hide-WinCommander mode.
        if !enabled {
            crate::set_calc_mode_active(&app, false);
            if let Some(window) = app.get_webview_window("main") {
                restore_wincommander_identity(&window, "WinCommander");
            }
        }
    }
    result
}

/// mode: "real" | "decoy" | "destroy"
#[tauri::command]
pub async fn clear_startup_pin(mode: String) -> Result<(), String> {
    let mut s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    match mode.as_str() {
        "real" => {
            s.ideal.privacy.startup_pin.real_hash = None;
            s.ideal.privacy.startup_pin.enabled = Some(false);
        }
        "decoy" => s.ideal.privacy.startup_pin.decoy_hash = None,
        "destroy" => s.ideal.privacy.startup_pin.destroy_hash = None,
        _ => return Err(format!("Unknown mode: {mode}")),
    }
    let result = crate::settings::write_settings(&s).map_err(|e| format!("write settings: {e}"));
    // Keep CalculatorMode DWORD in sync — clearing the real PIN disables the gate.
    if result.is_ok() {
        let is_calc = gate_enabled(&s.ideal.privacy.startup_pin);
        crate::write_wc_registry_dword("CalculatorMode", if is_calc { 1 } else { 0 });
    }
    result
}

/// Sets the process AUMID to "Calculator" AND writes the corresponding HKCU
/// registry entry so Task Manager resolves the display name as "Calculator"
/// instead of falling back to the EXE FileDescription ("WinCommander Free…").
/// Called both by lib.rs setup (before first window.show) and by the Tauri
/// command enter_calculator_mode (React path, safety-net for the async case).
#[cfg(windows)]
pub fn write_calculator_process_identity() {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let id: Vec<u16> = "Calculator\0".encode_utf16().collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }

    let key_path: Vec<u16> = "SOFTWARE\\Classes\\AppUserModelId\\Calculator\0"
        .encode_utf16()
        .collect();
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
        if r == 0 {
            let vn: Vec<u16> = "DisplayName\0".encode_utf16().collect();
            let vd: Vec<u16> = "Calculator\0".encode_utf16().collect();
            let _ = RegSetValueExW(
                hkey,
                vn.as_ptr(),
                0,
                REG_SZ,
                vd.as_ptr() as *const u8,
                (vd.len() * 2) as u32,
            );
            let _ = RegCloseKey(hkey);
        }
    }
}

#[cfg(not(windows))]
pub fn write_calculator_process_identity() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_pin_limiter_blocks_after_five_failures_without_global_state() {
        let now = Instant::now();
        let mut attempts = PinAttemptState::default();

        for _ in 0..4 {
            attempts.ensure_allowed(now).unwrap();
            attempts.record_result(now, "wrong");
            assert!(attempts.ensure_allowed(now).is_ok());
        }
        attempts.record_result(now, "wrong");

        assert!(attempts.ensure_allowed(now).is_err());
        assert!(attempts
            .ensure_allowed(now + Duration::from_secs(1))
            .is_ok());
    }

    #[test]
    fn local_pin_limiter_resets_after_a_valid_result() {
        let now = Instant::now();
        let mut attempts = PinAttemptState::default();
        for _ in 0..5 {
            attempts.record_result(now, "wrong");
        }

        attempts.record_result(now + Duration::from_secs(1), "real");

        assert_eq!(attempts.consecutive_failures, 0);
        assert!(attempts.blocked_until.is_none());
        assert!(attempts.ensure_allowed(now).is_ok());
    }

    /// A decoy PIN's hash must never satisfy the real-PIN branch of
    /// verify_pin_mode. If this ever passed, entering the decoy PIN under
    /// coercion would unlock the REAL app instead of the deniable decoy view —
    /// defeating the entire anti-coercion design.
    #[test]
    fn decoy_pin_never_verifies_as_real() {
        let real_hash = hash_pin("135798");
        let decoy_hash = hash_pin("246801");

        let mode = verify_pin_mode("246801", Some(&real_hash), Some(&decoy_hash), None);

        assert_eq!(
            mode, "decoy",
            "the decoy PIN must classify as decoy, never as real"
        );
        assert_ne!(
            mode, "real",
            "a decoy PIN must never verify as the real PIN"
        );
    }

    /// destroy / decoy / real are three distinct outcomes for three distinct
    /// PINs — none may collapse into another. If e.g. the destroy PIN also
    /// matched as decoy, a coerced destroy-PIN entry could show the decoy
    /// view instead of wiping, or vice versa.
    #[test]
    fn destroy_pin_is_distinct_from_decoy() {
        let real_hash = hash_pin("135798");
        let decoy_hash = hash_pin("246801");
        let destroy_hash = hash_pin("975312");

        let real_mode = verify_pin_mode(
            "135798",
            Some(&real_hash),
            Some(&decoy_hash),
            Some(&destroy_hash),
        );
        let decoy_mode = verify_pin_mode(
            "246801",
            Some(&real_hash),
            Some(&decoy_hash),
            Some(&destroy_hash),
        );
        let destroy_mode = verify_pin_mode(
            "975312",
            Some(&real_hash),
            Some(&decoy_hash),
            Some(&destroy_hash),
        );

        assert_eq!(real_mode, "real");
        assert_eq!(decoy_mode, "decoy");
        assert_eq!(destroy_mode, "destroy");

        // Pairwise distinctness — no two of the three PINs may classify the same way.
        assert_ne!(real_mode, decoy_mode);
        assert_ne!(real_mode, destroy_mode);
        assert_ne!(decoy_mode, destroy_mode);
    }

    /// A PIN that matches none of the three configured hashes must report
    /// "wrong" — not silently fall through to any of the armed modes. Without
    /// this, a typo or brute-force guess could accidentally be treated as a
    /// valid decoy/destroy/real entry.
    #[test]
    fn unmatched_pin_is_wrong_not_any_configured_mode() {
        let real_hash = hash_pin("135798");
        let decoy_hash = hash_pin("246801");
        let destroy_hash = hash_pin("975312");

        let mode = verify_pin_mode(
            "111222",
            Some(&real_hash),
            Some(&decoy_hash),
            Some(&destroy_hash),
        );

        assert_eq!(mode, "wrong");
    }

    /// startup_pin_is_configured (gate_enabled) must be true ONLY when a Real
    /// PIN is set AND enabled is not explicitly false. A decoy or destroy PIN
    /// alone — with no Real PIN — must NEVER arm the calculator gate, or the
    /// user would be trapped behind the calculator with no way back into the
    /// full app.
    #[test]
    fn gate_requires_real_pin_not_decoy_or_destroy_alone() {
        // Only a decoy hash set, no real hash: gate must stay OFF.
        let decoy_only = crate::settings::StartupPinSettings {
            enabled: None,
            real_hash: None,
            decoy_hash: Some(hash_pin("246801")),
            destroy_hash: None,
        };
        assert!(
            !gate_enabled(&decoy_only),
            "a decoy PIN with no real PIN must never arm the calculator gate"
        );

        // Only a destroy hash set, no real hash: gate must stay OFF.
        let destroy_only = crate::settings::StartupPinSettings {
            enabled: None,
            real_hash: None,
            decoy_hash: None,
            destroy_hash: Some(hash_pin("975312")),
        };
        assert!(
            !gate_enabled(&destroy_only),
            "a destroy PIN with no real PIN must never arm the calculator gate"
        );

        // Real PIN set, enabled left as default (None => treated as true): gate ON.
        let real_default_enabled = crate::settings::StartupPinSettings {
            enabled: None,
            real_hash: Some(hash_pin("135798")),
            decoy_hash: None,
            destroy_hash: None,
        };
        assert!(
            gate_enabled(&real_default_enabled),
            "a real PIN with enabled=None must default to armed"
        );

        // Real PIN set but explicitly disabled: gate must be OFF.
        let real_disabled = crate::settings::StartupPinSettings {
            enabled: Some(false),
            real_hash: Some(hash_pin("135798")),
            decoy_hash: None,
            destroy_hash: None,
        };
        assert!(
            !gate_enabled(&real_disabled),
            "enabled=false must turn the gate off even with a real PIN set"
        );
    }

    /// hash_pin salts the Argon2id KDF with the per-device hash
    /// (crate::license::current_device_hash()). A hash computed under one
    /// device salt must not equal a hash computed for the same PIN under a
    /// different device salt — otherwise a stolen settings file would let a
    /// PIN hash be replayed/verified on different hardware, defeating the
    /// device-binding property documented on hash_pin.
    #[test]
    fn pin_hash_is_device_bound() {
        let pin = "864213";
        let normalised = normalise_pin(pin);

        let real_salt = crate::license::current_device_hash();
        let other_salt = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_ne!(
            real_salt, other_salt,
            "test fixture salt must differ from the real device salt"
        );

        let params = Params::new(65_536, 2, 1, Some(32)).unwrap();

        let mut hash_for_real_device = [0u8; 32];
        Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params.clone(),
        )
        .hash_password_into(
            normalised.as_bytes(),
            real_salt.as_bytes(),
            &mut hash_for_real_device,
        )
        .unwrap();

        let mut hash_for_other_device = [0u8; 32];
        Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(
                normalised.as_bytes(),
                other_salt.as_bytes(),
                &mut hash_for_other_device,
            )
            .unwrap();

        assert_ne!(
            hash_for_real_device, hash_for_other_device,
            "the same PIN hashed under two different device salts must not produce the same hash"
        );

        // hash_pin() itself must agree with the "real device" computation above,
        // proving it really is salted by current_device_hash() and not some
        // fixed/global salt.
        assert_eq!(hex::encode(hash_for_real_device), hash_pin(pin));
    }
}

fn wincommander_hidden_mode_active() -> bool {
    crate::paths::hide_flag_path()
        .map(|path| path.exists())
        .unwrap_or(false)
}

/// Resize + retitle the main window to calculator dimensions.
/// Also shows the window in the taskbar with the calc icon and hides the tray.
#[tauri::command]
pub fn enter_calculator_mode(window: tauri::WebviewWindow) -> Result<(), String> {
    enter_calculator_mode_with(window, true)
}

/// `reveal = false` arms the calculator gate but leaves the window dark (startup
/// paths); `true` puts it on screen (every user-triggered reveal).
///
/// KT: this used to derive "stay dark" from `std::env::args()` containing
/// `--minimized`. argv is process-lifetime state and the autostart Scheduled Task
/// always passes that flag, so an autostarted process latched "stay dark" forever
/// and every later tray/hotkey reveal called hide() instead of show() — the
/// "clicking the tray does nothing" bug. Show-vs-hide is per-call intent, never
/// a property of how the process was launched.
pub fn enter_calculator_mode_with(
    window: tauri::WebviewWindow,
    reveal: bool,
) -> Result<(), String> {
    use tauri::{Emitter, Manager};

    // Mark the runtime state as locked and tell the frontend to show the
    // calculator gate. This is the SINGLE signal every entry path relies on
    // (cold start, dashboard Lock button, lock keyword, tray, peek hotkey,
    // relaunch) — without it the window resizes to calc dimensions but the
    // React gate never renders, leaving a calculator-sized blank real UI.
    crate::set_calc_mode_active(window.app_handle(), true);
    let _ = window.emit("calculator-mode-entered", ());

    crate::log_message_src(
        "info",
        "core",
        &format!("[Calculator] enter_calculator_mode reveal={}", reveal),
    );
    // Set AUMID BEFORE any window.show() call so taskbar sees "Calculator" from frame 1.
    write_calculator_process_identity();

    // Native window calls are presentation only. Never leave a configured PIN
    // gate blank because WebView2/Windows rejects one transition while the
    // packaged app is starting or restoring.
    let _ = window.set_title("Calculator");
    // Un-maximize first — re-locking from an unlocked (disguised) session leaves
    // the window maximized, and Windows ignores set_size while maximized.
    let _ = window.unmaximize();
    let _ = window.set_resizable(false);
    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
        width: 402.0,
        height: 660.0,
    }));
    let _ = window.center();

    // Show window in taskbar with the calculator icon
    let _ = window.set_skip_taskbar(false);
    let calc_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/calc.png")).ok();
    if let Some(icon) = calc_icon.as_ref() {
        let _ = window.set_icon(icon.clone());
    }

    // Calculator mode never exposes a tray icon. A non-revealing entry stays
    // fully dark until the peek hotkey shows this calculator window.
    if let Some(tray) = window.app_handle().tray_by_id("tray") {
        if let Some(icon) = calc_icon {
            let _ = tray.set_icon(Some(icon));
        }
        let _ = tray.set_visible(false);
    }
    if reveal {
        let _ = window.unminimize();
        let _ = window.show();
        // Foreground synchronously here: the reveal usually originates from a
        // background process (tray/hotkey), where show()+set_focus() alone is
        // silently refused by Windows' foreground-activation lock.
        crate::force_window_foreground(&window);
        let _ = window.set_focus();
    } else {
        let _ = window.set_skip_taskbar(true);
        let _ = window.hide();
    }
    Ok(())
}

/// Resize + retitle back to WinCommander after a successful PIN.
/// Restores the app icon, shows the tray, and unblocks resizing.
#[tauri::command]
pub fn exit_calculator_mode(window: tauri::WebviewWindow, title: String) -> Result<(), String> {
    use tauri::Manager;

    crate::log_message_src(
        "info",
        "core",
        "[Calculator] exit_calculator_mode: PIN accepted, unlocking session",
    );

    // No longer locked for THIS session.
    crate::set_calc_mode_active(window.app_handle(), false);

    // CalculatorMode stays 1 — the gate is still configured, so the next
    // cold-start still shows the calculator. R5/NSIS reads it; unlocking for
    // the current session doesn't change the long-term configuration.

    // DISGUISE MODEL: while the calculator lock is armed, the app keeps the
    // Calculator identity (taskbar icon/title/AUMID set by enter_calculator_mode)
    // even after unlock and exposes NO tray — closing re-locks (see CloseRequested
    // in lib.rs). Only when the lock is NOT armed do we restore full WinCommander
    // branding + the tray.
    let armed = startup_pin_is_configured_sync();

    // A valid PIN must never be rejected because Windows declines a cosmetic
    // resize/focus request (common while a packaged app is restoring). The
    // session state above is authoritative; window restoration is best-effort.
    let _ = window.set_resizable(true);
    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
        width: 1200.0,
        height: 800.0,
    }));
    let _ = window.set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize {
        width: 900.0,
        height: 600.0,
    })));
    let _ = window.center();
    let _ = window.set_skip_taskbar(false);
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.maximize();
    let _ = window.set_focus();

    if armed {
        // Keep the calculator disguise; no tray for a calculator-locked install.
        if let Some(tray) = window.app_handle().tray_by_id("tray") {
            let _ = tray.set_visible(false);
        }
    } else {
        // Lock not armed → drop the calculator disguise.
        restore_wincommander_identity(&window, &title);
    }
    Ok(())
}

/// Restore the genuine WinCommander window + tray identity (icon/title/AUMID),
/// undoing the calculator disguise. Tray visibility follows hidden-mode (no tray
/// while Hide WinCommander is on — the peek hotkey is the re-entry). Shared by
/// exit_calculator_mode's lock-not-armed path and set_startup_pin_enabled(false)
/// so turning the calculator lock off drops the Calculator disguise live.
pub fn restore_wincommander_identity(window: &tauri::WebviewWindow, title: &str) {
    use tauri::Manager;
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{RegDeleteKeyW, HKEY_CURRENT_USER};
        use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(std::ptr::null()) };
        let key_path: Vec<u16> = "SOFTWARE\\Classes\\AppUserModelId\\Calculator\0"
            .encode_utf16()
            .collect();
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key_path.as_ptr()) };
    }
    let _ = window.set_title(title);
    if let Ok(app_icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png")) {
        let _ = window.set_icon(app_icon.clone());
        if let Some(tray) = window.app_handle().tray_by_id("tray") {
            let _ = tray.set_icon(Some(app_icon));
            let _ = tray.set_visible(!wincommander_hidden_mode_active());
        }
    }
}
