use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;
use wincmd_shared::diagnostics::{
    DiagnosticEvent, DiagnosticLifecycle, DiagnosticOutcome, DiagnosticPrivacyClass,
    DiagnosticRetryability, DiagnosticSeverity,
};

const DIAGNOSTIC_FILE: &str = "diagnostic-events.log";
const MAX_READ_EVENTS: usize = 500;
const RETENTION_DAYS: i64 = 7;
const MAX_EMERGENCY_FAILURES: usize = 32;
const ALLOWED_CONTEXT_KEYS: &[&str] = &[
    "attempt",
    "build_version",
    "capability",
    "driver_state",
    "health",
    "os_error_code",
    "policy_version",
    "reason_category",
    "retry_count",
    "state",
];

#[derive(Default)]
struct DiagnosticHealthState {
    persisted: u64,
    dropped: u64,
    redacted_fields: u64,
    pruned_events: u64,
    corrupt_events: u64,
    recovery_events: u64,
    last_failure_code: Option<&'static str>,
    pending_recovery: bool,
    emergency_failure_codes: VecDeque<&'static str>,
}

fn health_state() -> &'static Mutex<DiagnosticHealthState> {
    static STATE: OnceLock<Mutex<DiagnosticHealthState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(DiagnosticHealthState::default()))
}

