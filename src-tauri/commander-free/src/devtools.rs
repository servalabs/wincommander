// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/devtools.rs
//
// ═══════════════════════════════════════════════════════════════════════
// DEV-only helpers — all #[cfg(debug_assertions)] except is_dev_build()
// ═══════════════════════════════════════════════════════════════════════
//
// is_dev_build() is always-compiled so the frontend can gate the dev panel
// at mount time.  It must NOT use import.meta.env.DEV — that is false in a
// Vite *build*, which is exactly what the fleet-kit uses.  The Rust flag is
// the authoritative source.
//
// Debug-only commands (absent from release):
//   dev_reset_state            — wipe local settings store for first-run tests
//   dev_simulate_event         — fake evidence_record + UI notification

/// Always-compiled sentinel that exposes the Rust debug-build flag to the
/// frontend.  Returns true for `cargo build` / `tauri dev`, false for
/// `cargo build --release`.
#[tauri::command]
pub fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

// ── Debug-only commands ────────────────────────────────────────────────────

/// Clear the encrypted settings store so the next app run starts from
/// scratch.  Mirrors `bun run dev:reset` but runs in-process.
/// Deletes %ProgramData%\WinCommander\store\settings.dat only.
/// Does NOT touch the licence cache — that would force a re-activation
/// prompt and is distracting during iterative UI testing.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_reset_state() -> Result<(), String> {
    // The settings section lives at machine_data_dir()/store/settings.dat.
    // We reproduce the path here rather than exposing a section_path helper
    // from datastore (which would require pub(crate) on an internal fn).
    let store_path = crate::paths::machine_data_dir()
        .map(|d| d.join("store").join("settings.dat"))
        .map_err(|e| format!("dev_reset_state: path error: {e}"))?;
    if store_path.exists() {
        std::fs::remove_file(&store_path)
            .map_err(|e| format!("dev_reset_state: remove settings: {e}"))?;
    }
    crate::log::log_message("info", "[DEV] dev_reset_state: settings store wiped");
    Ok(())
}

/// Append a synthetic EvidenceEntry to the local ledger and emit a
/// `dev-sim-event` Tauri event so the frontend can animate a monitor hit.
///
/// `kind` examples: "paste_hit", "decoy_hit", "ransomware_detected",
/// "honeypot_connection", "wifi_rogue_ap"
#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_simulate_event(app: tauri::AppHandle, kind: String) -> Result<(), String> {
    use tauri::Emitter;
    crate::evidence::evidence_record(
        "dev".to_string(),
        "info".to_string(),
        format!("[SIM] {}", kind),
        Some(format!("Simulated event injected by dev panel: {}", kind)),
    )?;
    crate::log::log_message("info", &format!("[DEV] dev_simulate_event: {}", kind));
    app.emit("dev-sim-event", &kind)
        .map_err(|e| format!("emit dev-sim-event: {e}"))?;
    Ok(())
}
