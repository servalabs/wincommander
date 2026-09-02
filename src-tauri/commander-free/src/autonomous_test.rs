// SPDX-License-Identifier: AGPL-3.0-or-later
//! Closed autonomous-test adapter for the isolated Fleet lab.
//!
//! This is intentionally separate from the generated GUI CLI catalog: callers
//! select a fixed scenario, never a handler, shell command, path, or URL.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const RESULT_SCHEMA: &str = "wincommander-autonomous-test-result/v1";
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const TEST_ISSUER_KEY_ID: &str = "autonomy-lab-1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum Scenario {
    #[serde(rename = "fleet.preflight")]
    FleetPreflight,
    #[serde(rename = "fleet.checkin.readback")]
    FleetCheckinReadback,
    #[serde(rename = "clipboard_guard.synthetic_marker")]
    ClipboardGuardSyntheticMarker,
    #[serde(rename = "privacy_shield.status")]
    PrivacyShieldStatus,
    #[serde(rename = "privacy_shield.start_stop")]
    PrivacyShieldStartStop,
}

impl Scenario {
    fn id(self) -> &'static str {
        match self {
            Self::FleetPreflight => "fleet.preflight",
            Self::FleetCheckinReadback => "fleet.checkin.readback",
            Self::ClipboardGuardSyntheticMarker => "clipboard_guard.synthetic_marker",
            Self::PrivacyShieldStatus => "privacy_shield.status",
            Self::PrivacyShieldStartStop => "privacy_shield.start_stop",
        }
    }

    fn command_binding(self) -> (&'static str, fleet_proto::ActionClass) {
        match self {
            Self::FleetPreflight | Self::FleetCheckinReadback => {
                ("mesh.status", fleet_proto::ActionClass::Safe)
            }
            Self::ClipboardGuardSyntheticMarker => {
                ("ink_receipt.status", fleet_proto::ActionClass::Safe)
            }
            Self::PrivacyShieldStatus => {
                ("endpoint.security_snapshot", fleet_proto::ActionClass::Safe)
            }
            Self::PrivacyShieldStartStop => ("policy.reapply", fleet_proto::ActionClass::Safe),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    run_id: String,
    scenario: Scenario,
    fixture_id: String,
    deadline_ms: u64,
    capability: fleet_proto::AutonomousTestCapability,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultEnvelope {
    schema: &'static str,
    run_id: String,
    scenario: Scenario,
    phase: &'static str,
    outcome: &'static str,
    observed_at: String,
    facts: Value,
    failure_code: Option<&'static str>,
    redacted_evidence_hash: String,
}

pub fn is_invocation(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("agent-test"))
}

pub fn main(args: Vec<String>) -> i32 {
    let request = match parse_request(&args) {
        Ok(request) => request,
        Err(error) => return fail("invalid_request", error),
    };
    if let Err(error) = validate_capability(&request) {
        return fail("capability_rejected", error);
    }
    run_in_tauri(request)
}

fn parse_request(args: &[String]) -> Result<Request, String> {
    let [verb, action, flag, raw] = args else {
        return Err("usage: agent-test run --request <json>".into());
    };
    if verb != "agent-test" || action != "run" || flag != "--request" {
        return Err("usage: agent-test run --request <json>".into());
    }
    if raw.len() > MAX_REQUEST_BYTES || raw.starts_with('@') || raw == "-" {
        return Err("request must be bounded inline JSON, not a file or stdin reference".into());
    }
    let request: Request =
        serde_json::from_str(raw).map_err(|_| "request must match the fixed schema".to_string())?;
    if uuid::Uuid::parse_str(&request.run_id).is_err() {
        return Err("runId must be a UUID".into());
    }
    if !valid_fixture_id(&request.fixture_id) {
        return Err(
            "fixtureId must use the wc-test- namespace and safe lowercase characters".into(),
        );
    }
    if !(1_000..=300_000).contains(&request.deadline_ms) {
        return Err("deadlineMs must be between 1000 and 300000".into());
    }
    Ok(request)
}

fn valid_fixture_id(value: &str) -> bool {
    value.len() <= 96
        && value.starts_with("wc-test-")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_capability(request: &Request) -> Result<(), String> {
    let settings = crate::settings::read_settings()?;
    let capability = &request.capability;
    if capability.run_id != request.run_id {
        return Err("capability run binding is invalid".into());
    }
    if capability.device_id.0 != settings.device_id
        || !capability
            .scenarios
            .iter()
            .any(|id| id == request.scenario.id())
    {
        return Err("capability is not bound to this device and scenario".into());
    }
    let (expected_catalog_id, expected_action_class) = request.scenario.command_binding();
    if uuid::Uuid::parse_str(&capability.command_id).is_err()
        || capability.catalog_id != expected_catalog_id
        || capability.action_class != expected_action_class
    {
        return Err("capability command binding is invalid".into());
    }
    let not_before = DateTime::parse_from_rfc3339(&capability.not_before)
        .map_err(|_| "capability notBefore is invalid")?
        .with_timezone(&Utc);
    let expires_at = DateTime::parse_from_rfc3339(&capability.expires_at)
        .map_err(|_| "capability expiresAt is invalid")?
        .with_timezone(&Utc);
    let now = Utc::now();
    if now < not_before || now >= expires_at {
        return Err("capability is not currently valid".into());
    }
    let key = settings
        .policy
        .fleet_signing_key
        .ok_or_else(|| "device has no pinned Fleet signing key".to_string())?;
    if !fleet_proto::verify_autonomous_test_capability(capability, TEST_ISSUER_KEY_ID, &key) {
        return Err("capability signature is invalid".into());
    }
    Ok(())
}

fn run_in_tauri(request: Request) -> i32 {
    let result = Arc::new(Mutex::new(None));
    let result_for_setup = result.clone();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().build.dev_url = None;
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let result = result_for_setup.clone();
            tauri::async_runtime::spawn(async move {
                let envelope = execute(app_handle.clone(), &request).await;
                let outcome = submit_result(&request.capability, &envelope)
                    .await
                    .map(|_| envelope);
                if let Ok(mut slot) = result.lock() {
                    *slot = Some(outcome);
                }
                app_handle.exit(0);
            });
            Ok(())
        });
    if let Err(error) = builder.run(context) {
        return fail("runtime_error", format!("test runtime failed: {error}"));
    }
    match result.lock().ok().and_then(|mut slot| slot.take()) {
        Some(Ok(envelope)) => {
            let exit_code = if envelope.outcome == "passed" { 0 } else { 8 };
            print_json(&envelope);
            exit_code
        }
        Some(Err((code, message))) => fail(code, message),
        None => fail(
            "runtime_error",
            "test runtime exited without a result".into(),
        ),
    }
}

