//! Tauri adapter for the service-owned Vault Access policy.
//!
//! `serde_json::Value` is intentional until generated shared policy types are
//! available in this crate. This module preserves the frozen snake_case wire
//! and contains that temporary adaptation at one boundary.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use wincmd_shared::diagnostics::{
    DiagnosticEvent, DiagnosticLifecycle, DiagnosticOutcome, DiagnosticPrivacyClass,
    DiagnosticRetryability, DiagnosticSeverity,
};
use zeroize::Zeroize;

const GET_POLICY: &str = "svc.vault.get_policy";
const APPLY_POLICY: &str = "svc.vault.apply_policy";
const GET_STATUS: &str = "svc.vault.get_status";
const UNMOUNT: &str = "svc.vault.unmount";
const LIST_AUTHORIZED: &str = "svc.vault.list_authorized";
const CAPABILITIES: &str = "svc.vault.capabilities";
const RECONCILE_ACCESS_GROUPS: &str = "svc.vault.reconcile_access_groups";
const QUERY_SERVICE_DIAGNOSTICS: &str = "svc.diagnostics.query";

static NEXT_DIAGNOSTIC_OPERATION: AtomicU64 = AtomicU64::new(1);

fn next_operation_id(action: &str) -> String {
    let sequence = NEXT_DIAGNOSTIC_OPERATION.fetch_add(1, Ordering::Relaxed);
    format!("VLT-{action}-{sequence}")
}

fn requested_operation_id(candidate: Option<String>, action: &str) -> String {
    candidate
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
        .unwrap_or_else(|| next_operation_id(action))
}

fn record_vault_event(
    operation_id: &str,
    action: &str,
    stage: &str,
    lifecycle: DiagnosticLifecycle,
    outcome: DiagnosticOutcome,
    severity: DiagnosticSeverity,
    error_code: Option<&str>,
    retryability: DiagnosticRetryability,
    suggested_next_action: &str,
    started: Option<Instant>,
) {
    let event_id = format!(
        "evt-{operation_id}-{stage}-{}",
        NEXT_DIAGNOSTIC_OPERATION.fetch_add(1, Ordering::Relaxed)
    );
    let event = DiagnosticEvent {
        event_id,
        operation_id: operation_id.to_string(),
        parent_operation_id: None,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        component: "desktop".into(),
        feature: "vault".into(),
        action: action.into(),
        stage: stage.into(),
        lifecycle,
        outcome,
        error_code: error_code.map(str::to_string),
        severity,
        retryability,
        suggested_next_action: suggested_next_action.into(),
        duration_ms: started.map(|value| value.elapsed().as_millis().min(u64::MAX as u128) as u64),
        privacy_class: DiagnosticPrivacyClass::LocalSensitive,
        redacted_context: BTreeMap::new(),
    };
    let _ = crate::diagnostics::record(event);
}

fn vault_failure_code(
    action: &str,
    error: &str,
) -> (&'static str, DiagnosticRetryability, &'static str) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("did not confirm") {
        (
            "VLT.OPERATION.TIMEOUT",
            DiagnosticRetryability::Manual,
            "refresh_status",
        )
    } else if lower.contains("not_authorized") || lower.contains("forbidden") {
        (
            "VLT.AUTH.DENIED",
            DiagnosticRetryability::Never,
            "request_authorization",
        )
    } else if lower.contains("driver") {
        (
            "VLT.DRIVER.UNAVAILABLE",
            DiagnosticRetryability::Manual,
            "check_driver_health",
        )
    } else if lower.contains("dismount") || action == "dismount" {
        (
            "VLT.DISMOUNT.FAILED",
            DiagnosticRetryability::Automatic,
            "retry_cleanup",
        )
    } else if lower.contains("validation") {
        (
            "VLT.REQUEST.INVALID",
            DiagnosticRetryability::Never,
            "review_request",
        )
    } else {
        ("VLT.BROKER.FAILED", DiagnosticRetryability::Manual, "retry")
    }
}

fn vault_result_failed(result: &Value) -> bool {
    result
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| state == "failed" || state == "denied")
}

#[tauri::command]
pub async fn get_vault_access_policy() -> Result<Value, String> {
    crate::svc_client::call(GET_POLICY, json!({})).await
}

#[tauri::command]
pub async fn apply_vault_access_policy(policy: Value) -> Result<Value, String> {
    let operation_id = next_operation_id("apply_policy");
    let started = Instant::now();
    record_vault_event(
        &operation_id,
        "apply_policy",
        "requested",
        DiagnosticLifecycle::Requested,
        DiagnosticOutcome::Started,
        DiagnosticSeverity::Info,
        None,
        DiagnosticRetryability::Never,
        "none",
        None,
    );
    let result = crate::svc_client::call_with_diagnostic_operation(
        APPLY_POLICY,
        policy,
        Some(operation_id.clone()),
    )
    .await;
    match &result {
        Ok(_) => record_vault_event(
            &operation_id,
            "apply_policy",
            "applied",
            DiagnosticLifecycle::Applied,
            DiagnosticOutcome::Succeeded,
            DiagnosticSeverity::Info,
            None,
            DiagnosticRetryability::Never,
            "none",
            Some(started),
        ),
        Err(error) => {
            let (code, retryability, next) = vault_failure_code("apply_policy", error);
            record_vault_event(
                &operation_id,
                "apply_policy",
                "applied",
                DiagnosticLifecycle::Applied,
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some(code),
                retryability,
                next,
                Some(started),
            );
        }
    }
    result
}

