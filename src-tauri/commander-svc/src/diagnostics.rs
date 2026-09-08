//! Service-owned encrypted terminal diagnostics. No plaintext fallback exists.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rand::{rngs::OsRng, RngCore};
use wincmd_shared::diagnostics::{
    open_diagnostic_record, seal_diagnostic_record, DiagnosticEvent, DiagnosticLifecycle,
    DiagnosticOutcome, DiagnosticPrivacyClass, DiagnosticRetryability, DiagnosticSeverity,
};
use wincmd_shared::vault_access::{
    VaultMountReason, VaultMountResult, VaultMountState, VaultPresentation,
};

const EVENT_FILE: &str = "service-diagnostic-events.log";
const MATERIAL_FILE: &str = "service-diagnostics.material";
const MAX_EVENT_BYTES: usize = 16 * 1024;
const RETENTION_DAYS: i64 = 7;
const MAX_EMERGENCY_FAILURES: usize = 32;
static PERSISTED_EVENTS: AtomicU64 = AtomicU64::new(0);
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
static STORAGE_FAILURES: AtomicU64 = AtomicU64::new(0);
static RETENTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct StorageHealthState {
    pruned_events: u64,
    corrupt_events: u64,
    recovery_events: u64,
    last_failure_code: Option<&'static str>,
    pending_recovery: bool,
    emergency_failure_codes: VecDeque<&'static str>,
}

fn storage_health() -> &'static Mutex<StorageHealthState> {
    static STATE: OnceLock<Mutex<StorageHealthState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(StorageHealthState::default()))
}

fn storage_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticsHealth {
    pub(crate) persisted_events: u64,
    pub(crate) dropped_persistence_events: u64,
    pub(crate) storage_failures: u64,
    pub(crate) pruned_events: u64,
    pub(crate) corrupt_events: u64,
    pub(crate) recovery_events: u64,
    pub(crate) emergency_failure_count: usize,
    pub(crate) last_failure_code: Option<&'static str>,
    pub(crate) encrypted_persistence_available: bool,
}

pub(crate) fn health() -> DiagnosticsHealth {
    let state = storage_health().lock().ok();
    DiagnosticsHealth {
        persisted_events: PERSISTED_EVENTS.load(Ordering::Relaxed),
        dropped_persistence_events: DROPPED_EVENTS.load(Ordering::Relaxed),
        storage_failures: STORAGE_FAILURES.load(Ordering::Relaxed),
        pruned_events: state.as_ref().map_or(0, |value| value.pruned_events),
        corrupt_events: state.as_ref().map_or(0, |value| value.corrupt_events),
        recovery_events: state.as_ref().map_or(0, |value| value.recovery_events),
        emergency_failure_count: state
            .as_ref()
            .map_or(0, |value| value.emergency_failure_codes.len()),
        last_failure_code: state.and_then(|value| value.last_failure_code),
        encrypted_persistence_available: cfg!(windows),
    }
}

pub(crate) fn prune_retained_diagnostics() {
    let _lock = match storage_lock().lock() {
        Ok(lock) => lock,
        Err(_) => {
            record_storage_failure("DIAGNOSTICS.PRUNE.LOCK_FAILED");
            return;
        }
    };
    let result = diagnostics_dir()
        .map_err(|_| "DIAGNOSTICS.PRUNE.PATH_FAILED")
        .and_then(|dir| prune_diagnostics_at(&dir));
    match result {
        Ok(retention) => record_prune_result(&retention),
        Err(code) => record_storage_failure(code),
    }
}
#[derive(Default)]
struct RetentionResult {
    pruned: u64,
    corrupt: u64,
    content: String,
}
/// Safe projection for the authenticated service pipe. Context, ciphertext,
/// storage errors, and service filesystem details never cross this boundary.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticSummary {
    pub(crate) event_id: String,
    pub(crate) operation_id: String,
    pub(crate) occurred_at: String,
    pub(crate) feature: String,
    pub(crate) action: String,
    pub(crate) stage: String,
    pub(crate) outcome: DiagnosticOutcome,
    pub(crate) error_code: Option<String>,
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) retryability: DiagnosticRetryability,
    pub(crate) suggested_next_action: String,
    pub(crate) duration_ms: Option<u64>,
}

