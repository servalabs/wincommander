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
// executions — as plain JSONL at
//   %ProgramData%\WinCommander\machine-state\evidence-ledger.jsonl
//
// Single-sink shape per the Track-2 mandate: everything flows through
// `evidence_record(source, severity, summary, detail)`. This is the FREE
// tier — plain, unsigned, local. The PAID `evidence.vault` upgrade
// (commander-pro) adds the court-admissible layer (Ed25519 signing +
// RFC 3161 timestamps + WORM export); that lives Pro-side and the timeline
// UI prefers it when present. Diagnostic debug logs (app.log) stay separate.
//
// Note: summaries are intentionally generic ("Lockdown triggered",
// "Honeypot connection") and never contain the engine command-name strings
// guarded by tools/strings-grep-forbidden.txt, so the Free binary stays
// AV-clean.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Hard cap on retained entries — the ledger is a rolling recent-activity
/// feed, not the permanent court record (that's the paid vault).
const MAX_ENTRIES: usize = 1000;
const LEDGER_FILENAME: &str = "evidence-ledger.jsonl";

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

fn legacy_ledger_path() -> Result<PathBuf, String> {
    Ok(crate::paths::user_data_dir()?
        .join("evidence")
        .join("ledger.jsonl"))
}

fn read_lines(path: &PathBuf) -> Vec<String> {
    match fs::read_to_string(path) {
        Ok(c) => c
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|s| s.to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn write_ledger_atomic(path: &std::path::Path, body: &str) -> Result<(), String> {
    crate::paths::atomic_write_machine_state(path, body.as_bytes())
}

fn migrate_legacy_ledger_if_needed(path: &std::path::Path) {
    if path.exists() {
        return;
    }
    let Ok(legacy) = legacy_ledger_path() else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&legacy) else {
        return;
    };
    // Preserve only parseable entries and apply the current retention cap
    // before a user-owned historical file becomes the device-wide ledger.
    let mut lines: Vec<String> = raw
        .lines()
        .filter(|line| serde_json::from_str::<EvidenceEntry>(line).is_ok())
        .map(str::to_string)
        .collect();
    if lines.len() > MAX_ENTRIES {
        lines.drain(..lines.len() - MAX_ENTRIES);
    }
    if lines.is_empty() && !raw.trim().is_empty() {
        return;
    }
    let body = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    if write_ledger_atomic(path, &body).is_ok() {
        let _ = fs::remove_file(legacy);
    }
}

fn read_machine_ledger_path() -> Result<PathBuf, String> {
    let path = ledger_path()?;
    migrate_legacy_ledger_if_needed(&path);
    Ok(path)
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
    let line = serde_json::to_string(&entry).map_err(|e| format!("encode evidence: {}", e))?;

    let _lock = crate::paths::acquire_machine_state_lock("evidence-ledger")?;
    let path = read_machine_ledger_path()?;
    let mut lines = read_lines(&path);
    lines.push(line);
    if lines.len() > MAX_ENTRIES {
        let excess = lines.len() - MAX_ENTRIES;
        lines.drain(0..excess);
    }
    let mut body = lines.join("\n");
    body.push('\n');
    write_ledger_atomic(&path, &body)?;
    Ok(())
}

/// Read the most recent `limit` entries, newest first. Malformed lines are
/// skipped so a torn write never breaks the feed.
#[tauri::command]
pub fn evidence_read(limit: Option<u32>) -> Result<Vec<EvidenceEntry>, String> {
    let n = limit.unwrap_or(200).min(MAX_ENTRIES as u32) as usize;
    let _lock = crate::paths::acquire_machine_state_lock("evidence-ledger")?;
    let path = read_machine_ledger_path()?;
    let mut entries: Vec<EvidenceEntry> = read_lines(&path)
        .iter()
        .filter_map(|l| serde_json::from_str::<EvidenceEntry>(l).ok())
        .collect();
    entries.reverse();
    entries.truncate(n);
    Ok(entries)
}

/// Read the entire retained ledger (up to MAX_ENTRIES), newest first. The
/// evidence-vault export uses this so the signed WORM bundle covers the full
/// retained record instead of the UI's default recent-window — a court/
/// continuity artifact must not silently omit older events.
pub fn evidence_read_all() -> Result<Vec<EvidenceEntry>, String> {
    evidence_read(Some(MAX_ENTRIES as u32))
}

/// Clear the ledger (user-initiated). Truncates the file in place.
#[tauri::command]
pub fn evidence_clear() -> Result<(), String> {
    let _lock = crate::paths::acquire_machine_state_lock("evidence-ledger")?;
    let path = read_machine_ledger_path()?;
    if path.exists() {
        write_ledger_atomic(&path, "")?;
    }
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
}
