use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::catalog::Scenario;
use super::parser::Request;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResultEnvelope {
    schema: &'static str,
    run_id: String,
    scenario: Scenario,
    phase: &'static str,
    pub(super) outcome: &'static str,
    observed_at: String,
    facts: Value,
    failure_code: Option<&'static str>,
    redacted_evidence_hash: String,
}

pub(super) fn passed(request: &Request, facts: Value) -> ResultEnvelope {
    envelope(request, "observe", "passed", facts, None)
}

pub(super) fn failed(request: &Request, code: &'static str) -> ResultEnvelope {
    envelope(request, "report", "failed", json!({}), Some(code))
}

fn envelope(
    request: &Request,
    phase: &'static str,
    outcome: &'static str,
    facts: Value,
    failure_code: Option<&'static str>,
) -> ResultEnvelope {
    ResultEnvelope {
        schema: "wincommander-autonomous-test-result/v1",
        run_id: request.run_id.clone(),
        scenario: request.scenario,
        phase,
        outcome,
        observed_at: Utc::now().to_rfc3339(),
        redacted_evidence_hash: sha256(&facts.to_string()),
        facts,
        failure_code,
    }
}

pub(super) fn sha256(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

pub(super) fn fail(code: &str, message: &str) -> i32 {
    print_json(&json!({ "ok": false, "error": code, "message": message }));
    8
}

pub(super) fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string(value).unwrap_or_else(|_| "{\"ok\":false}".into())
    );
}
