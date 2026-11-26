// SPDX-License-Identifier: AGPL-3.0-or-later
//! After a Vault file is renamed in its folder, every service record naming
//! the old path moves to the new one. Records are matched by file identity,
//! which a same-folder rename keeps, so no ACL or grant is re-applied.

use super::*;

/// `old_path` with its file name replaced, keeping the record's own spelling of the folder.
pub(super) fn renamed_path(old_path: &str, file_name: &str) -> String {
    Path::new(old_path).with_file_name(file_name).to_string_lossy().into_owned()
}

impl VaultAccessStore {
    /// All or nothing: a failed save rewrites the files already saved and
    /// leaves memory unchanged, so the caller can rename the file back.
    pub(crate) fn move_records_for_rename(
        &self,
        target: &PasswordChangeTarget,
        file_name: &str,
    ) -> Result<(), VaultError> {
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if !state.personal_registry_healthy {
            return Err(VaultError::Persistence);
        }
        let identity = target.container_identity.as_str();
        let policy = match &target.entry_id {
            Some(entry_id) => Some(moved_policy(&state, entry_id, identity, file_name)?),
            None => None,
        };
        let personal = moved_personal(&state, identity, file_name)?;
        let names = state.personal_names.moved(identity, file_name);
        let notes = state.hidden_parts.moved(identity, file_name)?;

        let mut writes: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        if let Some(policy) = &policy {
            writes.push((self.path.clone(), encode(policy)?));
        }
        if let Some(records) = &personal {
            let bytes = personal_registry_bytes(records, &state.personal_pending, &state.legacy_recoveries)?;
            writes.push((self.personal_path.clone(), bytes));
        }
        if let Some(names) = &names {
            writes.push((self.personal_names_path(), encode(names)?));
        }
        if let Some(notes) = &notes {
            writes.push((self.hidden_parts_path(), encode(notes)?));
        }
        let previous = writes
            .iter()
            .map(|(path, _)| match self.fs.read(path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(VaultError::Persistence),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (index, (path, bytes)) in writes.iter().enumerate() {
            if self.fs.atomic_write(path, bytes).is_err() {
                for ((path, _), old) in writes[..index].iter().zip(&previous).rev() {
                    let _ = match old {
                        Some(old) => self.fs.atomic_write(path, old),
                        None => self.fs.remove(path),
                    };
                }
                return Err(VaultError::Persistence);
            }
        }
        if let Some(policy) = policy {
            state.status.version = policy.policy.version;
            state.active = Some(policy);
        }
        if let Some(records) = personal {
            state.personal = records;
        }
        if let Some(names) = names {
            state.personal_names = names;
        }
        if let Some(notes) = notes {
            state.hidden_parts = notes;
        }
        Ok(())
    }
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, VaultError> {
    serde_json::to_vec(value).map_err(|_| VaultError::Persistence)
}

/// Only the path moves; the version still moves so a stale policy draft cannot silently undo it.
fn moved_policy(
    state: &State,
    entry_id: &str,
    identity: &str,
    file_name: &str,
) -> Result<PersistedPolicy, VaultError> {
    if state.status.validation_state != VaultValidationState::Current || state.rollback_recovery.is_some() {
        return Err(VaultError::Persistence);
    }
    let active = state.active.as_ref().ok_or(VaultError::Validation)?;
    if !active.resolved.iter().any(|resolved| resolved.id == entry_id && resolved.identity == identity) {
        return Err(VaultError::ContainerIdentity);
    }
    let mut replacement = active.clone();
    let entry = replacement
        .policy
        .entries
        .iter_mut()
        .find(|entry| entry.id == entry_id)
        .ok_or(VaultError::Validation)?;
    entry.container_path = renamed_path(&entry.container_path, file_name);
    replacement.policy.expected_previous_version = active.policy.version;
    replacement.policy.version = active.policy.version.checked_add(1).ok_or(VaultError::Validation)?;
    validate_policy(&replacement.policy)?;
    Ok(replacement)
}

/// The registry with `identity`'s record at its new path; a hard-linked second record fails closed.
fn moved_personal(
    state: &State,
    identity: &str,
    file_name: &str,
) -> Result<Option<HashMap<String, PersonalVaultRecord>>, VaultError> {
    let mut matching = state
        .personal
        .iter()
        .filter(|(_, record)| record.container_identity == identity);
    let Some((key, record)) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err(VaultError::Persistence);
    }
    let mut records = state.personal.clone();
    records.remove(key);
    let mut moved = record.clone();
    moved.container_path = renamed_path(&record.container_path, file_name);
    records.insert(personal_key(&moved.container_path), moved);
    Ok(Some(records))
}
