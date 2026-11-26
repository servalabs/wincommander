// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vault display names saved by earlier versions: a policy entry's `label`, or
//! for a personal Vault an entry in its own file so `vault-personal-v1.json`
//! never changes. A rename now renames the file, so nothing adds a name; an
//! old name of a container with no record follows its file to the new path.

use super::*;

const PERSONAL_NAMES_FILE: &str = "vault-personal-names-v1.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersonalVaultNames {
    #[serde(default)]
    names: Vec<PersonalVaultName>,
    /// An unreadable file is never overwritten.
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
}

impl PersonalVaultNames {
    pub(super) fn name_for(&self, record: &PersonalVaultRecord) -> Option<&str> {
        self.names
            .iter()
            .find(|entry| entry.is_for(record))
            .map(|entry| entry.name.as_str())
    }

    /// The names with every unregistered entry for `identity` renamed to
    /// `file_name`; `None` when nothing changes or the file was unreadable.
    pub(super) fn moved(&self, identity: &str, file_name: &str) -> Option<Self> {
        if self.unreadable {
            return None;
        }
        let mut moved = self.clone();
        let mut changed = false;
        for entry in &mut moved.names {
            if entry.container_identity == identity {
                if let Some(path) = &mut entry.container_path {
                    *path = rename_records::renamed_path(path, file_name);
                    changed = true;
                }
            }
        }
        changed.then_some(moved)
    }
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
    pub(super) fn personal_names_path(&self) -> PathBuf {
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
