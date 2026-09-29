// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::{json, Value};
use wincmd_shared::personal_settings::{PersonalSettingsRecord, READ_PERSONAL_SETTINGS_VERB};

const MIGRATION_MARKER: &str = "personal-settings-service.migrated";

pub(super) fn service_call(verb: &'static str, args: Value) -> Result<Value, String> {
    // Settings reads also run inside async jobs; don't nest a runtime's block_on.
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Personal settings transport could not start".to_string())?
            .block_on(crate::svc_client::call(verb, args))
    })
    .join()
    .map_err(|_| "Personal settings transport failed".to_string())?
}

pub(super) fn read_service() -> Result<PersonalSettingsRecord, String> {
    serde_json::from_value(service_call(READ_PERSONAL_SETTINGS_VERB, json!({}))?)
        .map_err(|_| "Invalid personal settings service response".to_string())
}

pub(super) fn has_migration_marker() -> Result<bool, String> {
    match std::fs::symlink_metadata(crate::paths::user_data_dir()?.join(MIGRATION_MARKER)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Could not inspect personal settings migration state".to_string()),
    }
}

pub(super) fn mark_migrated() -> Result<(), String> {
    crate::datastore_io::atomic_write(
        &crate::paths::user_data_dir()?.join(MIGRATION_MARKER),
        b"service-v1\n",
    )
}

pub(super) fn service_unavailable(error: &str) -> bool {
    // Authentication, integrity and denied records must never look like an absent service.
    error.starts_with("service connect failed:")
        || error == "service connection failed"
        || error == "Vault access service is available only on Windows"
        || error == "service reply timed out"
        || error == "service Hello acknowledgement timed out"
        || error.starts_with("service rejected request: unknown_verb ")
}
