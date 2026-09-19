// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native policy authority: verify and advance the epoch within one transaction.
use super::{merge_json, mutate_settings_with, AppSettings};
use serde_json::Value;

pub(super) struct Epoch {
    pub config: Value,
    pub locked_paths: Vec<String>,
    pub strategy: String,
    pub version: u32,
    pub signature: Option<String>,
    pub signer_key: Option<String>,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub managed: bool,
}

pub(super) fn apply(epoch: &Epoch) -> Result<AppSettings, String> {
    apply_with(epoch, super::write_settings_internal)
}

fn apply_with(
    epoch: &Epoch,
    persist: impl FnOnce(&AppSettings) -> Result<(), String>,
) -> Result<AppSettings, String> {
    mutate_settings_with(|current| prepare(current, epoch), false, persist, |_, _| {})
}

fn prepare(current: &AppSettings, epoch: &Epoch) -> Result<AppSettings, String> {
    let pinned = current
        .policy
        .fleet_signing_key
        .as_deref()
        .ok_or("cannot apply fleet policy: no pinned fleet signing key")?;
    let signature = epoch
        .signature
        .as_deref()
        .ok_or("fleet-managed device: config push requires a signature")?;
    if epoch
        .signer_key
        .as_deref()
        .is_some_and(|provided| provided != pinned)
    {
        return Err("config push signer key does not match the pinned fleet key".into());
    }
    if !epoch.config.is_object() || serde_json::to_string(&epoch.config).is_err() {
        return Err("config push must be a canonically serializable object".into());
    }
    if !matches!(epoch.strategy.as_str(), "merge" | "overwrite") {
        return Err("Unsupported policy merge strategy".into());
    }
    let message = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
        version: i64::from(epoch.version),
        config: &epoch.config,
        locked_paths: &epoch.locked_paths,
        managed: epoch.managed,
        target_kind: epoch.target_kind.as_deref().unwrap_or("org"),
        target_id: epoch.target_id.as_deref(),
    });
    if !wincmd_shared::fleet::verify_signature_b64(pinned, &message, signature) {
        return Err("config push signature verification failed".into());
    }
    if epoch.version <= current.policy.master_config_version.unwrap_or(0) {
        return Err("Config epoch is not newer than the committed policy".into());
    }
    // The signed config owns desired state, not local credentials, connection
    // identity or its own verification key. Overwrite only resets desired state.
    let mut candidate = current.clone();
    if epoch.strategy == "overwrite" {
        candidate.ideal = super::SystemState::default();
    }
    let mut value = serde_json::to_value(&candidate).map_err(|e| e.to_string())?;
    merge_json(&mut value, &serde_json::json!({"ideal": &epoch.config}));
    candidate =
        serde_json::from_value(value).map_err(|e| format!("Invalid policy configuration: {e}"))?;
    candidate.policy.locked_paths = epoch.locked_paths.clone();
    candidate.policy.last_synced_at = Some(super::now_iso8601());
    candidate.policy.master_config_version = Some(epoch.version);
    candidate.policy.sync_mode = "managed".into();
    candidate.policy.managed = epoch.managed;
    Ok(candidate)
}

#[cfg(test)]
#[path = "settings_epoch_write_tests.rs"]
mod tests;
