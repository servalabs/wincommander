// SPDX-License-Identifier: AGPL-3.0-or-later
//! Adoption rechecks authoritative intent under the existing machine lease and personal CAS.
use super::{local_write, AppSettings};
use serde_json::Value;
use std::cell::RefCell;

pub(super) fn apply(patch: Value) -> Result<AppSettings, String> {
    let lease = RefCell::new(None);
    let paid = crate::license::has_paid_entitlement();
    apply_with(
        patch,
        paid,
        || {
            let elevated = crate::process_privileges::current_process_elevation()?;
            let writer = super::startup_scope::acquire_writer(
                elevated,
                crate::paths::current_user_install_uses_local_datastore(),
            )?;
            *lease.borrow_mut() = Some((elevated, writer));
            let current = super::load_settings_from_store()?;
            // Legacy personal files have no cross-process compare-and-swap receipt.
            if !super::personal_settings::persisted_service_available()
                || !super::personal_settings::automation_available()
            {
                return Err("Automatic adoption requires verified service-backed settings".into());
            }
            Ok(current)
        },
        |settings| {
            let (elevated, writer) = lease
                .borrow_mut()
                .take()
                .ok_or("Settings adoption lease missing")?;
            super::write_settings_under_lease(settings, elevated, writer.is_some())
        },
        |old, new| crate::flow_bridge::on_settings_written(old, new),
    )
}

fn apply_with(
    patch: Value,
    paid: bool,
    load: impl FnOnce() -> Result<AppSettings, String>,
    persist: impl FnOnce(&AppSettings) -> Result<(), String>,
    notify: impl FnOnce(&Value, &Value),
) -> Result<AppSettings, String> {
    super::mutate_settings_from(
        |current| {
            prepare(current, &patch).map_err(|error| {
                // The fresh load may already have advanced the personal session
                // revision. Never leave an older cache beside that revision,
                // even when policy/input validation rejects before persistence.
                super::invalidate_cache_locked();
                error
            })
        },
        paid,
        load,
        persist,
        notify,
    )
}

fn prepare(current: &AppSettings, patch: &Value) -> Result<AppSettings, String> {
    let entries = patch
        .as_object()
        .ok_or("Adoption patch must be an object")?;
    if entries.len() != 1 || !entries.contains_key("ideal") {
        return Err("Adoption accepts only ideal preferences".into());
    }
    let mut snapshot = serde_json::to_value(current).map_err(|error| error.to_string())?;
    let observed = snapshot["current"].clone();
    adopt(&mut snapshot["ideal"], &observed, &patch["ideal"], 0)?;
    local_write::prepare(current, local_write::Mutation::Replace(snapshot))
}

fn adopt(intent: &mut Value, observed: &Value, patch: &Value, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("Adoption patch is too deeply nested".into());
    }
    match patch {
        Value::Object(entries) if !entries.is_empty() => {
            let object = intent
                .as_object_mut()
                .ok_or("Unknown adoption preference")?;
            for (name, child) in entries {
                let target = object.get_mut(name).ok_or("Unknown adoption preference")?;
                adopt(target, &observed[name], child, depth + 1)?;
            }
            Ok(())
        }
        Value::Bool(true) if intent.is_null() || intent.is_boolean() => {
            if intent.is_null() && observed.as_bool() == Some(true) {
                *intent = Value::Bool(true);
            }
            Ok(())
        }
        _ => Err("Adoption accepts only observed boolean preferences".into()),
    }
}

#[cfg(test)]
#[path = "settings_adoption_tests.rs"]
mod tests;
