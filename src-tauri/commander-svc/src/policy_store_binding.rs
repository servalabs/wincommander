// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use fleet_proto::PolicyEnvelope;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

impl PolicyStore {
    /// Called while FleetMachinePolicy holds its current trust/policy lock.
    pub fn bound_clipboard_response(
        &self,
        trust_bound: bool,
        policy: Option<&PolicyEnvelope>,
    ) -> Result<Value, &'static str> {
        let content = self
            .content_rules
            .lock()
            .map_err(|_| "clipboard_store_unavailable")?;
        let clipboard = self
            .clipboard
            .lock()
            .map_err(|_| "clipboard_store_unavailable")?;
        let active = if content.raw.is_some() {
            content.input.as_ref()
        } else {
            clipboard.input.as_ref()
        };
        if !trust_bound {
            return if active.is_none() {
                Ok(json!({"policy_version":0,"rules":[]}))
            } else {
                Err("clipboard_trust_unavailable")
            };
        }
        let policy = policy.ok_or("clipboard_policy_unavailable")?;
        let current = policy
            .config
            .as_ref()
            .ok_or("clipboard_policy_unavailable")?;
        if current.org_id != policy.org_id || current.signer_key != policy.signer_key {
            return Err("clipboard_policy_identity_mismatch");
        }
        match active {
            Some(active) => {
                if active.signer_key_b64 != policy.signer_key
                    || ["contentRules", "clipboardGuard"]
                        .iter()
                        .any(|key| active.config.get(*key) != current.config_json.get(*key))
                {
                    return Err("clipboard_policy_intent_mismatch");
                }
            }
            None if current.config_json.get("contentRules").is_some()
                || current.config_json.get("clipboardGuard").is_some() =>
            {
                return Err("clipboard_policy_not_installed")
            }
            None => {}
        }
        let identity = serde_json::to_vec(&[
            policy.signer_key.as_str(),
            policy.org_id.0.as_str(),
            policy.device_id.0.as_str(),
            policy.enrollment_generation.as_deref().unwrap_or(""),
        ])
        .map_err(|_| "clipboard_policy_identity_unavailable")?;
        let binding: String = Sha256::digest(&identity)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut response = if let Some(raw) = content.raw.as_ref() {
            json!({"policy_version":current.version,"rules":[],"content_rules":raw})
        } else {
            json!({"policy_version":current.version,"rules":clipboard.raw.as_ref().cloned().unwrap_or_default()})
        };
        response["reporting_binding"] = json!(binding);
        Ok(response)
    }
}
