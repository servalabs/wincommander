// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vault display names: a personal record's `display_name` or a policy entry's `label`.

use super::*;

impl VaultAccessStore {
    /// Resolves what a rename may change. `None` is an unregistered container:
    /// WinCommander holds no record that could carry its name.
    pub fn rename_target(
        &self,
        container_path: &str,
        caller_sid: &str,
        caller_session: u32,
    ) -> Result<Option<PasswordChangeTarget>, VaultError> {
        let target = self.password_change_target(container_path, caller_sid, caller_session)?;
        if target.entry_id.is_some() {
            return Ok(Some(target));
        }
        if wincmd_shared::vault_access::is_supported_vault_device_path(&target.container_path) {
            return Ok(None);
        }
        Ok(self
            .personal_record_for_path(&target.container_path)?
            .map(|_| target))
    }

    /// A failed save leaves the in-memory record exactly as it was.
    pub(crate) fn rename_personal_vault(
        &self,
        target: &PasswordChangeTarget,
        owner_sid: &str,
        name: &str,
    ) -> Result<(), VaultError> {
        let record = self
            .personal_record_for_path(&target.container_path)?
            .ok_or(VaultError::Validation)?;
        if record.owner_sid != owner_sid {
            return Err(VaultError::Forbidden);
        }
        if record.container_identity != target.container_identity {
            return Err(VaultError::ContainerIdentity);
        }
        let key = personal_key(&record.container_path);
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if !state.personal_registry_healthy {
            return Err(VaultError::Persistence);
        }
        let original = state
            .personal
            .get(&key)
            .filter(|current| **current == record)
            .cloned()
            .ok_or(VaultError::Persistence)?;
        let mut updated = original.clone();
        updated.display_name = Some(name.to_owned());
        state.personal.insert(key.clone(), updated);
        if self
            .persist_personal(&state.personal, &state.personal_pending, &state.legacy_recoveries)
            .is_err()
        {
            state.personal.insert(key, original);
            return Err(VaultError::Persistence);
        }
        Ok(())
    }

    /// Only the label changes, so no ACL, group or grant is re-applied. The
    /// version still moves so a stale policy draft cannot silently undo it.
    pub(crate) fn rename_policy_entry(&self, entry_id: &str, label: &str) -> Result<(), VaultError> {
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if state.status.validation_state != VaultValidationState::Current
            || state.rollback_recovery.is_some()
        {
            return Err(VaultError::Persistence);
        }
        let active = state.active.clone().ok_or(VaultError::Validation)?;
        let mut replacement = active.clone();
        replacement
            .policy
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
            .ok_or(VaultError::Validation)?
            .label = label.to_owned();
        replacement.policy.expected_previous_version = active.policy.version;
        replacement.policy.version = active
            .policy
            .version
            .checked_add(1)
            .ok_or(VaultError::Validation)?;
        validate_policy(&replacement.policy)?;
        let bytes = serde_json::to_vec(&replacement).map_err(|_| VaultError::Persistence)?;
        self.fs
            .atomic_write(&self.path, &bytes)
            .map_err(|_| VaultError::Persistence)?;
        state.status.version = replacement.policy.version;
        state.active = Some(replacement);
        Ok(())
    }

    /// The caller's own named personal Vaults plus the policy Vaults it owns or
    /// `may_rename` allows, as (container path, name) pairs.
    pub(crate) fn display_names_for_caller(
        &self,
        caller_sid: &str,
        may_rename: impl Fn(&str) -> bool,
    ) -> Vec<(String, String)> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        let mut names: Vec<(String, String)> = state
            .personal
            .values()
            .filter(|record| state.personal_registry_healthy && record.owner_sid == caller_sid)
            .filter_map(|record| {
                let name = record.display_name.clone()?;
                Some((record.container_path.clone(), name))
            })
            .collect();
        let entries: Vec<(String, String, String, bool)> = state
            .active
            .as_ref()
            .filter(|_| state.status.validation_state == VaultValidationState::Current)
            .map(|active| {
                active
                    .policy
                    .entries
                    .iter()
                    .map(|entry| {
                        (
                            entry.id.clone(),
                            entry.container_path.clone(),
                            entry.label.clone(),
                            entry.primary_owner_sid.as_deref() == Some(caller_sid),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        // The grant check takes this lock itself.
        drop(state);
        names.extend(
            entries
                .into_iter()
                .filter(|(id, _, _, owned)| *owned || may_rename(id))
                .map(|(_, path, label, _)| (path, label)),
        );
        names
    }
}
