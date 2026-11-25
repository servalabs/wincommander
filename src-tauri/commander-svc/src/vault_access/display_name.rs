// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vault display names: a policy entry's `label`, or for a personal Vault an
//! entry in its own file so `vault-personal-v1.json` never changes and older
//! versions, which reject unknown registry fields, keep reading it.

use super::*;

const PERSONAL_NAMES_FILE: &str = "vault-personal-names-v1.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersonalVaultNames {
    #[serde(default)]
    names: Vec<PersonalVaultName>,
    /// An unreadable file is never overwritten, so a rename is refused instead.
    #[serde(skip)]
    unreadable: bool,
}

/// Keyed like a personal mount: owner SID plus stable container identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersonalVaultName {
    owner_sid: String,
    container_identity: String,
    name: String,
}

impl PersonalVaultName {
    fn is_for(&self, record: &PersonalVaultRecord) -> bool {
        self.owner_sid == record.owner_sid && self.container_identity == record.container_identity
    }
}

impl PersonalVaultNames {
    pub(super) fn name_for(&self, record: &PersonalVaultRecord) -> Option<&str> {
        self.names
            .iter()
            .find(|entry| entry.is_for(record))
            .map(|entry| entry.name.as_str())
    }
}

impl VaultAccessStore {
    fn personal_names_path(&self) -> PathBuf {
        self.personal_path.with_file_name(PERSONAL_NAMES_FILE)
    }

    pub(super) fn load_personal_names_at_startup(&self) {
        let unreadable = PersonalVaultNames { names: Vec::new(), unreadable: true };
        let names = match self.fs.read(&self.personal_names_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(unreadable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersonalVaultNames::default(),
            Err(_) => unreadable,
        };
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .personal_names = names;
    }

    #[cfg(test)]
    pub(crate) fn personal_display_name_for_test(&self, record: &PersonalVaultRecord) -> Option<String> {
        self.state.lock().unwrap().personal_names.name_for(record).map(str::to_owned)
    }

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

    /// Writes only the names file, pruning names of records that no longer
    /// exist; a failed save leaves the previous names in memory and on disk.
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
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if !state.personal_registry_healthy
            || state.personal_names.unreadable
            || state.personal.get(&personal_key(&record.container_path)) != Some(&record)
        {
            return Err(VaultError::Persistence);
        }
        let mut names: Vec<PersonalVaultName> = state
            .personal_names
            .names
            .iter()
            .filter(|entry| !entry.is_for(&record) && state.personal.values().any(|live| entry.is_for(live)))
            .cloned()
            .collect();
        names.push(PersonalVaultName {
            owner_sid: record.owner_sid.clone(),
            container_identity: record.container_identity.clone(),
            name: name.to_owned(),
        });
        let updated = PersonalVaultNames { names, unreadable: false };
        let bytes = serde_json::to_vec(&updated).map_err(|_| VaultError::Persistence)?;
        self.fs
            .atomic_write(&self.personal_names_path(), &bytes)
            .map_err(|_| VaultError::Persistence)?;
        state.personal_names = updated;
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
                let name = state.personal_names.name_for(record)?;
                Some((record.container_path.clone(), name.to_owned()))
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