#[tauri::command]
pub async fn get_vault_access_status() -> Result<Value, String> {
    crate::svc_client::call(GET_STATUS, json!({})).await
}

/// Tauri maps the UI's mount arguments to this snake_case Rust signature.
/// `volume_role` remains optional so installed standard-container clients keep
/// their two-field request contract. Secrets are sent once and cleared from
/// both owned representations before this command returns.
#[tauri::command]
pub async fn vault_mount_entry(
    entry_id: String,
    password: String,
    volume_role: Option<wincmd_shared::vault_access::VaultVolumeRole>,
    hidden_protection_password: Option<String>,
    diagnostic_operation_id: Option<String>,
) -> Result<Value, String> {
    let operation_id = requested_operation_id(diagnostic_operation_id, "mount");
    let started = Instant::now();
    record_vault_event(
        &operation_id,
        "mount",
        "requested",
        DiagnosticLifecycle::Requested,
        DiagnosticOutcome::Started,
        DiagnosticSeverity::Info,
        None,
        DiagnosticRetryability::Never,
        "none",
        None,
    );
    let mut request = wincmd_shared::vault_access::VaultMountRequest {
        entry_id,
        password,
        volume_role: volume_role.unwrap_or_default(),
        hidden_protection_password,
    };
    let payload = serde_json::to_value(&request);
    // The service-call payload now owns the only remaining copy. Clear the
    // command-local copy before the potentially long engine wait begins.
    request.password.zeroize();
    let payload = match payload {
        Ok(payload) => payload,
        Err(_) => {
            record_vault_event(
                &operation_id,
                "mount",
                "requested",
                DiagnosticLifecycle::Requested,
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some("VLT.REQUEST.INVALID"),
                DiagnosticRetryability::Never,
                "review_request",
                Some(started),
            );
            return Err("mount request could not be encoded".to_string());
        }
    };
    let result =
        crate::svc_client::call_vault_mount_with_operation(payload, Some(operation_id.clone()))
            .await;
    match &result {
        Ok(value) if vault_result_failed(value) => record_vault_event(
            &operation_id,
            "mount",
            "applied",
            DiagnosticLifecycle::Applied,
            DiagnosticOutcome::Failed,
            DiagnosticSeverity::Warn,
            Some("VLT.MOUNT.DENIED_OR_FAILED"),
            DiagnosticRetryability::Manual,
            "review_status",
            Some(started),
        ),
        Ok(_) => record_vault_event(
            &operation_id,
            "mount",
            "applied",
            DiagnosticLifecycle::Applied,
            DiagnosticOutcome::Succeeded,
            DiagnosticSeverity::Info,
            None,
            DiagnosticRetryability::Never,
            "none",
            Some(started),
        ),
        Err(error) => {
            let (code, retryability, next) = vault_failure_code("mount", error);
            record_vault_event(
                &operation_id,
                "mount",
                "applied",
                DiagnosticLifecycle::Applied,
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some(code),
                retryability,
                next,
                Some(started),
            );
        }
    }
    result
}

#[tauri::command]
pub async fn vault_unmount_entry(
    entry_id: String,
    diagnostic_operation_id: Option<String>,
) -> Result<Value, String> {
    let operation_id = requested_operation_id(diagnostic_operation_id, "dismount");
    let started = Instant::now();
    record_vault_event(
        &operation_id,
        "dismount",
        "requested",
        DiagnosticLifecycle::Requested,
        DiagnosticOutcome::Started,
        DiagnosticSeverity::Info,
        None,
        DiagnosticRetryability::Never,
        "none",
        None,
    );
    let result = crate::svc_client::call_with_diagnostic_operation(
        UNMOUNT,
        serde_json::to_value(wincmd_shared::vault_access::VaultUnmountRequest { entry_id })
            .map_err(|_| "unmount request could not be encoded".to_string())?,
        Some(operation_id.clone()),
    )
    .await;
    match &result {
        Ok(value) if vault_result_failed(value) => record_vault_event(
            &operation_id,
            "dismount",
            "applied",
            DiagnosticLifecycle::Applied,
            DiagnosticOutcome::Failed,
            DiagnosticSeverity::Error,
            Some("VLT.DISMOUNT.FAILED"),
            DiagnosticRetryability::Automatic,
            "retry_cleanup",
            Some(started),
        ),
        Ok(_) => record_vault_event(
            &operation_id,
            "dismount",
            "applied",
            DiagnosticLifecycle::Applied,
            DiagnosticOutcome::Succeeded,
            DiagnosticSeverity::Info,
            None,
            DiagnosticRetryability::Never,
            "none",
            Some(started),
        ),
        Err(error) => {
            let (code, retryability, next) = vault_failure_code("dismount", error);
            record_vault_event(
                &operation_id,
                "dismount",
                "applied",
                DiagnosticLifecycle::Applied,
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some(code),
                retryability,
                next,
                Some(started),
            );
        }
    }
    result
}

