// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned read surface — backs the read-only `svc.*` pipe verbs.
//!
//! No coupling to commander-free's encrypted datastore; that migration is
//! deferred to a later Phase-1 step.  This module is intentionally cross-
//! platform so the workspace compiles cleanly on Linux CI.

use wincmd_shared::svc::SVC_PROTOCOL_VERSION;

/// Snapshot of service identity and health returned by `svc.status`.
#[derive(serde::Serialize)]
pub struct SvcStatus {
    pub service: String,
    pub protocol_version: String,
    pub pid: u32,
    pub healthy: bool,
    pub settings_present: bool,
}

/// Build a current-process `SvcStatus`.
pub fn status() -> SvcStatus {
    SvcStatus {
        service: "wincmd-svc".to_string(),
        protocol_version: SVC_PROTOCOL_VERSION.to_string(),
        pid: std::process::id(),
        healthy: true,
        settings_present: svc_settings_path().exists(),
    }
}

/// Canonical path to the service settings file.
///
/// `%ProgramData%\WinCommander\svc-settings.json` on Windows.
/// Falls back to `std::env::temp_dir()` on non-Windows / missing env var.
pub fn svc_settings_path() -> std::path::PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("WinCommander").join("svc-settings.json")
}

/// Return the parsed service settings file, or `{}` on any error.
///
/// Best-effort: IO errors and parse errors both collapse to an empty object.
pub fn get_settings() -> serde_json::Value {
    let path = svc_settings_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return serde_json::json!({}),
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::json!({}))
}

/// Return a static health object for `svc.health`.
pub fn health() -> serde_json::Value {
    serde_json::json!({ "ok": true, "pipe": "up" })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_has_correct_protocol_version_and_nonzero_pid() {
        let s = status();
        assert_eq!(s.protocol_version, SVC_PROTOCOL_VERSION);
        assert_ne!(s.pid, 0, "pid must be nonzero");
        assert_eq!(s.service, "wincmd-svc");
    }

    #[test]
    fn get_settings_returns_object_when_file_absent() {
        // The test environment almost certainly has no svc-settings.json at the
        // canonical path, so this exercises the "file missing → {}" branch.
        let v = get_settings();
        assert!(v.is_object(), "expected a JSON object, got {}", v);
    }
}