/// Reads only the safe summary projection. Pipe authorization belongs to the
/// caller; this function is deliberately incapable of returning context.
pub(crate) fn recent_summaries(
    operation_id: Option<&str>,
    limit: usize,
) -> Result<Vec<DiagnosticSummary>, String> {
    let _lock = storage_lock()
        .lock()
        .map_err(|_| "DIAGNOSTICS.READ.LOCK_FAILED".to_string())?;
    let dir = diagnostics_dir().map_err(|_| "DIAGNOSTICS.READ.PATH_FAILED".to_string())?;
    match prune_diagnostics_at(&dir) {
        Ok(retention) => record_prune_result(&retention),
        Err(code) => {
            record_storage_failure(code);
            return Err(code.to_string());
        }
    }
    recent_summaries_at(&dir, operation_id, limit).map_err(|_| {
        record_storage_failure("DIAGNOSTICS.READ.FAILED");
        "DIAGNOSTICS.READ.FAILED".to_string()
    })
}
fn recent_summaries_at(
    dir: &Path,
    operation_id: Option<&str>,
    limit: usize,
) -> Result<Vec<DiagnosticSummary>, String> {
    let content = match fs::read_to_string(dir.join(EVENT_FILE)) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("read encrypted diagnostic store failed".to_string()),
    };
    let key = service_key(dir)?;
    let mut summaries = Vec::new();
    for line in content.lines().rev() {
        let Some(plaintext) = open_diagnostic_record(&key, "service", line) else {
            continue;
        };
        let Ok(event) = serde_json::from_slice::<DiagnosticEvent>(&plaintext) else {
            continue;
        };
        if operation_id.is_none_or(|id| id == event.operation_id) {
            summaries.push(DiagnosticSummary {
                event_id: event.event_id,
                operation_id: event.operation_id,
                occurred_at: event.occurred_at,
                feature: event.feature,
                action: event.action,
                stage: event.stage,
                outcome: event.outcome,
                error_code: event.error_code,
                severity: event.severity,
                retryability: event.retryability,
                suggested_next_action: event.suggested_next_action,
                duration_ms: event.duration_ms,
            });
        }
        if summaries.len() == limit.min(500) {
            break;
        }
    }
    Ok(summaries)
}

/// Single service diagnostic ingress. It persists an event before any future
/// notification/Fleet projection; a failed store never becomes plaintext.
pub(crate) fn record_event(event: DiagnosticEvent) -> Result<(), String> {
    event.validate()?;
    let payload =
        serde_json::to_vec(&event).map_err(|_| "DIAGNOSTICS.ENCODE_FAILED".to_string())?;
    if payload.len() > MAX_EVENT_BYTES {
        record_storage_failure("DIAGNOSTICS.EVENT.TOO_LARGE");
        return Err("DIAGNOSTICS.EVENT.TOO_LARGE".to_string());
    }
    let _lock = storage_lock()
        .lock()
        .map_err(|_| "DIAGNOSTICS.STORAGE.LOCK_FAILED".to_string())?;
    let result = (|| {
        let dir = diagnostics_dir().map_err(|_| "DIAGNOSTICS.STORAGE.PATH_FAILED")?;
        let retention = prune_diagnostics_at(&dir)?;
        persist_event_at(&dir, &event, &payload).map_err(|_| "DIAGNOSTICS.STORAGE.WRITE_FAILED")?;
        Ok::<_, &'static str>((dir, retention))
    })();
    match result {
        Ok((dir, retention)) => {
            PERSISTED_EVENTS.fetch_add(1, Ordering::Relaxed);
            record_prune_result(&retention);
            record_recovery_if_needed(&dir);
            Ok(())
        }
        Err(code) => {
            record_storage_failure(code);
            Err(code.to_string())
        }
    }
}
fn retention_cutoff() -> String {
    // Retain today and the preceding six UTC calendar dates: exactly seven dates.
    utc_date_from_days((unix_ms() / 86_400_000) as i64 - (RETENTION_DAYS - 1))
}

fn parse_envelope_date(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("D1:")?;
    let date = rest.get(..10)?;
    if rest.as_bytes().get(10) != Some(&b':') || !is_valid_date(date) {
        return None;
    }
    Some(date)
}

