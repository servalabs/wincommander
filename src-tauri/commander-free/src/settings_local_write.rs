// SPDX-License-Identifier: AGPL-3.0-or-later
//! Renderer mutations share one authorization and atomic persistence boundary.
use super::{merge_json, mutate_settings_with, AppSettings};
use serde_json::Value;

const MAX_IMPORT_BYTES: usize = 4 * 1024 * 1024;

pub(super) enum Mutation {
    Patch(Value),
    Replace(Value),
    Import(String),
}

pub(super) fn managed(settings: &AppSettings) -> bool {
    settings.policy.managed
        || settings.policy.sync_mode == "managed"
        || settings.policy.fleet_signing_key.is_some()
        || !settings.policy.locked_paths.is_empty()
}

pub(super) fn apply(mutation: Mutation) -> Result<AppSettings, String> {
    if super::is_decoy_mode() {
        return Err("Settings are read-only in decoy mode.".to_string());
    }
    let paid = crate::license::has_paid_entitlement();
    apply_with(
        mutation,
        paid,
        super::write_settings_internal,
        |old, new| {
            crate::flow_bridge::on_settings_written(old, new);
        },
    )
}

fn apply_with(
    mutation: Mutation,
    paid: bool,
    persist: impl FnOnce(&AppSettings) -> Result<(), String>,
    notify: impl FnOnce(&Value, &Value),
) -> Result<AppSettings, String> {
    // Authorization and persistence share the writer transaction, not the snapshot lock.
    mutate_settings_with(|current| prepare(current, mutation), paid, persist, notify)
}

fn prepare(current: &AppSettings, mutation: Mutation) -> Result<AppSettings, String> {
    let candidate = match mutation {
        Mutation::Patch(patch) => {
            if !patch.is_object() {
                return Err("Settings patch must be an object".into());
            }
            let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
            merge_json(&mut value, &patch);
            serde_json::from_value(value).map_err(|e| format!("Invalid settings format: {e}"))?
        }
        Mutation::Replace(value) => {
            serde_json::from_value(value).map_err(|e| format!("Invalid settings format: {e}"))?
        }
        Mutation::Import(json) => {
            if json.len() > MAX_IMPORT_BYTES {
                return Err("Settings file exceeds the 4 MiB limit".into());
            }
            let mut imported: AppSettings =
                serde_json::from_str(&json).map_err(|e| format!("Import failed: {e}"))?;
            imported.device_id = current.device_id.clone();
            imported.created_at = current.created_at.clone();
            imported.app_version = super::get_app_version();
            imported
        }
    };
    validate(current, &candidate)?;
    Ok(candidate)
}

fn at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |parent, component| parent.get(component))
}

fn validate(current: &AppSettings, candidate: &AppSettings) -> Result<(), String> {
    let before_slots = &current.app.vault.quick_mount_slots;
    let after_slots = &candidate.app.vault.quick_mount_slots;
    if serde_json::to_value(before_slots).ok() != serde_json::to_value(after_slots).ok() {
        if after_slots.len() > 128 { return Err("Too many Quick Mount entries".into()); }
        let mut counts = std::collections::HashMap::new();
        for slot in after_slots {
            let letter = slot.drive_letter.trim_end_matches(':').to_ascii_uppercase();
            if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_uppercase() { return Err("Invalid Quick Mount drive letter".into()); }
            let count = counts.entry(letter.clone()).or_insert(0usize);
            *count += 1;
            let old_count = before_slots.iter().filter(|old| old.drive_letter.trim_end_matches(':').eq_ignore_ascii_case(&letter)).count();
            if (*count > 1 || letter.as_str() <= "C") && *count > old_count {
                return Err("Quick Mount cannot add duplicate or protected drive-letter reservations".into());
            }
        }
    }
    if current.device_id != candidate.device_id || current.created_at != candidate.created_at {
        return Err("Device identity is owned by WinCommander, not settings input".into());
    }
    if !managed(current) {
        return Ok(());
    }
    let before = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let after = serde_json::to_value(candidate).map_err(|e| e.to_string())?;
    // Local restoration cannot remove management, swap a verification key,
    // reset an epoch, or replace the signed control-plane connection.
    for path in [
        "policy",
        "settingsVersion",
        "app.fleet.enabled",
        "app.fleet.serverUrl",
        "app.fleet.dispatch",
        "app.fleet.signingKeyPub",
        "app.fleet.shieldDesiredState",
        "app.fleet.privacyShieldAppliedRevision",
        "app.fleet.privacyShieldAppliedCommandId",
    ] {
        if at_path(&before, path) != at_path(&after, path) {
            return Err(
                "Managed policy and Fleet identity can only change through their native authority"
                    .into(),
            );
        }
    }
    // privacyShieldSessionOwned is a local UI/session observation, not a
    // policy authority or a signed desired-state value; its existing callers remain valid.
    for lock in &current.policy.locked_paths {
        let path = if lock == "app.flows" {
            "app.proFlows".to_string()
        } else if lock.is_empty() {
            "ideal".to_string()
        } else if matches!(
            lock.split('.').next(),
            Some("ideal" | "current" | "app" | "policy")
        ) {
            lock.clone()
        } else {
            format!("ideal.{lock}")
        };
        if path.split('.').any(str::is_empty) {
            return Err("Managed policy contains an invalid locked path".into());
        }
        if at_path(&before, &path) != at_path(&after, &path) {
            return Err(format!("Setting '{lock}' is locked by admin policy"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "settings_local_write_tests.rs"]
mod tests;