#[tauri::command]
pub async fn vault_list_authorized_entries() -> Result<Value, String> {
    crate::svc_client::call(LIST_AUTHORIZED, json!({})).await
}

#[tauri::command]
pub async fn get_vault_access_capabilities() -> Result<Value, String> {
    crate::svc_client::call(CAPABILITIES, json!({})).await
}

/// The SYSTEM service decrypts its own store and returns a bounded, safe
/// summary. The renderer never receives ciphertext, context, raw errors,
/// paths, identities, or service filesystem details.
#[tauri::command]
pub async fn get_service_diagnostic_summaries(
    operation_id: Option<String>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let mut args = json!({ "limit": limit.unwrap_or(50).min(100) });
    if let Some(operation_id) = operation_id {
        args["operation_id"] = Value::String(operation_id);
    }
    crate::svc_client::call(QUERY_SERVICE_DIAGNOSTICS, args).await
}

/// Wrap the renderer-supplied group list into the frozen
/// `svc.vault.reconcile_access_groups` request shape.
fn reconcile_access_groups_payload(groups: Value) -> Value {
    json!({ "groups": groups })
}

/// Reconcile Windows local groups (create/update membership) for the given
/// access-control groups. Privileged: the service rejects an unprivileged
/// caller. The bridge passes the `groups` array through untouched and
/// returns the service's per-group result verbatim; it does not add or
/// observe any container path/SID/ACL data of its own.
#[tauri::command]
pub async fn reconcile_vault_access_groups(groups: Value) -> Result<Value, String> {
    crate::svc_client::call(
        RECONCILE_ACCESS_GROUPS,
        reconcile_access_groups_payload(groups),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_verbs_stay_on_the_frozen_service_wire() {
        assert_eq!(GET_POLICY, "svc.vault.get_policy");
        assert_eq!(APPLY_POLICY, "svc.vault.apply_policy");
        assert_eq!(GET_STATUS, "svc.vault.get_status");
        assert_eq!(UNMOUNT, "svc.vault.unmount");
        assert_eq!(LIST_AUTHORIZED, "svc.vault.list_authorized");
        assert_eq!(CAPABILITIES, "svc.vault.capabilities");
        assert_eq!(RECONCILE_ACCESS_GROUPS, "svc.vault.reconcile_access_groups");
        assert_eq!(QUERY_SERVICE_DIAGNOSTICS, "svc.diagnostics.query");
    }

    #[test]
    fn mount_command_payload_has_no_service_owned_fields() {
        let value = serde_json::to_value(wincmd_shared::vault_access::VaultMountRequest {
            entry_id: "shared".into(),
            password: "canary".into(),
            volume_role: wincmd_shared::vault_access::VaultVolumeRole::Outer,
            hidden_protection_password: None,
        })
        .unwrap();
        assert_eq!(value.as_object().unwrap().len(), 3);
        assert!(value.get("container_path").is_none());
        assert!(value.get("sid").is_none());
        assert_eq!(
            value.get("volume_role"),
            Some(&serde_json::Value::String("outer".into()))
        );
    }

    #[test]
    fn reconcile_access_groups_payload_matches_the_frozen_wire_shape() {
        let groups = json!([
            { "local_group": "WC_Sales", "member_sids": ["S-1-5-21-1", "S-1-5-21-2"] }
        ]);
        let payload = reconcile_access_groups_payload(groups.clone());
        assert_eq!(payload, json!({ "groups": groups }));
        assert_eq!(payload.as_object().unwrap().len(), 1);
    }

    #[test]
    fn vault_transport_failures_map_to_safe_stable_codes() {
        assert_eq!(
            vault_failure_code(
                "mount",
                "service operation did not confirm before its deadline"
            )
            .0,
            "VLT.OPERATION.TIMEOUT"
        );
        assert_eq!(
            vault_failure_code("mount", "service rejected request: vault_not_authorized").0,
            "VLT.AUTH.DENIED"
        );
        assert_eq!(
            vault_failure_code("dismount", "unrecognized service reply").0,
            "VLT.DISMOUNT.FAILED"
        );
    }

    #[test]
    fn terminal_mount_state_is_not_treated_as_success() {
        assert!(vault_result_failed(&json!({ "state": "failed" })));
        assert!(vault_result_failed(&json!({ "state": "denied" })));
        assert!(!vault_result_failed(&json!({ "state": "mounted" })));
    }
}