async fn execute(
    app: tauri::AppHandle,
    request: &Request,
) -> ResultEnvelope {
    let facts = match request.scenario {
        Scenario::FleetPreflight => fleet_preflight(),
        Scenario::FleetCheckinReadback => fleet_checkin_readback().await,
        Scenario::ClipboardGuardSyntheticMarker => clipboard_marker(app, request).await,
        Scenario::PrivacyShieldStatus => privacy_status(app).await,
        Scenario::PrivacyShieldStartStop => privacy_start_stop(app).await,
    };
    match facts {
        Ok(facts) => pass(request, facts),
        Err((code, _)) => failed(request, code),
    }
}

fn fleet_preflight() -> Result<Value, (&'static str, String)> {
    let settings = crate::settings::read_settings().map_err(|e| ("settings_unavailable", e))?;
    Ok(json!({
        "fleetConfigured": settings.app.fleet.enabled && !settings.app.fleet.server_url.is_empty(),
        "pinnedSigningKey": settings.policy.fleet_signing_key.is_some(),
        "deviceIdHash": sha256(&settings.device_id),
    }))
}

async fn fleet_checkin_readback() -> Result<Value, (&'static str, String)> {
    let status = crate::fleet_agent::fleet_status()
        .await
        .map_err(|e| ("fleet_status_unavailable", e))?;
    Ok(json!({
        "connected": status.get("connected").and_then(Value::as_bool).unwrap_or(false),
        "pendingApproval": status.get("pendingApproval").and_then(Value::as_bool).unwrap_or(false),
        "hasEnrollmentTimestamp": status.get("lastEnrollAt").and_then(Value::as_str).is_some(),
        "hasTerminalError": status.get("lastError").and_then(Value::as_str).is_some(),
    }))
}

