// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded management receipts for explicitly selected personal Vault folders.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(rename_all = "snake_case")]
pub enum VaultSyncAction {
    List,
    Rename,
    Remove,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VaultSyncManagementRequest {
    pub personal: bool,
    pub internal_drive: u8,
    pub action: VaultSyncAction,
    pub relative_path: Option<String>,
    pub folder_label: Option<String>,
    pub folder_id: Option<String>,
    pub expected_mount_receipt: Option<String>,
}

impl VaultSyncManagementRequest {
    pub fn valid(&self) -> bool {
        self.personal
            && self.internal_drive <= 25
            && match self.action {
                VaultSyncAction::List => {
                    self.relative_path.is_none()
                        && self.folder_label.is_none()
                        && self.folder_id.is_none()
                        && self.expected_mount_receipt.is_none()
                }
                VaultSyncAction::Rename => {
                    self.relative_path.as_deref().is_some_and(valid_sync_path)
                        && self
                            .folder_label
                            .as_deref()
                            .is_none_or(|label| label.trim().is_empty() || valid_sync_label(label))
                        && self.folder_id.as_deref().is_some_and(valid_sync_folder_id)
                        && self
                            .expected_mount_receipt
                            .as_deref()
                            .is_some_and(valid_sync_mount_receipt)
                }
                VaultSyncAction::Remove => {
                    self.relative_path.as_deref().is_some_and(valid_sync_path)
                        && self.folder_label.is_none()
                        && self.folder_id.as_deref().is_some_and(valid_sync_folder_id)
                        && self
                            .expected_mount_receipt
                            .as_deref()
                            .is_some_and(valid_sync_mount_receipt)
                }
            }
    }
}

pub fn valid_sync_label(label: &str) -> bool {
    !label.trim().is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
}

pub fn valid_sync_folder_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

pub fn valid_sync_mount_receipt(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

pub fn valid_sync_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && path.split(['\\', '/']).all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part
                    .chars()
                    .any(|c| c.is_control() || ":*?\"<>|".contains(c))
        })
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultSyncFolder {
    pub folder_id: String,
    pub relative_path: String,
    pub label: String,
    pub paused: bool,
    pub recovery_required: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultSyncManagementResult {
    pub managed: bool,
    #[serde(default)]
    pub mount_receipt: Option<String>,
    pub gui_url: Option<String>,
    pub folders: Vec<VaultSyncFolder>,
    pub removed: bool,
}

impl VaultSyncManagementResult {
    pub fn valid(&self) -> bool {
        let mut roots = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        self.folders.len() <= 32
            && self.mount_receipt.as_deref().is_none_or(|value| {
                value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
            })
            && self.managed == !self.folders.is_empty()
            && self.folders.iter().all(|folder| {
                !folder.folder_id.is_empty()
                    && folder.folder_id.len() <= 64
                    && folder
                        .folder_id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                    && valid_sync_path(&folder.relative_path)
                    && valid_sync_label(&folder.label)
                    && ids.insert(folder.folder_id.clone())
                    && roots.insert(folder.relative_path.replace('/', "\\").to_ascii_lowercase())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unbounded_names_and_ambiguous_actions() {
        for label in ["", "   ", "Camera\nPhone", &"x".repeat(129)] {
            assert!(!valid_sync_label(label));
        }
        assert!(valid_sync_label("Phone photos"));
        for path in [
            "../Other",
            "V:\\Camera",
            "Phone\\\\Camera",
            "Phone\\..",
            "Phone:Other",
        ] {
            assert!(!valid_sync_path(path));
        }
        let mut request = VaultSyncManagementRequest {
            personal: true,
            internal_drive: 8,
            action: VaultSyncAction::List,
            relative_path: None,
            folder_label: None,
            folder_id: None,
            expected_mount_receipt: None,
        };
        assert!(request.valid());
        request.relative_path = Some("Phone\\Camera".into());
        assert!(!request.valid());
        request.action = VaultSyncAction::Rename;
        assert!(!request.valid());
        request.folder_label = Some("Phone photos".into());
        request.folder_id = Some("wcv-test".into());
        request.expected_mount_receipt = Some("a".repeat(64));
        assert!(request.valid());
        request.action = VaultSyncAction::Remove;
        assert!(!request.valid());
    }
}
