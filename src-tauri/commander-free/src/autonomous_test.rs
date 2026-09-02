// SPDX-License-Identifier: AGPL-3.0-or-later
//! Closed Fleet-lab controls for a disposable debug artifact.
//!
//! This module deliberately does not contact Fleet. Joining and leaving require
//! a server-verified, one-time capability that is implemented by the paired
//! Fleet and Pro feature. Until that contract is present, every mutation fails
//! closed rather than falling back to the GUI or a broad local dispatcher.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_CAPABILITY_BYTES: usize = 4096;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JoinRequest {
    schema: String,
    run_id: String,
    capability: String,
    deadline_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupRequest {
    schema: String,
    run_id: String,
    capability: String,
}

pub fn is_invocation(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("fleet-lab"))
}

pub fn main(args: Vec<String>) -> i32 {
    match args.as_slice() {
        [verb, action] if verb == "fleet-lab" && action == "preflight" => preflight(),
        [verb, action] if verb == "fleet-lab" && action == "enrollment-request" => enrollment_request(),
        [verb, action] if verb == "fleet-lab" && action == "status" => status(),
        [verb, action, flag, raw]
            if verb == "fleet-lab" && action == "join" && flag == "--request" =>
        {
            join(raw)
        }
        [verb, action, flag, raw]
            if verb == "fleet-lab" && action == "leave-and-cleanup" && flag == "--request" =>
        {
            leave_and_cleanup(raw)
        }
        _ => fail(
            "invalid_request",
            "usage: fleet-lab <preflight|enrollment-request|status> | fleet-lab join --request <json> | fleet-lab leave-and-cleanup --request <json>",
        ),
    }
}

fn preflight() -> i32 {
    let settings = match crate::settings::read_settings() {
        Ok(settings) => settings,
        Err(_) => return fail("settings_unavailable", "settings could not be read"),
    };
    print_json(json!({
        "ok": true,
        "schema": "wincommander-fleet-lab-preflight/v1",
        "debugBuild": cfg!(debug_assertions),
        "deviceIdHash": sha256(&settings.device_id),
        "fleetConfigured": settings.app.fleet.enabled && !settings.app.fleet.server_url.is_empty(),
        "pinnedSigningKey": settings.policy.fleet_signing_key.is_some(),
    }));
    0
}

/// Returns the stable local identity that Fleet must bind into a one-use lab
/// admission pass. This is debug-build-only together with the whole command
/// surface; joining never changes the client's identity.
fn enrollment_request() -> i32 {
    let settings = match crate::settings::read_settings() {
        Ok(settings) => settings,
        Err(_) => return fail("settings_unavailable", "settings could not be read"),
    };
    if uuid::Uuid::parse_str(&settings.device_id).is_err() {
        return fail(
            "invalid_device_identity",
            "local device identity is invalid",
        );
    }
    print_json(json!({
        "ok": true,
        "schema": "wincommander-fleet-lab-enrollment-request/v1",
        "deviceId": settings.device_id,
    }));
    0
}

fn status() -> i32 {
    let settings = match crate::settings::read_settings() {
        Ok(settings) => settings,
        Err(_) => return fail("settings_unavailable", "settings could not be read"),
    };
    let configured = settings.app.fleet.enabled && !settings.app.fleet.server_url.is_empty();
    print_json(json!({
        "ok": true,
        "schema": "wincommander-fleet-lab-status/v1",
        "deviceIdHash": sha256(&settings.device_id),
        "fleetConfigured": configured,
        "originHash": configured.then(|| sha256(&settings.app.fleet.server_url)),
        "signerFingerprint": settings.policy.fleet_signing_key.as_deref().map(sha256),
        "authoritativeFleetState": "unavailable",
    }));
    0
}

fn join(raw: &str) -> i32 {
    let request = match parse_join(raw) {
        Ok(request) => request,
        Err(message) => return fail("invalid_request", &message),
    };
    let capability = match decode_enrollment_capability(&request.capability) {
        Ok(capability) => capability,
        Err(message) => return fail("invalid_request", &message),
    };
    let settings = match crate::settings::read_settings() {
        Ok(settings) => settings,
        Err(_) => return fail("settings_unavailable", "settings could not be read"),
    };
    if capability.device_id.0 != settings.device_id {
        return fail(
            "device_identity_mismatch",
            "Fleet lab capability is not bound to this client",
        );
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return fail("runtime_unavailable", "could not start bounded lab bridge"),
    };
    match runtime.block_on(crate::sidecar::dispatch_paid_command(
        "fleet_lab_join",
        json!({ "schema": request.schema, "runId": request.run_id, "capability": request.capability }),
    )) {
        Ok(response) => { print_json(json!({ "ok": true, "result": response })); 0 }
        Err(_) => fail("lab_join_rejected", "Fleet lab enrollment was rejected or the Pro sidecar is unavailable"),
    }
}

fn leave_and_cleanup(raw: &str) -> i32 {
    let request = match parse_cleanup(raw) {
        Ok(request) => request,
        Err(message) => return fail("invalid_request", &message),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return fail(
                "runtime_unavailable",
                "could not start bounded cleanup bridge",
            )
        }
    };
    match runtime.block_on(crate::sidecar::dispatch_paid_command(
        "fleet_lab_cleanup",
        json!({ "schema": request.schema, "runId": request.run_id, "capability": request.capability }),
    )) {
        Ok(response) => { print_json(json!({ "ok": true, "result": response })); 0 }
        Err(_) => fail("lab_cleanup_rejected", "Fleet lab cleanup was rejected or the Pro sidecar is unavailable"),
    }
}

