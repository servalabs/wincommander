// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/evidence.rs
//
// ═══════════════════════════════════════════════════════════════════════
// evidence.timeline (FREE tier) — local append-only activity ledger
// ═══════════════════════════════════════════════════════════════════════
//
// Records what WinCommander itself detected/did — defensive detections
// (decoy/paste/ransomware monitor fires, network honeypot hits, wifi-guard
// alerts), protective actions (lockdown runs, metadata scrubs), and flow
// executions — in an authenticated encrypted ledger at
//   %ProgramData%\WinCommander\machine-state\evidence-ledger.jsonl
//
// Single-sink shape per the Track-2 mandate: everything flows through
// `evidence_record(source, severity, summary, detail)`. This is the FREE
// tier — encrypted, unsigned, local. The PAID `evidence.vault` upgrade
// (commander-pro) adds the court-admissible layer (Ed25519 signing +
// RFC 3161 timestamps + WORM export); that lives Pro-side and the timeline
// UI prefers it when present. Diagnostic debug logs (app.log) stay separate.
//
// Note: summaries are intentionally generic ("Lockdown triggered",
// "Honeypot connection") and never contain the engine command-name strings
// guarded by tools/strings-grep-forbidden.txt, so the Free binary stays
// AV-clean.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[path = "fleet_lifecycle_mutex.rs"]
#[allow(dead_code)]
mod mutex;
#[path = "evidence_storage.rs"]
mod storage;

fn acquire_evidence_lock() -> Result<mutex::Guard, String> {
    #[cfg(windows)]
    let result = mutex::acquire_named(r"Global\WinCommander_evidence-ledger_lock", 5_000);
    #[cfg(not(windows))]
    let result = mutex::acquire();
    result.map_err(|_| {
        "Evidence coordination is unavailable; close older WinCommander sessions and retry".into()
    })
}

/// Hard cap on retained entries — the ledger is a rolling recent-activity
/// feed, not the permanent court record (that's the paid vault).
const MAX_ENTRIES: usize = 1000;
const LEDGER_FILENAME: &str = "evidence-ledger.v2";
const LEGACY_MACHINE_LEDGER_FILENAME: &str = "evidence-ledger.jsonl";
const FAILURE_MARKER_FILENAME: &str = "evidence-recording-incomplete.flag";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceEntry {
    /// ISO-8601 UTC timestamp.
    pub time: String,
    /// Origin subsystem: "monitor" | "network" | "lockdown" | "privacy" | "flow" | …
    pub source: String,
    /// "info" | "warn" | "danger".
    pub severity: String,
    /// One-line human summary.
    pub summary: String,
    /// Optional longer detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn ledger_path() -> Result<PathBuf, String> {
    crate::paths::machine_state_file(LEDGER_FILENAME)
}

fn legacy_ledger_path() -> Result<Option<PathBuf>, String> {
    Ok(crate::paths::optional_user_data_location()?
        .map(|local| local.join("evidence").join("ledger.jsonl")))
}

fn key_path() -> Result<PathBuf, String> {
    crate::paths::machine_state_file("evidence-ledger.key")
}

fn read_ledger(path: &std::path::Path, key: &std::path::Path) -> Result<storage::Ledger, String> {
    storage::read_with_machine_source(
        path,
        &crate::paths::machine_state_file(LEGACY_MACHINE_LEDGER_FILENAME)?,
        &legacy_ledger_path,
        key,
        &crate::paths::atomic_write_machine_state,
    )
}

/// Append one entry to the device ledger, trimming to MAX_ENTRIES. Security
/// events are infrequent, so the read-modify-write keeps the file bounded
/// without a separate compaction pass.
#[tauri::command]
pub fn evidence_record(
    source: String,
    severity: String,
    summary: String,
    detail: Option<String>,
) -> Result<(), String> {
    let entry = EvidenceEntry {
        time: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        source,
        severity,
        summary,
        detail,
    };
    let result = record_entry(entry);
    if result.is_err() && mark_record_failure().is_err() {
        crate::log::log_message(
            "warn",
            "Evidence recording and its failure marker could not be saved",
        );
    }
    result
}

fn mark_record_failure() -> Result<(), String> {
    mark_record_failure_at(
        &crate::paths::machine_state_file(FAILURE_MARKER_FILENAME)?,
        &crate::paths::atomic_write_machine_state,
    )
}

