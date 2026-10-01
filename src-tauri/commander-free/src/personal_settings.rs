// SPDX-License-Identifier: AGPL-3.0-or-later
//! Password-independent preferences; legacy personal secrets retain their own protection.
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Mutex;
use wincmd_shared::personal_settings::{
    PersonalSettingsRecord, WritePersonalSettingsRequest, WRITE_PERSONAL_SETTINGS_VERB,
};

#[path = "personal_settings_secrets.rs"]
mod secrets;
#[path = "personal_settings_transport.rs"]
mod transport;
use transport::*;
#[cfg(test)]
#[path = "personal_settings_atomic_tests.rs"]
mod atomic_tests;
#[cfg(test)]
#[path = "personal_settings_tests.rs"]
mod tests;

const SECRET_ENVELOPE: &str = "_personalSecrets";
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
const UNAVAILABLE_STATUS: Status = Status {
    mode: Mode::Temporary,
    recovery_required: false,
    can_save: false,
};
static STATUS: Mutex<Status> = Mutex::new(UNAVAILABLE_STATUS);

#[derive(Clone, Copy, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Status {
    mode: Mode,
    recovery_required: bool,
    can_save: bool,
}

#[derive(Clone, Copy, Serialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Service,
    Legacy,
    Temporary,
}

struct Session {
    mode: Mode,
    revision: u64,
    legacy_overlay_pending_migration: bool,
    legacy_recovery_required: bool,
    secrets_locked: bool,
    secrets: Value,
    protected_secrets: Option<String>,
    safe_defaults: bool,
}

impl Session {
    fn status(&self) -> Status {
        Status {
            mode: self.mode,
            recovery_required: self.legacy_recovery_required || self.secrets_locked,
            can_save: self.mode != Mode::Temporary,
        }
    }
}

pub(super) fn status() -> Status {
    STATUS
        .lock()
        .map(|status| *status)
        .unwrap_or(UNAVAILABLE_STATUS)
}

fn publish_status(status: Status) {
    if let Ok(mut snapshot) = STATUS.lock() {
        *snapshot = status;
    }
}

pub(super) fn automation_available() -> bool {
    let status = status();
    status.can_save && !status.recovery_required
}

pub(super) struct Loaded {
    pub value: Option<Value>,
    pub safe_defaults: bool,
    pub service_backed: bool,
    /// A readable legacy per-user overlay was loaded while the service has no
    /// record yet. Persist it once so later launches do not depend on the
    /// legacy file.
    pub migration_persistence_needed: bool,
}

pub(super) fn is_conflict(error: &str) -> bool {
    error.contains("service rejected request: personal_settings_conflict ")
}

fn key_unavailable(error: &str) -> bool {
    error.starts_with("SETTINGS_KEY_UNAVAILABLE:")
        || error.starts_with("Settings key is missing but encrypted data exists;")
}

pub(super) fn load() -> Result<Loaded, String> {
    publish_status(UNAVAILABLE_STATUS);
    let mut session = SESSION
        .lock()
        .map_err(|_| "Personal settings lock failed".to_string())?;
    let (state, value) = load_with(
        read_service(),
        has_migration_marker()?,
        super::load_legacy_user_settings_overlay,
        secrets::load,
    )?;
    if state.mode == Mode::Service && state.revision > 0 && !super::is_decoy_mode() {
        mark_migrated()?;
    }
    let loaded = Loaded {
        value,
        safe_defaults: state.safe_defaults,
        service_backed: state.mode == Mode::Service && state.revision > 0,
        migration_persistence_needed: state.legacy_overlay_pending_migration,
    };
    publish_status(state.status());
    *session = Some(state);
    Ok(loaded)
}

fn load_with(
    service: Result<PersonalSettingsRecord, String>,
    migrated: bool,
    legacy: impl FnOnce() -> Result<Option<Value>, String>,
    secret_reader: impl FnOnce() -> Result<Value, String>,
) -> Result<(Session, Option<Value>), String> {
    load_with_open(service, migrated, legacy, secret_reader, secrets::open)
}