fn parse_join(raw: &str) -> Result<JoinRequest, String> {
    let request: JoinRequest = parse_inline(raw)?;
    if request.schema != "wincommander-fleet-lab-join/v1" {
        return Err("schema must be wincommander-fleet-lab-join/v1".into());
    }
    validate_run_and_capability(&request.run_id, &request.capability)?;
    if !(1_000..=300_000).contains(&request.deadline_ms) {
        return Err("deadlineMs must be between 1000 and 300000".into());
    }
    Ok(request)
}

fn parse_cleanup(raw: &str) -> Result<CleanupRequest, String> {
    let request: CleanupRequest = parse_inline(raw)?;
    if request.schema != "wincommander-fleet-lab-cleanup/v1" {
        return Err("schema must be wincommander-fleet-lab-cleanup/v1".into());
    }
    validate_run_and_capability(&request.run_id, &request.capability)?;
    Ok(request)
}

fn parse_inline<T: for<'a> Deserialize<'a>>(raw: &str) -> Result<T, String> {
    if raw.len() > MAX_REQUEST_BYTES || raw.starts_with('@') || raw == "-" {
        return Err("request must be bounded inline JSON, not a file or stdin reference".into());
    }
    serde_json::from_str(raw).map_err(|_| "request must match the fixed schema".into())
}

fn validate_run_and_capability(run_id: &str, capability: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(run_id).is_err() {
        return Err("runId must be a UUID".into());
    }
    if capability.is_empty()
        || capability.len() > MAX_CAPABILITY_BYTES
        || !capability
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("capability must be a bounded base64url value".into());
    }
    Ok(())
}

fn decode_enrollment_capability(
    encoded: &str,
) -> Result<fleet_proto::FleetLabEnrollmentCapability, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "capability must be valid base64url".to_string())?;
    let capability = serde_json::from_slice::<fleet_proto::FleetLabEnrollmentCapability>(&bytes)
        .map_err(|_| "capability must match the Fleet lab enrollment schema".to_string())?;
    if uuid::Uuid::parse_str(&capability.device_id.0).is_err() {
        return Err("capability deviceId must be a UUID".into());
    }
    Ok(capability)
}

fn sha256(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

fn fail(code: &str, message: &str) -> i32 {
    print_json(json!({ "ok": false, "error": code, "message": message }));
    8
}

fn print_json(value: serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| "{\"ok\":false}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN_ID: &str = "00000000-0000-4000-8000-000000000001";

    #[test]
    fn join_refuses_unknown_fields_and_file_indirection() {
        assert!(parse_join("@request.json").is_err());
        assert!(parse_join(&format!(r#"{{"schema":"wincommander-fleet-lab-join/v1","runId":"{RUN_ID}","capability":"abc","deadlineMs":1000,"url":"https://bad"}}"#)).is_err());
    }

    #[test]
    fn join_requires_expected_schema_uuid_capability_and_deadline() {
        assert!(parse_join(&format!(
            r#"{{"schema":"wrong","runId":"{RUN_ID}","capability":"abc","deadlineMs":1000}}"#
        ))
        .is_err());
        assert!(parse_join(r#"{"schema":"wincommander-fleet-lab-join/v1","runId":"bad","capability":"abc","deadlineMs":1000}"#).is_err());
        assert!(parse_join(&format!(r#"{{"schema":"wincommander-fleet-lab-join/v1","runId":"{RUN_ID}","capability":"abc","deadlineMs":999}}"#)).is_err());
    }

    #[test]
    fn cleanup_accepts_only_its_fixed_contract() {
        assert!(parse_cleanup(&format!(r#"{{"schema":"wincommander-fleet-lab-cleanup/v1","runId":"{RUN_ID}","capability":"a-b_C"}}"#)).is_ok());
        assert!(parse_cleanup(&format!(
            r#"{{"schema":"wincommander-fleet-lab-join/v1","runId":"{RUN_ID}","capability":"abc"}}"#
        ))
        .is_err());
    }

    #[test]
    fn join_capability_requires_a_real_device_uuid() {
        let capability = json!({
            "version": 1,
            "capabilityId": "00000000-0000-4000-8000-000000000010",
            "orgId": "local",
            "deviceId": "00000000-0000-4000-8000-000000000011",
            "fleetOrigin": "http://127.0.0.1:8788",
            "notBefore": "2026-01-01T00:00:00Z",
            "expiresAt": "2026-01-01T00:05:00Z",
            "issuerKeyId": "fleet-lab-enrollment-1",
            "nonce": "n",
            "signature": "s",
        });
        let encoded = URL_SAFE_NO_PAD.encode(capability.to_string());
        assert!(decode_enrollment_capability(&encoded).is_ok());

        let malformed = URL_SAFE_NO_PAD.encode(
            capability
                .to_string()
                .replace("00000000-0000-4000-8000-000000000011", "not-a-uuid"),
        );
        assert!(decode_enrollment_capability(&malformed).is_err());
    }
}