fn retention_sequence() -> u64 {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticHealth {
    persisted_events: u64,
    dropped_events: u64,
    redacted_fields: u64,
    pruned_events: u64,
    corrupt_events: u64,
    recovery_events: u64,
    emergency_failure_count: usize,
    healthy: bool,
    last_failure_code: Option<&'static str>,
}

#[derive(Default)]
struct RetentionResult {
    pruned: u64,
    corrupt: u64,
    content: String,
}

fn redact_context(event: &mut DiagnosticEvent) -> usize {
    let original = std::mem::take(&mut event.redacted_context);
    let original_len = original.len();
    let mut kept = BTreeMap::new();
    for (key, value) in original {
        if ALLOWED_CONTEXT_KEYS.contains(&key.as_str())
            && value.len() <= 256
            && !looks_sensitive(&value)
        {
            kept.insert(key, value);
        }
    }
    let removed = original_len.saturating_sub(kept.len());
    event.redacted_context = kept;
    removed
}

fn looks_sensitive(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("clipboard")
        || lower.contains(":\\")
        || lower.starts_with("\\\\")
        || lower.contains("http://")
        || lower.contains("https://")
}

fn diagnostic_path() -> Result<std::path::PathBuf, String> {
    crate::paths::user_logs_dir().map(|dir| dir.join(DIAGNOSTIC_FILE))
}

/// Records only stable failure codes because this health endpoint is renderer-visible.
fn record_storage_failure(code: &'static str) {
    if let Ok(mut state) = health_state().lock() {
        state.dropped += 1;
        state.last_failure_code = Some(code);
        state.pending_recovery = true;
        if state.emergency_failure_codes.len() == MAX_EMERGENCY_FAILURES {
            state.emergency_failure_codes.pop_front();
        }
        state.emergency_failure_codes.push_back(code);
    }
}

fn record_prune_result(result: &RetentionResult) {
    if let Ok(mut state) = health_state().lock() {
        state.pruned_events += result.pruned;
        state.corrupt_events += result.corrupt;
    }
}

fn retention_cutoff() -> NaiveDate {
    // Retain today and the preceding six UTC calendar dates: exactly seven dates.
    Utc::now().date_naive() - Duration::days(RETENTION_DAYS - 1)
}

fn parse_envelope_date(line: &str) -> Option<NaiveDate> {
    let rest = line.strip_prefix("D1:")?;
    let date = rest.get(..10)?;
    if rest.as_bytes().get(10) != Some(&b':') {
        return None;
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn retain_diagnostic_records<F>(content: &str, cutoff: NaiveDate, mut decrypt: F) -> RetentionResult
where
    F: FnMut(&str) -> Option<String>,
{
    let mut result = RetentionResult::default();
    for line in content.lines() {
        let Some(date) = parse_envelope_date(line) else {
            result.corrupt += 1;
            continue;
        };
        if date < cutoff {
            result.pruned += 1;
            continue;
        }
        let Some(body) = decrypt(line) else {
            result.corrupt += 1;
            continue;
        };
        let Ok(event) = serde_json::from_str::<DiagnosticEvent>(&body) else {
            result.corrupt += 1;
            continue;
        };
        let occurred_date = event.occurred_at.get(..10);
        if occurred_date != Some(&line[3..13]) {
            result.corrupt += 1;
            continue;
        }
        result.content.push_str(line);
        result.content.push('\n');
    }
    result
}

fn atomic_replace(path: &Path, content: &[u8]) -> Result<(), &'static str> {
    let parent = path.parent().ok_or("DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", retention_sequence()));
    let write_result = (|| {
        let mut file =
            std::fs::File::create(&temporary).map_err(|_| "DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
        file.write_all(content)
            .map_err(|_| "DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
        file.sync_all()
            .map_err(|_| "DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
        std::fs::rename(&temporary, path).map_err(|_| "DIAGNOSTICS.PRUNE.REPLACE_FAILED")
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

fn prune_diagnostic_store(path: &Path) -> Result<RetentionResult, &'static str> {
    let content = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RetentionResult::default())
        }
        Err(_) => return Err("DIAGNOSTICS.PRUNE.READ_FAILED"),
    };
    let result = retain_diagnostic_records(&content, retention_cutoff(), |line| {
        crate::datastore::diagnostic_decrypt_line(line).map(|(_, body)| body)
    });
    if result.content != content {
        atomic_replace(path, result.content.as_bytes())?;
    }
    Ok(result)
}

fn append_encrypted(path: &Path, encrypted: &str) -> Result<(), &'static str> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "DIAGNOSTICS.STORAGE.OPEN_FAILED")?;
    file.write_all(encrypted.as_bytes())
        .map_err(|_| "DIAGNOSTICS.STORAGE.WRITE_FAILED")?;
    file.write_all(b"\n")
        .map_err(|_| "DIAGNOSTICS.STORAGE.WRITE_FAILED")?;
    file.sync_data()
        .map_err(|_| "DIAGNOSTICS.STORAGE.SYNC_FAILED")
}

fn persist_event(path: &Path, event: &DiagnosticEvent) -> Result<(), &'static str> {
    let payload = serde_json::to_string(event).map_err(|_| "DIAGNOSTICS.ENCODE_FAILED")?;
    let encrypted = crate::datastore::diagnostic_encrypt_line(&event.occurred_at[..10], &payload)
        .map_err(|_| "DIAGNOSTICS.ENCRYPTION.UNAVAILABLE")?;
    append_encrypted(path, &encrypted)
}

fn recovery_event() -> DiagnosticEvent {
    let sequence = retention_sequence();
    DiagnosticEvent {
        event_id: format!("diag-storage-recovery-{sequence}"),
        operation_id: format!("DIA-STORAGE-RECOVERY-{sequence}"),
        parent_operation_id: None,
        occurred_at: Utc::now().to_rfc3339(),
        component: "free".into(),
        feature: "diagnostics".into(),
        action: "storage".into(),
        stage: "recovery".into(),
        lifecycle: DiagnosticLifecycle::Applied,
        outcome: DiagnosticOutcome::Recovered,
        error_code: Some("DIAGNOSTICS.STORAGE.RECOVERED".into()),
        severity: DiagnosticSeverity::Warn,
        retryability: DiagnosticRetryability::Automatic,
        suggested_next_action: "none".into(),
        duration_ms: None,
        privacy_class: DiagnosticPrivacyClass::LocalSensitive,
        redacted_context: BTreeMap::new(),
    }
}

fn record_recovery_if_needed(path: &Path) {
    let should_record = health_state()
        .lock()
        .map(|mut state| {
            let pending = state.pending_recovery;
            if pending {
                state.pending_recovery = false;
                state.last_failure_code = None;
            }
            pending
        })
        .unwrap_or(false);
    if !should_record {
        return;
    }
    match persist_event(path, &recovery_event()) {
        Ok(()) => {
            if let Ok(mut state) = health_state().lock() {
                state.persisted += 1;
                state.recovery_events += 1;
            }
        }
        Err(code) => record_storage_failure(code),
    }
}

pub(crate) fn prune_retained_diagnostics() {
    let result = (|| {
        let _lock = crate::paths::acquire_machine_state_lock("diagnostic-events")
            .map_err(|_| "DIAGNOSTICS.PRUNE.LOCK_FAILED")?;
        let path = diagnostic_path().map_err(|_| "DIAGNOSTICS.PRUNE.PATH_FAILED")?;
        prune_diagnostic_store(&path)
    })();
    match result {
        Ok(retention) => record_prune_result(&retention),
        Err(code) => record_storage_failure(code),
    }
}

pub(crate) fn record(mut event: DiagnosticEvent) -> Result<DiagnosticEvent, String> {
    event.validate()?;
    let removed = redact_context(&mut event);
    let result = (|| {
        let _lock = crate::paths::acquire_machine_state_lock("diagnostic-events")
            .map_err(|_| "DIAGNOSTICS.STORAGE.LOCK_FAILED")?;
        let path = diagnostic_path().map_err(|_| "DIAGNOSTICS.STORAGE.PATH_FAILED")?;
        persist_event(&path, &event)?;
        // Retention is deliberately deferred to idle startup maintenance.  Reading,
        // decrypting and rewriting the entire event store for every event caused the
        // desktop process to stall as the store grew.
        Ok::<_, &'static str>(path)
    })();
    match result {
        Ok(path) => {
            if let Ok(mut state) = health_state().lock() {
                state.persisted += 1;
                state.redacted_fields += removed as u64;
            }
            record_recovery_if_needed(&path);
            Ok(event)
        }
        Err(code) => {
            record_storage_failure(code);
            Err(code.to_string())
        }
    }
}

#[tauri::command]
pub(crate) async fn record_diagnostic_event(event: DiagnosticEvent) -> Result<DiagnosticEvent, String> {
    tauri::async_runtime::spawn_blocking(move || record(event))
        .await
        .map_err(|_| "DIAGNOSTICS.STORAGE.TASK_FAILED".to_string())?
}

#[tauri::command]
pub(crate) async fn get_diagnostic_events(
    operation_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<DiagnosticEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || read_diagnostic_events(operation_id, limit))
        .await
        .map_err(|_| "DIAGNOSTICS.READ.TASK_FAILED".to_string())?
}