fn load_with_open(
    service: Result<PersonalSettingsRecord, String>,
    migrated: bool,
    legacy: impl FnOnce() -> Result<Option<Value>, String>,
    secret_reader: impl FnOnce() -> Result<Value, String>,
    open: impl FnOnce(&str) -> Result<Value, String>,
) -> Result<(Session, Option<Value>), String> {
    let record = match service {
        Ok(record) => Some(record),
        Err(error) if service_unavailable(&error) => None,
        Err(error) => return Err(error),
    };
    if migrated && record.as_ref().is_some_and(|record| record.value.is_none()) {
        return Err("Personal settings service data is missing; original data preserved".into());
    }
    let mut state = Session {
        mode: if record.is_some() {
            Mode::Service
        } else {
            Mode::Legacy
        },
        revision: record.as_ref().map_or(0, |record| record.revision),
        legacy_overlay_pending_migration: false,
        legacy_recovery_required: record
            .as_ref()
            .is_some_and(|record| record.legacy_recovery_required),
        secrets_locked: false,
        secrets: json!({}),
        protected_secrets: None,
        safe_defaults: false,
    };
    if let Some(mut value) = record.and_then(|record| record.value) {
        let encrypted = value
            .as_object_mut()
            .and_then(|object| object.remove(SECRET_ENVELOPE));
        let secret_result = match encrypted {
            Some(Value::String(envelope)) => {
                let result = open(&envelope);
                state.protected_secrets = Some(envelope);
                result
            }
            Some(Value::Null) => Ok(json!({})),
            None => secret_reader(),
            _ => return Err("Invalid personal secrets envelope".into()),
        };
        // Neither arbitrary pipe clients nor a stale overlay may replace machine policy.
        let (mut ordinary, _) = secrets::split(super::split_settings_value(value)?.1);
        match secret_result {
            Ok(secret) => {
                state.secrets = secret;
                secrets::merge(&mut ordinary, &state.secrets);
            }
            Err(error) if key_unavailable(&error) => state.secrets_locked = true,
            Err(error) => return Err(error),
        }
        // Restoring Windows key access unlocks protected features, without overwriting newer preferences.
        if state.legacy_recovery_required && !state.secrets_locked {
            match legacy() {
                Ok(Some(old)) => {
                    state.legacy_recovery_required = false;
                    if state.protected_secrets.is_none() {
                        state.secrets = secrets::split(super::split_settings_value(old)?.1).1;
                        secrets::merge(&mut ordinary, &state.secrets);
                    }
                }
                Ok(None) => state.secrets_locked = true,
                Err(error) if key_unavailable(&error) => state.secrets_locked = true,
                Err(error) => return Err(error),
            }
        }
        return Ok((state, Some(ordinary)));
    }
    if migrated {
        state.mode = Mode::Temporary;
        state.safe_defaults = true;
        return Ok((state, None));
    }
    let value = match legacy() {
        Ok(value) => value,
        Err(error) if key_unavailable(&error) => {
            state.legacy_recovery_required = true;
            state.secrets_locked = true;
            state.safe_defaults = true;
            if state.mode != Mode::Service {
                state.mode = Mode::Temporary;
            }
            None
        }
        Err(error) => return Err(error),
    };
    let value = value
        .map(|value| super::split_settings_value(value).map(|(_, user)| user))
        .transpose()?;
    // The service was reachable but had no record.  Carry this marker to the
    // caller so it performs exactly one migration write; do not turn normal
    // settings reads into writes after that record exists.
    if value.is_some() && state.mode == Mode::Service {
        state.legacy_overlay_pending_migration = true;
    }
    if let Some(value) = &value {
        state.secrets = secrets::split(value.clone()).1;
    }
    Ok((state, value))
}

pub(super) fn save(value: &Value) -> Result<bool, String> {
    let result = save_inner(value);
    if result.is_err() {
        publish_status(UNAVAILABLE_STATUS);
    }
    result
}

fn save_inner(value: &Value) -> Result<bool, String> {
    let mut guard = SESSION
        .lock()
        .map_err(|_| "Personal settings lock failed".to_string())?;
    let state = guard
        .as_mut()
        .ok_or("Personal settings have not been loaded")?;
    if state.mode == Mode::Temporary {
        return Err("Personal settings are temporary; restore or update the WinCommander service before saving".into());
    }
    if state.mode == Mode::Legacy {
        let bytes = serde_json::to_vec(value).map_err(|_| "Could not encode personal settings")?;
        return crate::datastore::save_user_blob(
            super::USER_SETTINGS_FILENAME,
            &bytes,
            super::USER_SETTINGS_MAX_PLAINTEXT_BYTES,
        )
        .map(|_| false);
    }
    save_service_with(state, value, secrets::seal, |request| {
        serde_json::from_value(service_call(
            WRITE_PERSONAL_SETTINGS_VERB,
            serde_json::to_value(request).map_err(|_| "Could not encode personal settings")?,
        )?)
        .map_err(|_| "Invalid personal settings service response".to_string())
    })?;
    mark_migrated()?;
    Ok(true)
}

fn save_service_with(
    state: &mut Session,
    value: &Value,
    seal: impl FnOnce(&Value, bool) -> Result<String, String>,
    write: impl FnOnce(WritePersonalSettingsRequest) -> Result<PersonalSettingsRecord, String>,
) -> Result<(), String> {
    let (mut ordinary, secret) = secrets::split(value.clone());
    let secrets_changed = secret != state.secrets;
    if secrets_changed && state.secrets_locked {
        return Err("Personal secrets are locked; restore the original Windows encryption access before changing them".into());
    }
    // Ciphertext and preferences commit in ONE CAS; a losing writer changes neither.
    let protected_secrets = if state.secrets_locked {
        state.protected_secrets.clone()
    } else if secret == json!({}) && !(secrets_changed && state.protected_secrets.is_some()) {
        None
    } else if secrets_changed || state.protected_secrets.is_none() {
        Some(seal(&secret, state.protected_secrets.is_some())?)
    } else {
        state.protected_secrets.clone()
    };
    ordinary[SECRET_ENVELOPE] = serde_json::to_value(&protected_secrets)
        .map_err(|_| "Could not encode personal secrets envelope")?;
    let request = WritePersonalSettingsRequest {
        expected_revision: state.revision,
        value: ordinary,
        legacy_recovery_required: state.legacy_recovery_required,
    };
    let record = write(request)?;
    state.revision = record.revision;
    state.secrets = secret;
    state.protected_secrets = protected_secrets;
    // The persisted marker records history; a successful legacy decrypt in this session resolves it.
    Ok(())
}