async fn clipboard_marker(
    app: tauri::AppHandle,
    request: &Request,
) -> Result<Value, (&'static str, String)> {
    let before = crate::paste_monitor::get_paste_monitor_recent()
        .await
        .map_err(|e| ("clipboard_observe_failed", e))?
        .len();
    crate::paste_monitor::start_paste_monitor(app)
        .await
        .map_err(|e| ("clipboard_start_failed", e))?;
    let marker = format!(
        "AKIA{}",
        sha256(&format!("{}:{}", request.run_id, request.fixture_id))[..16].to_ascii_uppercase()
    );
    #[cfg(windows)]
    clipboard_win::set_clipboard_string(&marker).map_err(|_| {
        (
            "clipboard_fixture_write_failed",
            "synthetic marker could not be written".into(),
        )
    })?;
    #[cfg(not(windows))]
    return Err((
        "platform_unsupported",
        "clipboard scenario requires Windows".into(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
    let after = crate::paste_monitor::get_paste_monitor_recent()
        .await
        .map_err(|e| ("clipboard_observe_failed", e))?
        .len();
    let health = crate::paste_monitor::get_paste_monitor_health()
        .await
        .map_err(|e| ("clipboard_observe_failed", e))?;
    if after <= before {
        return Err((
            "clipboard_marker_not_observed",
            "the production listener did not record the synthetic marker".into(),
        ));
    }
    Ok(json!({
        "markerDigest": sha256(&marker),
        "detectionCountDelta": after - before,
        "listenerRegistered": health.listener_registered,
        "rulesCompiled": health.rules_compiled,
        "clearFailing": health.clear_failing,
    }))
}

async fn privacy_status(app: tauri::AppHandle) -> Result<Value, (&'static str, String)> {
    let result = run_privacy_command(app, "Get-PrivacyShieldStatus").await?;
    Ok(json!({ "statusObserved": true, "statusHash": sha256(&result.to_string()) }))
}

async fn privacy_start_stop(app: tauri::AppHandle) -> Result<Value, (&'static str, String)> {
    run_privacy_command(app.clone(), "Start-PrivacyShield").await?;
    let status = run_privacy_command(app.clone(), "Get-PrivacyShieldStatus").await?;
    run_privacy_command(app, "Stop-PrivacyShield").await?;
    Ok(json!({ "startStopObserved": true, "runningStatusHash": sha256(&status.to_string()) }))
}

async fn run_privacy_command(
    app: tauri::AppHandle,
    command: &str,
) -> Result<Value, (&'static str, String)> {
    crate::backend::run_backend_script(app, command.to_string(), HashMap::new())
        .await
        .map_err(|e| ("privacy_handler_failed", e))
}

fn pass(request: &Request, facts: Value) -> ResultEnvelope {
    let evidence = facts.to_string();
    ResultEnvelope {
        schema: RESULT_SCHEMA,
        run_id: request.run_id.clone(),
        scenario: request.scenario,
        phase: "observe",
        outcome: "passed",
        observed_at: Utc::now().to_rfc3339(),
        facts,
        failure_code: None,
        redacted_evidence_hash: sha256(&evidence),
    }
}

fn failed(request: &Request, code: &'static str) -> ResultEnvelope {
    let failure_code = "TEST_ACTION_FAILED";
    ResultEnvelope {
        schema: RESULT_SCHEMA,
        run_id: request.run_id.clone(),
        scenario: request.scenario,
        phase: "report",
        outcome: "failed",
        observed_at: Utc::now().to_rfc3339(),
        facts: json!({}),
        failure_code: Some(failure_code),
        redacted_evidence_hash: sha256(code),
    }
}

/// Queue the closed result with the existing Pro Fleet agent. Pro owns the
/// durable device signing key and the single check-in loop; the Free test CLI
/// must never sign or post Fleet results itself.
async fn submit_result(
    capability: &fleet_proto::AutonomousTestCapability,
    result: &ResultEnvelope,
) -> Result<(), (&'static str, String)> {
    let response = crate::sidecar::dispatch_paid_command(
        "fleet_agent_autonomous_test_result",
        json!({ "capability": capability, "result": result }),
    )
    .await
    .map_err(|error| ("report_queue_failed", error))?;
    if response.get("queued").and_then(Value::as_bool) != Some(true) {
        return Err((
            "report_queue_failed",
            "Pro did not confirm the autonomous-test report queue".into(),
        ));
    }
    Ok(())
}

fn sha256(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

fn fail(code: &'static str, message: String) -> i32 {
    print_json(&json!({ "ok": false, "error": code, "message": message }));
    8
}

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string(value)
            .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"serialization_failed\"}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_catalog_rejects_unknown_scenarios() {
        let raw = r#"{"runId":"00000000-0000-4000-8000-000000000001","scenario":"shell","fixtureId":"00000000-0000-4000-8000-000000000002","deadlineMs":1000,"capability":{"version":2,"runId":"00000000-0000-4000-8000-000000000001","deviceId":"00000000-0000-4000-8000-000000000003","commandId":"00000000-0000-4000-8000-000000000004","catalogId":"mesh.status","actionClass":"safe","scenarios":[],"notBefore":"2026-01-01T00:00:00Z","expiresAt":"2027-01-01T00:00:00Z","issuerKeyId":"autonomy-lab-1","nonce":"x","signature":"x"}}"#;
        assert!(parse_request(&[
            "agent-test".into(),
            "run".into(),
            "--request".into(),
            raw.into()
        ])
        .is_err());
    }

    #[test]
    fn request_refuses_file_and_stdin_indirection() {
        for raw in ["@request.json", "-"] {
            assert!(parse_request(&[
                "agent-test".into(),
                "run".into(),
                "--request".into(),
                raw.into()
            ])
            .is_err());
        }
    }

    #[test]
    fn request_requires_the_disposable_fixture_namespace() {
        let raw = r#"{"runId":"00000000-0000-4000-8000-000000000001","scenario":"fleet.preflight","fixtureId":"C:/temp","deadlineMs":1000,"capability":{"version":2,"runId":"00000000-0000-4000-8000-000000000001","deviceId":"00000000-0000-4000-8000-000000000003","commandId":"00000000-0000-4000-8000-000000000004","catalogId":"mesh.status","actionClass":"safe","scenarios":["fleet.preflight"],"notBefore":"2026-01-01T00:00:00Z","expiresAt":"2027-01-01T00:00:00Z","issuerKeyId":"autonomy-lab-1","nonce":"x","signature":"x"}}"#;
        assert!(parse_request(&[
            "agent-test".into(),
            "run".into(),
            "--request".into(),
            raw.into()
        ])
        .is_err());
    }

    #[test]
    fn capability_preimage_binds_every_authority_field() {
        let base = fleet_proto::AutonomousTestCapability {
            version: 2,
            run_id: "r".into(),
            device_id: fleet_proto::DeviceId("d".into()),
            command_id: "c".into(),
            catalog_id: "mesh.status".into(),
            action_class: fleet_proto::ActionClass::Safe,
            scenarios: vec![Scenario::FleetPreflight.id().into()],
            not_before: "2026-01-01T00:00:00Z".into(),
            expires_at: "2027-01-01T00:00:00Z".into(),
            issuer_key_id: TEST_ISSUER_KEY_ID.into(),
            nonce: "n".into(),
            signature: "x".into(),
        };
        let changed = fleet_proto::AutonomousTestCapability {
            scenarios: vec![Scenario::PrivacyShieldStatus.id().into()],
            ..base
        };
        assert_ne!(
            fleet_proto::autonomous_test_capability_preimage(&changed),
            fleet_proto::autonomous_test_capability_preimage(
                &fleet_proto::AutonomousTestCapability {
                    scenarios: vec![Scenario::FleetPreflight.id().into()],
                    ..changed
                }
            )
        );
    }

    #[test]
    fn synthetic_marker_digest_does_not_contain_marker_text() {
        let marker = "AKIA0123456789ABCDEF";
        assert!(!sha256(marker).contains(marker));
    }
}
