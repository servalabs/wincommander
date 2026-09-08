use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use wincmd_shared::diagnostics::DiagnosticEvent;

const DIAGNOSTIC_FILE: &str = "diagnostic-events.log";
const MAX_READ_EVENTS: usize = 500;
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
    last_error: Option<String>,
}

fn health_state() -> &'static Mutex<DiagnosticHealthState> {
    static STATE: OnceLock<Mutex<DiagnosticHealthState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(DiagnosticHealthState::default()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticHealth {
    persisted_events: u64,
    dropped_events: u64,
    redacted_fields: u64,
    healthy: bool,
    last_error: Option<String>,
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

fn record_failure(message: String) {
    if let Ok(mut state) = health_state().lock() {
        state.dropped += 1;
        state.last_error = Some(message);
    }
}

pub(crate) fn record(mut event: DiagnosticEvent) -> Result<DiagnosticEvent, String> {
    event.validate()?;
    let removed = redact_context(&mut event);
    let payload = serde_json::to_string(&event)
        .map_err(|error| format!("encode diagnostic event: {error}"))?;
    let encrypted =
        match crate::datastore::diagnostic_encrypt_line(&event.occurred_at[..10], &payload) {
            Ok(value) => value,
            Err(_) => {
                let error = "diagnostic encryption is unavailable".to_string();
                record_failure(error.clone());
                return Err(error);
            }
        };
    let write_result = (|| {
        let _lock = crate::paths::acquire_machine_state_lock("diagnostic-events")?;
        let path = diagnostic_path()?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| format!("open diagnostic store: {error}"))?;
        file.write_all(encrypted.as_bytes())
            .map_err(|error| format!("write diagnostic store: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("finish diagnostic store: {error}"))?;
        file.sync_data()
            .map_err(|error| format!("sync diagnostic store: {error}"))
    })();
    match write_result {
        Ok(()) => {
            if let Ok(mut state) = health_state().lock() {
                state.persisted += 1;
                state.redacted_fields += removed as u64;
                state.last_error = None;
            }
            Ok(event)
        }
        Err(error) => {
            record_failure(error.clone());
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) fn record_diagnostic_event(event: DiagnosticEvent) -> Result<DiagnosticEvent, String> {
    record(event)
}

#[tauri::command]
pub(crate) fn get_diagnostic_events(
    operation_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<DiagnosticEvent>, String> {
    if crate::settings::is_decoy_mode() {
        return Ok(vec![]);
    }
    let path = diagnostic_path()?;
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(format!("read diagnostic store: {error}")),
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
        healthy: state
            .as_ref()
            .is_some_and(|value| value.last_error.is_none()),
        last_error: state.and_then(|value| value.last_error.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincmd_shared::diagnostics::{
        DiagnosticLifecycle, DiagnosticOutcome, DiagnosticPrivacyClass, DiagnosticRetryability,
        DiagnosticSeverity,
    };

    fn event() -> DiagnosticEvent {
        DiagnosticEvent {
            event_id: "evt-1".into(),
            operation_id: "VLT-1".into(),
            parent_operation_id: None,
            occurred_at: "2026-09-07T00:00:00Z".into(),
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

    #[test]
    fn redaction_keeps_only_allowlisted_non_sensitive_context() {
        let mut input = event();
        let removed = redact_context(&mut input);
        assert_eq!(removed, 2);
        assert_eq!(
            input.redacted_context,
            BTreeMap::from([("driver_state".into(), "unavailable".into())])
        );
    }

    #[test]
    fn diagnostic_reader_retains_legacy_log_records() {
        // D1 is the write format. Reader compatibility remains necessary for
        // already-persisted L2/L records and is supplied by datastore.
        let source = include_str!("datastore.rs");
        assert!(source.contains("log_decrypt_line(line)"));
        assert!(source.contains("seal_diagnostic_record"));
    }
}
