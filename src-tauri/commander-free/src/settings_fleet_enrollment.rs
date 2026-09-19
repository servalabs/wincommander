// SPDX-License-Identifier: AGPL-3.0-or-later
//! Enrollment changes bind native approval to the observed policy and request.
use super::{merge_json, AppSettings};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

pub(crate) struct ConnectionPlan {
    previous: Value,
    server_url: String,
    dispatch: bool,
    signing_key: String,
    initial: bool,
    needs_confirmation: bool,
}

impl ConnectionPlan {
    pub(crate) fn new(
        current: &AppSettings,
        server_url: String,
        dispatch: bool,
        signing_key: String,
    ) -> Result<Self, String> {
        let signing_key = signing_key.trim().to_string();
        if !signing_key.is_empty() {
            if signing_key.len() != 44 {
                return Err("Fleet verification key must be a base64-encoded 32-byte key".into());
            }
            let decoded = STANDARD
                .decode(&signing_key)
                .map_err(|_| "Invalid Fleet verification key")?;
            let bytes: [u8; 32] = decoded
                .try_into()
                .map_err(|_| "Fleet verification key must be 32 bytes")?;
            ed25519_dalek::VerifyingKey::from_bytes(&bytes)
                .map_err(|_| "Invalid Fleet verification key")?;
        }
        let policy_key = current
            .policy
            .fleet_signing_key
            .as_deref()
            .filter(|key| !key.is_empty());
        if signing_key.is_empty()
            && (policy_key.is_some() || !current.app.fleet.signing_key_pub.is_empty())
        {
            return Err("An enrolled Fleet verification key cannot be cleared by Connect".into());
        }
        let prior = binding(current);
        let changing = policy_key.is_some_and(|key| key != signing_key)
            || (!current.app.fleet.signing_key_pub.is_empty()
                && current.app.fleet.signing_key_pub != signing_key)
            || (!current.app.fleet.server_url.is_empty()
                && current.app.fleet.server_url != server_url)
            || (super::local_write::managed(current) && current.app.fleet.dispatch != dispatch);
        let initial = !super::local_write::managed(current)
            && current.app.fleet.server_url.is_empty()
            && current.app.fleet.signing_key_pub.is_empty()
            && !current.app.fleet.enabled;
        Ok(Self {
            previous: prior,
            server_url,
            dispatch,
            signing_key,
            initial,
            needs_confirmation: changing,
        })
    }

    pub(crate) fn needs_confirmation(&self) -> bool {
        self.needs_confirmation
    }

    pub(crate) fn confirmation_binding(&self) -> String {
        json!({ "previous": self.previous, "serverUrl": self.server_url,
            "signingKey": self.signing_key, "dispatch": self.dispatch })
        .to_string()
    }

    pub(crate) fn commit(&self, confirmed: bool) -> Result<AppSettings, String> {
        let paid = crate::license::has_paid_entitlement();
        self.commit_with(
            confirmed,
            paid,
            super::write_settings_internal,
            |old, new| {
                crate::flow_bridge::on_settings_written(old, new);
            },
        )
    }

    fn commit_with(
        &self,
        confirmed: bool,
        paid: bool,
        persist: impl FnOnce(&AppSettings) -> Result<(), String>,
        notify: impl FnOnce(&Value, &Value),
    ) -> Result<AppSettings, String> {
        super::mutate_settings_with(
            |current| self.prepare(current, confirmed),
            paid,
            persist,
            notify,
        )
    }

    fn prepare(&self, current: &AppSettings, confirmed: bool) -> Result<AppSettings, String> {
        if binding(current) != self.previous {
            return Err("Fleet settings changed while confirming; retry Connect".into());
        }
        if self.needs_confirmation && !confirmed {
            return Err("Native re-enrollment confirmation is required".into());
        }
        let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
        let mut patch = json!({
            "app": { "fleet": { "enabled": true, "serverUrl": self.server_url,
                "dispatch": self.dispatch, "signingKeyPub": self.signing_key } },
            "policy": { "fleetSigningKey": if self.signing_key.is_empty() { Value::Null } else { json!(self.signing_key) } }
        });
        // Only a first enrollment initializes local defaults. Reconnect must not
        // turn off a signed desired state or take over the user's active session.
        if self.initial {
            patch["app"]["fleet"]["privacyShieldSessionOwned"] = json!(false);
            patch["app"]["modules"] = json!({"privacyShield": true});
            patch["ideal"] = json!({ "privacy": { "privacyShield": {
                "fleetManaged": false, "fleetMonitoringEnabled": false
            } } });
        }
        merge_json(&mut value, &patch);
        serde_json::from_value(value).map_err(|e| e.to_string())
    }
}

fn binding(current: &AppSettings) -> Value {
    json!({ "deviceId": current.device_id, "policy": current.policy,
        "serverUrl": current.app.fleet.server_url, "signingKey": current.app.fleet.signing_key_pub,
        "enabled": current.app.fleet.enabled, "dispatch": current.app.fleet.dispatch })
}

#[cfg(test)]
#[path = "settings_fleet_enrollment_tests.rs"]
mod tests;
