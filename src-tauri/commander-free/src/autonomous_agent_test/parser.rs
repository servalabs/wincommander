use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::catalog::Scenario;

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const TEST_ISSUER_KEY_ID: &str = "autonomy-lab-1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    pub(super) run_id: String,
    pub(super) scenario: Scenario,
    pub(super) fixture_id: String,
    deadline_ms: u64,
    pub(super) capability: fleet_proto::AutonomousTestCapability,
}

pub(super) fn parse_request(args: &[String]) -> Result<Request, String> {
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

pub(super) fn valid_fixture_id(value: &str) -> bool {
    value.len() <= 96
        && value.starts_with("wc-test-")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(super) fn validate_capability(request: Request) -> Result<Request, String> {
    let settings = crate::settings::read_settings()?;
    let capability = &request.capability;
    let (catalog_id, action_class) = request.scenario.command_binding();
    if capability.run_id != request.run_id
        || capability.device_id.0 != settings.device_id
        || capability.catalog_id != catalog_id
        || capability.action_class != action_class
        || !capability
            .scenarios
            .iter()
            .any(|id| id == request.scenario.id())
    {
        return Err("capability binding is invalid".into());
    }
    let not_before = parse_time(&capability.not_before, "notBefore")?;
    let expires_at = parse_time(&capability.expires_at, "expiresAt")?;
    let now = Utc::now();
    if now < not_before || now >= expires_at {
        return Err("capability is not currently valid".into());
    }
    let signer = settings
        .policy
        .fleet_signing_key
        .ok_or_else(|| "device has no pinned Fleet signing key".to_string())?;
    if !fleet_proto::verify_autonomous_test_capability(capability, TEST_ISSUER_KEY_ID, &signer) {
        return Err("capability signature is invalid".into());
    }
    Ok(request)
}

fn parse_time(value: &str, field: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| format!("capability {field} is invalid"))
}