fn is_valid_date(value: &str) -> bool {
    if value.len() != 10
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
    {
        return false;
    }
    let year = value.get(..4).and_then(|part| part.parse::<i32>().ok());
    let month = value.get(5..7).and_then(|part| part.parse::<u32>().ok());
    let day = value.get(8..10).and_then(|part| part.parse::<u32>().ok());
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    day >= 1 && day <= days
}

fn retain_diagnostic_records<F>(content: &str, cutoff: String, mut decrypt: F) -> RetentionResult
where
    F: FnMut(&str) -> Option<Vec<u8>>,
{
    let mut result = RetentionResult::default();
    for line in content.lines() {
        let Some(date) = parse_envelope_date(line) else {
            result.corrupt += 1;
            continue;
        };
        if date < cutoff.as_str() {
            result.pruned += 1;
            continue;
        }
        let Some(body) = decrypt(line) else {
            result.corrupt += 1;
            continue;
        };
        let Ok(event) = serde_json::from_slice::<DiagnosticEvent>(&body) else {
            result.corrupt += 1;
            continue;
        };
        if event.occurred_at.get(..10) != Some(&line[3..13]) {
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
    let sequence = RETENTION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{file_name}.{sequence}.tmp"));
    let write_result = (|| {
        let mut file =
            fs::File::create(&temporary).map_err(|_| "DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
        file.write_all(content)
            .map_err(|_| "DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
        file.sync_all()
            .map_err(|_| "DIAGNOSTICS.PRUNE.WRITE_FAILED")?;
        fs::rename(&temporary, path).map_err(|_| "DIAGNOSTICS.PRUNE.REPLACE_FAILED")
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn record_prune_result(result: &RetentionResult) {
    if let Ok(mut state) = storage_health().lock() {
        state.pruned_events += result.pruned;
        state.corrupt_events += result.corrupt;
    }
}

fn record_storage_failure(code: &'static str) {
    DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
    STORAGE_FAILURES.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut state) = storage_health().lock() {
        state.last_failure_code = Some(code);
        state.pending_recovery = true;
        if state.emergency_failure_codes.len() == MAX_EMERGENCY_FAILURES {
            state.emergency_failure_codes.pop_front();
        }
        state.emergency_failure_codes.push_back(code);
    }
}

fn prune_diagnostics_at(dir: &Path) -> Result<RetentionResult, &'static str> {
    let path = dir.join(EVENT_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RetentionResult::default())
        }
        Err(_) => return Err("DIAGNOSTICS.PRUNE.READ_FAILED"),
    };
    let key = service_key(dir).map_err(|_| "DIAGNOSTICS.PRUNE.KEY_UNAVAILABLE")?;
    let result = retain_diagnostic_records(&content, retention_cutoff(), |line| {
        open_diagnostic_record(&key, "service", line)
    });
    if result.content != content {
        atomic_replace(&path, result.content.as_bytes())?;
    }
    Ok(result)
}

fn recovery_event() -> DiagnosticEvent {
    let sequence = RETENTION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    DiagnosticEvent {
        event_id: format!("svc-diag-recovery-{sequence}"),
        operation_id: format!("DIA-SVC-RECOVERY-{sequence}"),
        parent_operation_id: None,
        occurred_at: format!("{}T00:00:00Z", today_utc()),
        component: "service".into(),
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

fn record_recovery_if_needed(dir: &Path) {
    let should_record = storage_health()
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
    let event = recovery_event();
    let payload = match serde_json::to_vec(&event) {
        Ok(payload) => payload,
        Err(_) => {
            record_storage_failure("DIAGNOSTICS.ENCODE_FAILED");
            return;
        }
    };
    match persist_event_at(dir, &event, &payload) {
        Ok(()) => {
            PERSISTED_EVENTS.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut state) = storage_health().lock() {
                state.recovery_events += 1;
            }
        }
        Err(_) => record_storage_failure("DIAGNOSTICS.STORAGE.RECOVERY_WRITE_FAILED"),
    }
}
fn persist_event_at(dir: &Path, event: &DiagnosticEvent, payload: &[u8]) -> Result<(), String> {
    let key = service_key(dir)?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let line = seal_diagnostic_record(&key, &event.occurred_at[..10], "service", &payload, nonce)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(EVENT_FILE))
        .map_err(|_| "open encrypted diagnostic store failed".to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|_| "write encrypted diagnostic store failed".to_string())?;
    file.write_all(b"\n")
        .map_err(|_| "finish encrypted diagnostic store failed".to_string())?;
    file.sync_data()
        .map_err(|_| "sync encrypted diagnostic store failed".to_string())
}

pub(crate) fn record_vault_terminal(
    operation_id: &str,
    action: &'static str,
    result: &VaultMountResult,
    started: Instant,
) {
    let (outcome, error_code, severity, retryability, next) = outcome_fields(result, action);
    let mut context = BTreeMap::new();
    if let Some(presentation) = result.presentation {
        context.insert("state".into(), presentation_name(presentation).into());
    }
    let _ = record_event(vault_event(
        operation_id,
        action,
        outcome,
        error_code,
        severity,
        retryability,
        next,
        started,
        context,
    ));
}

pub(crate) fn record_vault_failure(
    operation_id: &str,
    action: &'static str,
    error_code: &'static str,
    next: &'static str,
    retryable: bool,
    started: Instant,
) {
    let _ = record_event(vault_event(
        operation_id,
        action,
        DiagnosticOutcome::Failed,
        error_code,
        DiagnosticSeverity::Error,
        retryable,
        next,
        started,
        BTreeMap::new(),
    ));
}

fn vault_event(
    operation_id: &str,
    action: &'static str,
    outcome: DiagnosticOutcome,
    error_code: &'static str,
    severity: DiagnosticSeverity,
    retryable: bool,
    next: &'static str,
    started: Instant,
    context: BTreeMap<String, String>,
) -> DiagnosticEvent {
    DiagnosticEvent {
        event_id: format!("svc-vlt-{operation_id}-{}", unix_ms()),
        operation_id: operation_id.to_string(),
        parent_operation_id: None,
        occurred_at: format!("{}T00:00:00Z", today_utc()),
        component: "service".into(),
        feature: "vault".into(),
        action: action.into(),
        stage: "terminal".into(),
        lifecycle: DiagnosticLifecycle::Applied,
        outcome,
        error_code: Some(error_code.into()),
        severity,
        retryability: if retryable {
            DiagnosticRetryability::Automatic
        } else {
            DiagnosticRetryability::Never
        },
        suggested_next_action: next.into(),
        duration_ms: Some(started.elapsed().as_millis() as u64),
        privacy_class: DiagnosticPrivacyClass::Restricted,
        redacted_context: context,
    }
}

fn diagnostics_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("WinCommander").join("diagnostics");
    fs::create_dir_all(&dir).map_err(|_| "create diagnostic directory failed".to_string())?;
    #[cfg(windows)]
    {
        use crate::policy_store::PolicyFs;
        crate::policy_store::WindowsPolicyFs
            .ensure_dir_secure(&dir)
            .map_err(|_| "secure diagnostic directory unavailable".to_string())?;
    }
    Ok(dir)
}

fn service_key(dir: &Path) -> Result<[u8; 32], String> {
    let path = dir.join(MATERIAL_FILE);
    if path.exists() {
        let raw = fs::read(path).map_err(|_| "read diagnostic key material failed".to_string())?;
        let plain = machine_unprotect(&raw)?;
        return plain
            .try_into()
            .map_err(|_| "diagnostic key material is invalid".to_string());
    }
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let protected = machine_protect(&key)?;
    fs::write(path, protected).map_err(|_| "write diagnostic key material failed".to_string())?;
    Ok(key)
}

#[cfg(windows)]
fn machine_protect(input: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(input, true)
}
#[cfg(windows)]
fn machine_unprotect(input: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(input, false)
}
#[cfg(windows)]
fn dpapi(input: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
    };
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = if protect {
            CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut out,
            )
        };
        if ok == 0 {
            return Err("machine key protection unavailable".to_string());
        }
        let value = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Ok(value)
    }
}
#[cfg(not(windows))]
fn machine_protect(input: &[u8]) -> Result<Vec<u8>, String> {
    Ok(input.to_vec())
}
#[cfg(not(windows))]
fn machine_unprotect(input: &[u8]) -> Result<Vec<u8>, String> {
    Ok(input.to_vec())
}

