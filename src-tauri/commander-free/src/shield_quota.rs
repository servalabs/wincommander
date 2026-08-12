// src-tauri/src/shield_quota.rs
//
// ═══════════════════════════════════════════════════════════════════════
// PRIVACY SHIELD QUOTA — 15 minutes / day for free-tier users
// ═══════════════════════════════════════════════════════════════════════
//
// Tracks cumulative Privacy Shield usage per local-calendar day. Free
// users get 15 minutes per day; paid (or trial) users are unlimited.
//
// Mechanics
// ---------
//   - Storage: %APPDATA%\WinCommander\shield-quota.json
//     Schema:  { "date": "YYYY-MM-DD", "minutes_used": 12.5 }
//   - Window: per local-time calendar day. The first read after midnight
//     local resets minutes_used to 0 (and updates the date field).
//   - Hard cap: HARD_CAP_MINUTES (currently 15.0). consume_shield_minutes
//     clamps at the cap and reports remaining = 0.
//   - Unlimited bypass: when license::has_paid_entitlement() returns true,
//     the quota is reported as unlimited and consume_shield_minutes is
//     a no-op. We still write to the file so quota state survives a
//     license downgrade gracefully.
//
// Threat model
// ------------
// This is honour-system v1. Plain-text JSON is trivially editable; we
// don't sign / HMAC the file. The acceptable failure mode is "a determined
// abuser resets it manually" — the protection is for casual usage, not
// adversarial. Phase 9 of the rollout adds HMAC-signed quota state when
// abuse becomes visible (see plan §"Open decisions").
//
// API
// ---
//   get_shield_quota()                  -> ShieldQuotaStatus
//   consume_shield_minutes(minutes)     -> ShieldQuotaStatus
//   reset_shield_quota()                -> ShieldQuotaStatus  (testing only)

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::license;

const HARD_CAP_MINUTES: f64 = 15.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QuotaRecord {
    /// YYYY-MM-DD in local time. Reset trigger when this differs from today.
    date: String,
    /// Cumulative minutes used today. Clamped at HARD_CAP_MINUTES.
    minutes_used: f64,
}

#[derive(Debug, Serialize)]
pub struct ShieldQuotaStatus {
    /// True when the user has a paid entitlement (paid license or active
    /// trial). When true, the other counters are still reported but the
    /// frontend should treat them as informational only.
    is_unlimited: bool,
    /// Cumulative minutes used today. 0..=HARD_CAP_MINUTES.
    minutes_used: f64,
    /// Minutes remaining today. HARD_CAP_MINUTES - minutes_used (>= 0).
    minutes_remaining: f64,
    /// The hard cap value, exposed so the UI can show "X / 15 min today"
    /// without hard-coding the constant.
    hard_cap_minutes: f64,
    /// YYYY-MM-DD of the day this quota record applies to (local time).
    date: String,
}

fn quota_file_path() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| "APPDATA not available for shield-quota path".to_string())?;
    let mut path = PathBuf::from(appdata);
    path.push("WinCommander");
    fs::create_dir_all(&path).map_err(|e| format!("Failed to create data directory: {}", e))?;
    path.push("shield-quota.json");
    Ok(path)
}

fn today_local() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn load_record() -> QuotaRecord {
    let today = today_local();
    let Ok(path) = quota_file_path() else {
        return QuotaRecord {
            date: today,
            minutes_used: 0.0,
        };
    };
    if !path.exists() {
        return QuotaRecord {
            date: today,
            minutes_used: 0.0,
        };
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return QuotaRecord {
            date: today,
            minutes_used: 0.0,
        };
    };
    let parsed: QuotaRecord = serde_json::from_str(&raw).unwrap_or(QuotaRecord {
        date: today.clone(),
        minutes_used: 0.0,
    });

    // Auto-reset when the calendar day rolled over.
    if parsed.date != today {
        return QuotaRecord {
            date: today,
            minutes_used: 0.0,
        };
    }
    parsed
}

fn save_record(record: &QuotaRecord) -> Result<(), String> {
    let path = quota_file_path()?;
    let data = serde_json::to_string_pretty(record)
        .map_err(|e| format!("Failed to encode shield quota: {}", e))?;
    fs::write(path, data).map_err(|e| format!("Failed to write shield quota: {}", e))
}

/// Server-side gate used by `run_backend_script` before dispatching
/// `Start-PrivacyShield` for free-tier users. Returns true when the
/// caller has no paid entitlement AND today's counter has reached
/// HARD_CAP_MINUTES. Paid / trial entitlements always return false
/// (unlimited). This is the defence-in-depth for the JS-side check
/// in PrivacyShieldCard — a devtools `invoke("Start-PrivacyShield")`
/// must still be refused once the daily cap is hit.
pub fn is_quota_exhausted() -> bool {
    if license::has_paid_entitlement() {
        return false;
    }
    let record = load_record();
    record.minutes_used >= HARD_CAP_MINUTES
}

fn status_from(record: &QuotaRecord, is_unlimited: bool) -> ShieldQuotaStatus {
    let used = record.minutes_used.clamp(0.0, HARD_CAP_MINUTES);
    ShieldQuotaStatus {
        is_unlimited,
        minutes_used: used,
        minutes_remaining: (HARD_CAP_MINUTES - used).max(0.0),
        hard_cap_minutes: HARD_CAP_MINUTES,
        date: record.date.clone(),
    }
}

#[tauri::command]
pub fn get_shield_quota() -> Result<serde_json::Value, String> {
    let record = load_record();
    // Persist the auto-reset on day rollover so the next call doesn't
    // re-derive the same reset.
    let _ = save_record(&record);
    let status = status_from(&record, license::has_paid_entitlement());
    serde_json::to_value(status).map_err(|e| e.to_string())
}

/// Increments the today counter by `minutes` and returns the updated quota.
/// Paid users skip the increment but still get the current status. Negative
/// values are clamped to 0; the cumulative total is clamped to HARD_CAP_MINUTES.
#[tauri::command]
pub fn consume_shield_minutes(minutes: f64) -> Result<serde_json::Value, String> {
    let is_unlimited = license::has_paid_entitlement();
    let mut record = load_record();

    if !is_unlimited {
        let inc = minutes.max(0.0);
        record.minutes_used = (record.minutes_used + inc).min(HARD_CAP_MINUTES);
        save_record(&record)?;
    }

    let status = status_from(&record, is_unlimited);
    serde_json::to_value(status).map_err(|e| e.to_string())
}

/// Reset today's quota to 0. Internal / dev tool — exposed as a Tauri
/// command so QA and the dev console can verify quota behaviour. Real
/// users should never call this; the only legitimate reset is the
/// midnight rollover handled by `load_record`. The body short-circuits
/// in release builds so a devtools session against a shipped binary
/// cannot zero the counter at will (would defeat the 15-min/day cap).
#[tauri::command]
pub fn reset_shield_quota() -> Result<serde_json::Value, String> {
    if !cfg!(debug_assertions) {
        return Err("reset_shield_quota is a debug-build-only diagnostic.".to_string());
    }
    let record = QuotaRecord {
        date: today_local(),
        minutes_used: 0.0,
    };
    save_record(&record)?;
    let status = status_from(&record, license::has_paid_entitlement());
    serde_json::to_value(status).map_err(|e| e.to_string())
}