fn mark_record_failure_at(
    path: &std::path::Path,
    persist: &impl Fn(&std::path::Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    persist(path, b"evidence-recording-incomplete-v1\n")
}

fn record_entry(entry: EvidenceEntry) -> Result<(), String> {
    let _lock = acquire_evidence_lock()?;
    let path = ledger_path()?;
    let key = key_path()?;
    let mut ledger = read_ledger(&path, &key)?;
    ledger.entries.push(entry);
    if ledger.entries.len() > MAX_ENTRIES {
        let excess = ledger.entries.len() - MAX_ENTRIES;
        ledger.entries.drain(0..excess);
    }
    storage::write(
        &path,
        &key,
        &ledger,
        &crate::paths::atomic_write_machine_state,
    )?;
    Ok(())
}

/// Read the most recent `limit` entries, newest first.
#[tauri::command]
pub fn evidence_read(limit: Option<u32>) -> Result<Vec<EvidenceEntry>, String> {
    let n = limit.unwrap_or(200).min(MAX_ENTRIES as u32) as usize;
    let _lock = acquire_evidence_lock()?;
    let path = ledger_path()?;
    let mut entries = read_ledger(&path, &key_path()?)?.entries;
    entries.reverse();
    entries.truncate(n);
    Ok(entries)
}

/// Read the entire retained ledger (up to MAX_ENTRIES), newest first. The
/// evidence-vault export uses this so the signed WORM bundle covers the full
/// retained record instead of the UI's default recent-window — a court/
/// continuity artifact must not silently omit older events.
pub fn evidence_read_all() -> Result<Vec<EvidenceEntry>, String> {
    let _lock = acquire_evidence_lock()?;
    let path = ledger_path()?;
    let key = key_path()?;
    let mut ledger = read_ledger(&path, &key)?;
    let pending = storage::read_only_status(
        &path,
        &crate::paths::machine_state_file(LEGACY_MACHINE_LEDGER_FILENAME)?,
        &legacy_ledger_path,
        &key,
        &crate::paths::machine_state_file(FAILURE_MARKER_FILENAME)?,
        &crate::paths::atomic_write_machine_state,
    )?;
    ensure_export_continuity(
        ledger.history_gap_unverified || ledger.profile_import_pending || pending,
    )?;
    ledger.entries.reverse();
    Ok(ledger.entries)
}

fn ensure_export_continuity(unverified: bool) -> Result<(), String> {
    if unverified {
        Err("Activity history may be incomplete or older records may need review. Review them before exporting evidence.".into())
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceStatus {
    continuity_unverified: bool,
}

#[tauri::command]
pub fn evidence_status() -> Result<EvidenceStatus, String> {
    let _lock = acquire_evidence_lock()?;
    let continuity_unverified = storage::read_only_status(
        &ledger_path()?,
        &crate::paths::machine_state_file(LEGACY_MACHINE_LEDGER_FILENAME)?,
        &legacy_ledger_path,
        &key_path()?,
        &crate::paths::machine_state_file(FAILURE_MARKER_FILENAME)?,
        &crate::paths::atomic_write_machine_state,
    )?;
    Ok(EvidenceStatus {
        continuity_unverified,
    })
}

/// Clear the ledger (user-initiated). Truncates the file in place.
#[tauri::command]
pub fn evidence_clear() -> Result<(), String> {
    let _lock = acquire_evidence_lock()?;
    let path = ledger_path()?;
    let key = key_path()?;
    let mut ledger = read_ledger(&path, &key)?;
    ledger.entries.clear();
    ledger.skip_future_imports = true;
    storage::write(
        &path,
        &key,
        &ledger,
        &crate::paths::atomic_write_machine_state,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_round_trips_and_omits_empty_detail() {
        let e = EvidenceEntry {
            time: "2026-06-10T10:00:00Z".to_string(),
            source: "monitor".to_string(),
            severity: "danger".to_string(),
            summary: "Decoy file opened".to_string(),
            detail: None,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"severity\":\"danger\""));
        assert!(!json.contains("detail")); // skip_serializing_if
        let back: EvidenceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.summary, "Decoy file opened");
    }

    #[test]
    fn unverified_history_cannot_be_exported_as_complete_evidence() {
        assert!(ensure_export_continuity(true).is_err());
        assert!(ensure_export_continuity(false).is_ok());
    }

    #[test]
    fn recording_failure_marker_is_written_and_reports_write_failure() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("failure.flag");
        assert!(mark_record_failure_at(&marker, &|_, _| Err("injected failure".into())).is_err());
        assert!(!marker.exists());
        mark_record_failure_at(&marker, &crate::datastore_io::atomic_write).unwrap();
        assert_eq!(
            std::fs::read(marker).unwrap(),
            b"evidence-recording-incomplete-v1\n"
        );
    }
}
