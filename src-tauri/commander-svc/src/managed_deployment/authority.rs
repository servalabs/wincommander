// SPDX-License-Identifier: AGPL-3.0-or-later
//! Independent service authorization; a relay cannot select its own trust pin.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use wincmd_shared::{
    fleet::{canonical_command_bytes, canonical_json_sha256, verify_signature_b64, ActionClass},
    managed_deployment::{ManagedDeploymentContext, ManagedDeploymentRequest},
};

pub const MAX_JOBS: usize = 256;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalEntry {
    digest: String,
    result: Option<Value>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub context: Option<ManagedDeploymentContext>,
    max_epoch: i64,
    jobs: BTreeMap<String, JournalEntry>,
}

pub enum Decision {
    Apply { setup_id: String },
    Observed(Value),
}

fn bounded_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

impl Authority {
    pub fn bind(&mut self, context: ManagedDeploymentContext) -> Result<(), &'static str> {
        if !bounded_id(&context.org_id, 80)
            || !bounded_id(&context.device_id, 36)
            || STANDARD
                .decode(&context.signing_key_b64)
                .map_or(true, |key| key.len() != 32)
        {
            return Err("management_context_invalid");
        }
        if let Some(existing) = &self.context {
            return if existing == &context {
                Ok(())
            } else {
                Err("management_context_conflict")
            };
        }
        self.context = Some(context);
        Ok(())
    }

    pub fn authorize(
        &mut self,
        request: &ManagedDeploymentRequest,
        now: i64,
    ) -> Result<Decision, &'static str> {
        let context = self
            .context
            .as_ref()
            .ok_or("management_service_unprovisioned")?;
        let command = &request.command;
        let payload = command
            .payload
            .as_object()
            .ok_or("deployment_authority_invalid")?;
        if payload.len() != 4
            || command.catalog_id != "managed_scanner.setup"
            || command.action_class != ActionClass::Destructive
            || command.signer_key != context.signing_key_b64
            || command.device_id.0 != context.device_id
            || command.epoch_version < self.max_epoch
            || command.idempotency_key.is_empty()
            || command.idempotency_key.len() > 128
            || !command.idempotency_key.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err("deployment_authority_invalid");
        }
        let signed = canonical_command_bytes(
            &command.idempotency_key,
            &command.device_id.0,
            &command.catalog_id,
            command.action_class.as_wire_str(),
            &command.payload,
            command.epoch_version,
        );
        if !verify_signature_b64(&context.signing_key_b64, &signed, &command.signature) {
            return Err("deployment_signature_invalid");
        }
        let field = |key: &str| {
            request
                .bundle
                .get(key)
                .and_then(Value::as_str)
                .ok_or("deployment_bundle_invalid")
        };
        let setup_id = field("setup_id")?;
        let deployment_id = field("deployment_id")?;
        if !bounded_id(setup_id, 36)
            || !bounded_id(deployment_id, 80)
            || field("org_id")? != context.org_id
            || field("device_id")? != context.device_id
            || payload.get("setup_id").and_then(Value::as_str) != Some(setup_id)
            || payload.get("deployment_id").and_then(Value::as_str) != Some(deployment_id)
            || payload.get("device_id").and_then(Value::as_str) != Some(context.device_id.as_str())
        {
            return Err("deployment_target_invalid");
        }
        let digest = canonical_json_sha256(&request.bundle);
        if payload.get("bundle_sha256").and_then(Value::as_str) != Some(digest.as_str()) {
            return Err("deployment_digest_invalid");
        }
        if let Some(existing) = self.jobs.get(setup_id) {
            if existing.digest != digest {
                return Err("deployment_replay_conflict");
            }
            return Ok(Decision::Observed(existing.result.clone().unwrap_or_else(
                || {
                    json!({
                        "schema_version":1,"setup_id":setup_id,"device_id":context.device_id,
                        "state":"unknown","reason_code":"installation_outcome_unknown"
                    })
                },
            )));
        }
        let expires = request
            .bundle
            .get("expires_at")
            .and_then(Value::as_i64)
            .ok_or("deployment_expiry_invalid")?;
        if now <= 0 || expires <= now || expires.saturating_sub(now) > 86_400 {
            return Err("deployment_expired");
        }
        if self.jobs.len() >= MAX_JOBS {
            return Err("deployment_journal_full");
        }
        self.max_epoch = command.epoch_version;
        self.jobs.insert(
            setup_id.into(),
            JournalEntry {
                digest,
                result: None,
            },
        );
        Ok(Decision::Apply {
            setup_id: setup_id.into(),
        })
    }

    pub fn complete(&mut self, setup_id: &str, result: Value) -> Result<Value, &'static str> {
        let context = self
            .context
            .as_ref()
            .ok_or("management_service_unprovisioned")?;
        let object = result.as_object().ok_or("deployment_receipt_invalid")?;
        let allowed = [
            "schema_version",
            "setup_id",
            "device_id",
            "client_id",
            "state",
            "reason_code",
        ];
        if object.keys().any(|key| !allowed.contains(&key.as_str()))
            || result["schema_version"] != 1
            || result["setup_id"] != setup_id
            || result["device_id"] != context.device_id
            || !matches!(
                result["state"].as_str(),
                Some("installed" | "already_installed" | "failed" | "unknown")
            )
            || result
                .get("client_id")
                .is_some_and(|value| !value.as_str().is_some_and(|id| bounded_id(id, 40)))
            || result
                .get("reason_code")
                .is_some_and(|value| !value.as_str().is_some_and(|id| bounded_id(id, 80)))
        {
            return Err("deployment_receipt_invalid");
        }
        let entry = self
            .jobs
            .get_mut(setup_id)
            .ok_or("deployment_receipt_invalid")?;
        if let Some(existing) = &entry.result {
            return if existing == &result {
                Ok(existing.clone())
            } else {
                Err("deployment_receipt_conflict")
            };
        }
        entry.result = Some(result.clone());
        Ok(result)
    }
}