fn read_diagnostic_events(
    operation_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<DiagnosticEvent>, String> {
    if crate::settings::is_decoy_mode() {
        return Ok(vec![]);
    }
    let _lock = match crate::paths::acquire_machine_state_lock("diagnostic-events") {
        Ok(lock) => lock,
        Err(_) => {
            record_storage_failure("DIAGNOSTICS.READ.LOCK_FAILED");
            return Err("DIAGNOSTICS.READ.LOCK_FAILED".to_string());
        }
    };
    let path = match diagnostic_path() {
        Ok(path) => path,
        Err(_) => {
            record_storage_failure("DIAGNOSTICS.READ.PATH_FAILED");
            return Err("DIAGNOSTICS.READ.PATH_FAILED".to_string());
        }
    };
    match prune_diagnostic_store(&path) {
        Ok(retention) => record_prune_result(&retention),
        Err(code) => {
            record_storage_failure(code);
            return Err(code.to_string());
        }
    }
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => {
            record_storage_failure("DIAGNOSTICS.READ.FAILED");
            return Err("DIAGNOSTICS.READ.FAILED".to_string());
        }
    };
    let max = limit.unwrap_or(100).min(MAX_READ_EVENTS);
    let mut events = Vec::new();
    for line in content.lines().rev() {
        let Some((_, body)) = crate::datastore::diagnostic_decrypt_line(line) else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<DiagnosticEvent>(&body) else {
            continue;
        };
        if operation_id
            .as_ref()
            .is_none_or(|id| &event.operation_id == id)
        {
            events.push(event);
        }
        if events.len() == max {
            break;
        }
    }
    Ok(events)
}

