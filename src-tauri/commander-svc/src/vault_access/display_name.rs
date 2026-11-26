// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vault display names: a policy entry's `label`, or for a personal Vault an
//! entry in its own file so `vault-personal-v1.json` never changes and older
//! versions, which reject unknown registry fields, keep reading it. A
//! container WinCommander holds no record of gets a per-caller name there too.

use super::*;

const PERSONAL_NAMES_FILE: &str = "vault-personal-names-v1.json";
/// Bounds the names file: a caller's oldest unregistered name makes room for a new one.
pub(super) const MAX_UNREGISTERED_NAMES_PER_CALLER: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersonalVaultNames {
    #[serde(default)]
    names: Vec<PersonalVaultName>,
    /// An unreadable file is never overwritten, so a rename is refused instead.
    #[serde(skip)]
    unreadable: bool,
}

/// Keyed like a personal mount: owner SID plus stable container identity.
/// For an unregistered container the "owner" is the caller who named it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersonalVaultName {
    owner_sid: String,
    container_identity: String,
    name: String,
    /// Set only for an unregistered container; a registered Vault's path comes from its record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container_path: Option<String>,
}

impl PersonalVaultName {
    fn is_for(&self, record: &PersonalVaultRecord) -> bool {
        self.container_path.is_none()
            && self.owner_sid == record.owner_sid
            && self.container_identity == record.container_identity
    }

    fn same_key(&self, other: &Self) -> bool {
        self.owner_sid == other.owner_sid
            && self.container_identity == other.container_identity
            && self.container_path.is_some() == other.container_path.is_some()
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

/// Which record a rename changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameRecord {
    Policy,
    Personal,
    /// No WinCommander record: the name is kept for the caller only.
    Unregistered,
}

/// One row of `svc.vault.display_names`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultDisplayName {
    pub path: String,
    pub name: String,
}

impl VaultDisplayName {
    fn new(path: String, name: String) -> Self {
        Self { path, name }
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

    /// Resolves what a rename may change: a policy entry, the caller's own
    /// registered personal Vault, or a container with no record at all.
    pub fn rename_target(
        &self,
        container_path: &str,
        caller_sid: &str,
        caller_session: u32,
    ) -> Result<(PasswordChangeTarget, RenameRecord), VaultError> {
        let target = self.password_change_target(container_path, caller_sid, caller_session)?;
        let record = if target.entry_id.is_some() {
            RenameRecord::Policy
        } else if !wincmd_shared::vault_access::is_supported_vault_device_path(&target.container_path)
            && self.personal_record_for_path(&target.container_path)?.is_some()
        {
            RenameRecord::Personal
        } else {
            RenameRecord::Unregistered
        };
        Ok((target, record))
    }

    /// Writes only the names file, pruning names of records that no longer
    /// exist; a failed save leaves the previous names in memory and on disk.
    /// `target` was resolved in the caller's token, so the record is found by
    /// identity: the service itself may not be able to open the container's folder.
    pub(crate) fn rename_personal_vault(
        &self,
        target: &PasswordChangeTarget,
        owner_sid: &str,
        name: &str,
    ) -> Result<(), VaultError> {
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        let record = single_record_for(&state, target)?.ok_or(VaultError::Validation)?;
        if record.owner_sid != owner_sid {
            return Err(VaultError::Forbidden);
        }
        self.save_name(
            &mut state,
            PersonalVaultName {
                owner_sid: record.owner_sid.clone(),
                container_identity: record.container_identity.clone(),
                name: name.to_owned(),
                container_path: None,
            },
        )
    }

    /// Names a container WinCommander holds no record of, for `caller_sid`
    /// only. The caller's write access to it is proven before this is called.
    pub(crate) fn rename_unregistered_container(
        &self,
        target: &PasswordChangeTarget,
        caller_sid: &str,
        name: &str,
    ) -> Result<(), VaultError> {
        if target.entry_id.is_some() || caller_sid.is_empty() {
            return Err(VaultError::Validation);
        }
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if single_record_for(&state, target)?.is_some() {
            return Err(VaultError::ContainerIdentity);
        }
        self.save_name(
            &mut state,
            PersonalVaultName {
                owner_sid: caller_sid.to_owned(),
                container_identity: target.container_identity.clone(),
                name: name.to_owned(),
                container_path: Some(target.container_path.clone()),
            },
        )
    }

    fn save_name(&self, state: &mut State, entry: PersonalVaultName) -> Result<(), VaultError> {
        if !state.personal_registry_healthy || state.personal_names.unreadable {
            return Err(VaultError::Persistence);
        }
        let mut names: Vec<PersonalVaultName> = state
            .personal_names
            .names
            .iter()
            .filter(|kept| !kept.same_key(&entry))
            .filter(|kept| kept.container_path.is_some() || state.personal.values().any(|live| kept.is_for(live)))
            .cloned()
            .collect();
        if entry.container_path.is_some() {
            let callers = |kept: &PersonalVaultName| kept.container_path.is_some() && kept.owner_sid == entry.owner_sid;
            while names.iter().filter(|kept| callers(kept)).count() >= MAX_UNREGISTERED_NAMES_PER_CALLER {
                if let Some(oldest) = names.iter().position(callers) {
                    names.remove(oldest);
                }
            }
        }
        names.push(entry);
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

    /// The policy Vaults the caller owns or `may_rename` allows, its own named
    /// personal Vaults, then the unregistered containers it named.
    pub(crate) fn display_names_for_caller(
        &self,
        caller_sid: &str,
        may_rename: impl Fn(&str) -> bool,
    ) -> Vec<VaultDisplayName> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        let mut personal: Vec<VaultDisplayName> = state
            .personal
            .values()
            .filter(|record| state.personal_registry_healthy && record.owner_sid == caller_sid)
            .filter_map(|record| {
                let name = state.personal_names.name_for(record)?;
                Some(VaultDisplayName::new(record.container_path.clone(), name.to_owned()))
            })
            .collect();
        personal.extend(
            state
                .personal_names
                .names
                .iter()
                .filter(|entry| entry.owner_sid == caller_sid)
                .filter_map(|entry| {
                    let path = entry.container_path.clone()?;
                    Some(VaultDisplayName::new(path, entry.name.clone()))
                }),
        );
        let entries: Vec<(String, VaultDisplayName, bool)> = state
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
                            VaultDisplayName::new(entry.container_path.clone(), entry.label.clone()),
                            entry.primary_owner_sid.as_deref() == Some(caller_sid),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        // The grant check takes this lock itself.
        drop(state);
        // A policy name wins over a caller's own name for the same path.
        let mut names: Vec<VaultDisplayName> = entries
            .into_iter()
            .filter(|(id, _, owned)| *owned || may_rename(id))
            .map(|(_, row, _)| row)
            .collect();
        names.extend(personal);
        names
    }
}

/// The one personal record of `target`'s file; a hard-linked second record fails closed.
fn single_record_for(state: &State, target: &PasswordChangeTarget) -> Result<Option<PersonalVaultRecord>, VaultError> {
    if !state.personal_registry_healthy {
        return Err(VaultError::Persistence);
    }
    let mut records = state
        .personal
        .values()
        .filter(|record| record.container_identity == target.container_identity);
    let record = records.next().cloned();
    if records.next().is_some() {
        return Err(VaultError::Persistence);
    }
    Ok(record)
}