// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/decoy_mode.rs
//
// ═══════════════════════════════════════════════════════════════════════
// DECOY MODE (paid) — rebrand the running app as another application
// ═══════════════════════════════════════════════════════════════════════
//
// When enabled, WinCommander presents itself under a user-chosen custom
// name (when none is set it falls back to the real app name — there is no
// built-in impersonation default):
//   • Native window title  → taskbar, Alt+Tab, and the Task Manager
//     "Processes" tab (which labels a windowed app by its window title).
//   • System-tray tooltip.
//   • In-app title-bar text + About — handled on the frontend, which reads
//     `app.decoyMode` from settings and the `decoy://changed` event.
//
// This is the "B" tier: UI + the Task-Manager-visible process label. It
// does NOT rename the executable on disk or swap the tray icon image.
//
// The persisted preference lives in `settings.app.decoy_mode`; the running
// app is updated immediately by the setter, and re-applied at startup (the
// setup hook calls `apply_on_startup` before the window is shown so the
// title is never briefly "WinCommander").

use tauri::{AppHandle, Emitter, Manager};

/// Fallback label when decoy mode is on but the user left the name blank:
/// the real app name, so a blank cover never impersonates anything — and no
/// "Microsoft …" string is baked into the binary as a default identity.
fn default_decoy_name() -> String {
    crate::paths::app_display_name().to_string()
}

/// Apply the given label to the OS-visible surfaces (window title + tray
/// tooltip). Best-effort: a missing window/tray is non-fatal.
fn apply_label(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(label);
    }
    if let Some(tray) = app.tray_by_id("tray") {
        let _ = tray.set_tooltip(Some(label));
    }
}

/// Engage or release decoy mode. Persists the preference, applies the label
/// (or restores "WinCommander") immediately, and emits `decoy://changed` so
/// the frontend updates its in-app branding. Paid.
#[tauri::command]
pub fn decoy_mode_set(app: AppHandle, enable: bool, display_name: String) -> Result<(), String> {
    crate::license::require_paid("decoy mode")?;

    let name = {
        let trimmed = display_name.trim();
        if trimmed.is_empty() {
            default_decoy_name()
        } else {
            trimmed.to_string()
        }
    };

    // Persist first so a crash between apply + persist can't leave the OS
    // showing a decoy the settings don't know about.
    let patch = serde_json::json!({
        "app": {
            "decoyMode": { "enabled": enable, "displayName": name }
        }
    });
    crate::settings::patch_settings(patch).map_err(|e| format!("persist failed: {}", e))?;

    if enable {
        apply_label(&app, &name);
    } else {
        // Restore the real label. The frontend TitleBar effect also re-pushes
        // the edition label on the decoy://changed event; setting it here too
        // makes the change instant and correct even if the webview is hidden.
        apply_label(&app, crate::paths::app_display_name());
    }

    let _ = app.emit(
        "decoy://changed",
        serde_json::json!({ "enabled": enable, "displayName": name }),
    );
    crate::log_message(
        "info",
        &format!(
            "[Cover] mode {} (label: {})",
            if enable { "ON" } else { "OFF" },
            name
        ),
    );
    Ok(())
}

/// Current decoy state. Free — reading it exposes no paid capability and the
/// frontend needs it to render the toggle + in-app branding.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecoyState {
    pub enabled: bool,
    pub display_name: String,
}

#[tauri::command]
pub fn decoy_mode_get() -> DecoyState {
    let settings = crate::settings::read_settings().ok();
    let dm = settings.map(|s| s.app.decoy_mode).unwrap_or_default();
    DecoyState {
        enabled: dm.enabled,
        display_name: if dm.display_name.trim().is_empty() {
            default_decoy_name()
        } else {
            dm.display_name
        },
    }
}

/// Re-apply decoy mode at startup if it was left enabled. Called from the
/// setup hook before the window is shown so the title never flashes the
/// real name. Best-effort.
pub fn apply_on_startup(app: &AppHandle) {
    if let Ok(settings) = crate::settings::read_settings() {
        let dm = settings.app.decoy_mode;
        if dm.enabled {
            let name = if dm.display_name.trim().is_empty() {
                default_decoy_name()
            } else {
                dm.display_name
            };
            apply_label(app, &name);
        }
    }
}