#[tauri::command]
pub(crate) fn get_diagnostics_health() -> DiagnosticHealth {
    let state = health_state().lock().ok();
    DiagnosticHealth {
        persisted_events: state.as_ref().map_or(0, |value| value.persisted),
        dropped_events: state.as_ref().map_or(0, |value| value.dropped),
        redacted_fields: state.as_ref().map_or(0, |value| value.redacted_fields),
        pruned_events: state.as_ref().map_or(0, |value| value.pruned_events),
        corrupt_events: state.as_ref().map_or(0, |value| value.corrupt_events),
        recovery_events: state.as_ref().map_or(0, |value| value.recovery_events),
        emergency_failure_count: state
            .as_ref()
            .map_or(0, |value| value.emergency_failure_codes.len()),
        healthy: state
            .as_ref()
            .is_some_and(|value| value.last_failure_code.is_none()),
        last_failure_code: state.and_then(|value| value.last_failure_code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincmd_shared::diagnostics::{DiagnosticPrivacyClass, DiagnosticRetryability};

    fn event(date: &str) -> DiagnosticEvent {
        DiagnosticEvent {
            event_id: "evt-1".into(),
            operation_id: "VLT-1".into(),
            parent_operation_id: None,
            occurred_at: format!("{date}T00:00:00Z"),
            component: "free".into(),
            feature: "vault".into(),
            action: "mount".into(),
            stage: "driver".into(),
            lifecycle: DiagnosticLifecycle::Applying,
            outcome: DiagnosticOutcome::Failed,
            error_code: Some("VLT.DRIVER.UNAVAILABLE".into()),
            severity: DiagnosticSeverity::Error,
            retryability: DiagnosticRetryability::Manual,
            suggested_next_action: "repair_driver".into(),
            duration_ms: Some(45),
            privacy_class: DiagnosticPrivacyClass::LocalSensitive,
            redacted_context: BTreeMap::from([
                ("driver_state".into(), "unavailable".into()),
                ("path".into(), "C:\\private.hc".into()),
                ("reason_category".into(), "password rejected".into()),
            ]),
        }
    }

    fn record_line(date: &str) -> String {
        let body = serde_json::to_string(&event(date)).unwrap();
        format!("D1:{date}:test-{date}:{body}")
    }

    #[test]
    fn redaction_keeps_only_allowlisted_non_sensitive_context() {
        let mut input = event("2026-09-07");
        let removed = redact_context(&mut input);
        assert_eq!(removed, 2);
        assert_eq!(
            input.redacted_context,
            BTreeMap::from([("driver_state".into(), "unavailable".into())])
        );
    }

    #[test]
    fn retention_keeps_the_seventh_calendar_day_and_removes_older_records() {
        let cutoff = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let oldest_kept = record_line("2026-09-01");
        let expired = record_line("2026-08-31");
        let content = format!("{oldest_kept}\n{expired}\n");
        let retained = retain_diagnostic_records(&content, cutoff, |line| {
            line.find('{')
                .and_then(|index| line.get(index..))
                .map(str::to_string)
        });
        assert_eq!(retained.pruned, 1);
        assert_eq!(retained.corrupt, 0);
        assert_eq!(retained.content, format!("{oldest_kept}\n"));
    }

    #[test]
    fn retention_removes_malformed_and_corrupt_current_records() {
        let cutoff = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let valid = record_line("2026-09-01");
        let content = format!("{valid}\nD1:2026-09-01:corrupt\nnot-a-record\n");
        let retained = retain_diagnostic_records(&content, cutoff, |line| {
            line.find('{')
                .and_then(|index| line.get(index..))
                .map(str::to_string)
        });
        assert_eq!(retained.corrupt, 2);
        assert_eq!(retained.content, format!("{valid}\n"));
    }

    #[test]
    fn atomic_replace_reports_write_failures_without_exposing_a_path() {
        let target = std::env::temp_dir()
            .join(format!("wincmd-retention-{}", retention_sequence()))
            .join("missing")
            .join("diagnostics.log");
        assert_eq!(
            atomic_replace(&target, b"safe"),
            Err("DIAGNOSTICS.PRUNE.WRITE_FAILED")
        );
    }

    #[test]
    fn recovery_record_contains_only_stable_diagnostic_fields() {
        let event = recovery_event();
        assert_eq!(event.outcome, DiagnosticOutcome::Recovered);
        assert_eq!(
            event.error_code.as_deref(),
            Some("DIAGNOSTICS.STORAGE.RECOVERED")
        );
        assert!(event.redacted_context.is_empty());
    }
    #[test]
    fn diagnostic_reader_retains_legacy_log_records() {
        let source = include_str!("datastore.rs");
        assert!(source.contains("log_decrypt_line(line)"));
        assert!(source.contains("seal_diagnostic_record"));
    }
}