fn outcome_fields(
    result: &VaultMountResult,
    action: &str,
) -> (
    DiagnosticOutcome,
    &'static str,
    DiagnosticSeverity,
    bool,
    &'static str,
) {
    match result.state {
        VaultMountState::Mounted | VaultMountState::Unmounted => (
            DiagnosticOutcome::Succeeded,
            if action == "mount" {
                "VLT.MOUNT.COMPLETED"
            } else {
                "VLT.DISMOUNT.COMPLETED"
            },
            DiagnosticSeverity::Info,
            false,
            "none",
        ),
        VaultMountState::Denied => (
            DiagnosticOutcome::Failed,
            "VLT.AUTH.DENIED",
            DiagnosticSeverity::Warn,
            false,
            "request_authorization",
        ),
        VaultMountState::Failed => reason_fields(result.reason, action),
    }
}
fn reason_fields(
    reason: Option<VaultMountReason>,
    action: &str,
) -> (
    DiagnosticOutcome,
    &'static str,
    DiagnosticSeverity,
    bool,
    &'static str,
) {
    let (code, severity, retry, next) = match reason {
        Some(VaultMountReason::NotAuthorized) => (
            "VLT.AUTH.DENIED",
            DiagnosticSeverity::Warn,
            false,
            "request_authorization",
        ),
        Some(VaultMountReason::InvalidRequest) => (
            "VLT.REQUEST.INVALID",
            DiagnosticSeverity::Error,
            false,
            "review_request",
        ),
        Some(VaultMountReason::BrokerUnavailable) => (
            "VLT.BROKER.UNAVAILABLE",
            DiagnosticSeverity::Error,
            true,
            "retry",
        ),
        Some(VaultMountReason::BrokerRejected) => (
            "VLT.BROKER.REJECTED",
            DiagnosticSeverity::Error,
            false,
            "check_service_health",
        ),
        Some(VaultMountReason::SessionUnavailable) => (
            "VLT.SESSION.UNAVAILABLE",
            DiagnosticSeverity::Warn,
            true,
            "sign_in_and_retry",
        ),
        Some(VaultMountReason::EngineUnlockFailed) => (
            "VLT.UNLOCK.FAILED",
            DiagnosticSeverity::Error,
            true,
            "check_credentials",
        ),
        Some(VaultMountReason::EngineDriveLetterUnavailable) => (
            "VLT.DRIVE_LETTER.UNAVAILABLE",
            DiagnosticSeverity::Warn,
            true,
            "select_another_drive_letter",
        ),
        Some(VaultMountReason::EngineMountFailed) => (
            "VLT.MOUNT.ENGINE_FAILED",
            DiagnosticSeverity::Error,
            true,
            "check_driver_health",
        ),
        Some(VaultMountReason::AclApplyFailed) => (
            "VLT.ACL.APPLY_FAILED",
            DiagnosticSeverity::Error,
            false,
            "check_vault_permissions",
        ),
        Some(VaultMountReason::AclReadbackFailed) => (
            "VLT.ACL.READBACK_FAILED",
            DiagnosticSeverity::Error,
            false,
            "check_vault_permissions",
        ),
        Some(VaultMountReason::DismountFailed) if action == "dismount" => (
            "VLT.DISMOUNT.FAILED",
            DiagnosticSeverity::Error,
            true,
            "retry_cleanup",
        ),
        Some(VaultMountReason::DismountFailed) => (
            "VLT.CLEANUP.FAILED",
            DiagnosticSeverity::Error,
            true,
            "retry_cleanup",
        ),
        None => ("VLT.UNKNOWN", DiagnosticSeverity::Error, true, "retry"),
    };
    (DiagnosticOutcome::Failed, code, severity, retry, next)
}
fn presentation_name(value: VaultPresentation) -> &'static str {
    match value {
        VaultPresentation::Machine => "machine",
        VaultPresentation::PerUser => "per_user",
    }
}
fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or_default()
}
fn today_utc() -> String {
    utc_date_from_days((unix_ms() / 86_400_000) as i64)
}

fn utc_date_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    format!(
        "{:04}-{:02}-{:02}",
        y + (mp < 10) as i64,
        (mp + if mp < 10 { 3 } else { -9 }) as i64,
        doy - (153 * mp + 2) / 5 + 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_vault_reason_is_stable() {
        for reason in VaultMountReason::ALL {
            assert!(reason_fields(Some(reason), "mount").1.starts_with("VLT."));
        }
    }
    #[test]
    fn envelope_does_not_leak_vault_identity() {
        let event = vault_event(
            "VLT-4",
            "mount",
            DiagnosticOutcome::Failed,
            "VLT.MOUNT.ENGINE_FAILED",
            DiagnosticSeverity::Error,
            true,
            "retry",
            Instant::now(),
            BTreeMap::new(),
        );
        let raw = serde_json::to_string(&event).unwrap();
        assert!(!raw.contains("entry_id"));
        assert!(!raw.contains("drive_letter"));
    }
    #[test]
    fn service_terminal_record_preserves_the_client_operation_id() {
        let event = vault_event(
            "VLT-mount-client-1",
            "mount",
            DiagnosticOutcome::Succeeded,
            "VLT.MOUNT.COMPLETED",
            DiagnosticSeverity::Info,
            false,
            "none",
            Instant::now(),
            BTreeMap::new(),
        );
        assert_eq!(event.operation_id, "VLT-mount-client-1");
    }
    #[test]
    fn time_has_a_valid_date_prefix() {
        assert_eq!(today_utc().len(), 10);
    }
    fn retention_event(date: &str) -> DiagnosticEvent {
        let mut event = vault_event(
            "VLT-retention-1",
            "mount",
            DiagnosticOutcome::Succeeded,
            "VLT.MOUNT.COMPLETED",
            DiagnosticSeverity::Info,
            false,
            "none",
            Instant::now(),
            BTreeMap::new(),
        );
        event.occurred_at = format!("{date}T00:00:00Z");
        event
    }

    fn retention_line(date: &str) -> String {
        let body = serde_json::to_string(&retention_event(date)).unwrap();
        format!("D1:{date}:test-{date}:{body}")
    }

    #[test]
    fn retention_keeps_the_seventh_calendar_day_and_removes_older_records() {
        let cutoff = "2026-09-01".to_string();
        let oldest_kept = retention_line("2026-09-01");
        let expired = retention_line("2026-08-31");
        let content = format!("{oldest_kept}\n{expired}\n");
        let retained = retain_diagnostic_records(&content, cutoff, |line| {
            line.find('{')
                .and_then(|index| line.get(index..))
                .map(|body| body.as_bytes().to_vec())
        });
        assert_eq!(retained.pruned, 1);
        assert_eq!(retained.corrupt, 0);
        assert_eq!(retained.content, format!("{oldest_kept}\n"));
    }

    #[test]
    fn retention_removes_malformed_and_corrupt_current_records() {
        let cutoff = "2026-09-01".to_string();
        let valid = retention_line("2026-09-01");
        let content = format!("{valid}\nD1:2026-09-01:corrupt\nnot-a-record\n");
        let retained = retain_diagnostic_records(&content, cutoff, |line| {
            line.find('{')
                .and_then(|index| line.get(index..))
                .map(|body| body.as_bytes().to_vec())
        });
        assert_eq!(retained.corrupt, 2);
        assert_eq!(retained.content, format!("{valid}\n"));
    }

    #[test]
    fn atomic_replace_reports_write_failures_without_exposing_a_path() {
        let target = std::env::temp_dir()
            .join(format!("wincmd-service-retention-{}", unix_ms()))
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
    fn safe_summary_never_returns_context_or_ciphertext() {
        let dir = std::env::temp_dir().join(format!("wincmd-diagnostics-{}", unix_ms()));
        fs::create_dir_all(&dir).unwrap();
        let mut context = BTreeMap::new();
        context.insert("state".into(), "machine".into());
        let event = vault_event(
            "VLT-5",
            "mount",
            DiagnosticOutcome::Failed,
            "VLT.MOUNT.ENGINE_FAILED",
            DiagnosticSeverity::Error,
            true,
            "retry",
            Instant::now(),
            context,
        );
        let encoded = serde_json::to_vec(&event).unwrap();
        persist_event_at(&dir, &event, &encoded).unwrap();
        let summaries = recent_summaries_at(&dir, Some("VLT-5"), 20).unwrap();
        let json = serde_json::to_string(&summaries).unwrap();
        assert_eq!(summaries.len(), 1);
        assert!(!json.contains("context"));
        assert!(!json.contains("machine"));
        assert!(!json.contains("D1:"));
        let _ = fs::remove_dir_all(dir);
    }
}
