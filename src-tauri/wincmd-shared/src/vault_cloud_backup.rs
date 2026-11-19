// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, provider-neutral requests and observations for mounted Vault backup.
use serde::{Deserialize, Serialize};

use crate::vault_sync::{valid_sync_mount_receipt, valid_sync_path};

/// Read-only service attestation for a caller that cannot query driver ownership.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupAttestationRequest {
    pub personal: bool,
    pub internal_drive: u8,
    pub expected_engine_mount_identity: String,
}

impl VaultCloudBackupAttestationRequest {
    pub fn valid(&self) -> bool {
        self.internal_drive <= 25
            && !self.expected_engine_mount_identity.is_empty()
            && self.expected_engine_mount_identity.len() <= 192
            && !self
                .expected_engine_mount_identity
                .chars()
                .any(char::is_control)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(rename_all = "snake_case")]
pub enum VaultCloudBackupAction {
    Prepare,
    Connect,
    Configure,
    Status,
    Start,
    Stop,
    Verify,
    Restore,
    Disconnect,
    RemoveSetup,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupConfiguration {
    pub whole_vault: bool,
    pub source_relative_paths: Vec<String>,
    pub remote_folder_name: String,
    pub automatic: bool,
}

impl VaultCloudBackupConfiguration {
    pub fn valid(&self) -> bool {
        if !valid_backup_folder_name(&self.remote_folder_name)
            || self.source_relative_paths.len() > 32
            || self.whole_vault != self.source_relative_paths.is_empty()
            || !self
                .source_relative_paths
                .iter()
                .all(|path| valid_backup_path(path))
        {
            return false;
        }
        let roots: Vec<String> = self
            .source_relative_paths
            .iter()
            .map(|path| path.replace('/', "\\").to_ascii_lowercase())
            .collect();
        !roots.iter().enumerate().any(|(index, root)| {
            roots[..index].iter().any(|other| {
                root == other
                    || root.starts_with(&format!("{other}\\"))
                    || other.starts_with(&format!("{root}\\"))
            })
        })
    }
}

pub fn valid_backup_path(path: &str) -> bool {
    valid_sync_path(path)
        && path
            .split(['\\', '/'])
            .all(|part| !part.ends_with(['.', ' ']))
        && path.split(['\\', '/']).next().is_some_and(|first| {
            ![".wincommander", "system volume information", "$recycle.bin"]
                .iter()
                .any(|excluded| first.eq_ignore_ascii_case(excluded))
        })
}

pub fn valid_backup_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.trim() == name
        && !name.ends_with('.')
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupRequest {
    pub personal: bool,
    pub internal_drive: u8,
    pub action: VaultCloudBackupAction,
    pub expected_mount_receipt: Option<String>,
    pub configuration: Option<VaultCloudBackupConfiguration>,
    pub restore_relative_path: Option<String>,
}

impl VaultCloudBackupRequest {
    pub fn valid(&self) -> bool {
        use VaultCloudBackupAction::{Configure, Restore, Status};
        self.internal_drive <= 25
            && self
                .expected_mount_receipt
                .as_deref()
                .is_none_or(valid_sync_mount_receipt)
            && (self.action == Status || self.expected_mount_receipt.is_some())
            && match self.action {
                Configure => {
                    self.configuration
                        .as_ref()
                        .is_some_and(|config| config.valid())
                        && self.restore_relative_path.is_none()
                }
                Restore => {
                    self.configuration.is_none()
                        && self
                            .restore_relative_path
                            .as_deref()
                            .is_some_and(valid_backup_path)
                }
                _ => self.configuration.is_none() && self.restore_relative_path.is_none(),
            }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(rename_all = "snake_case")]
pub enum VaultCloudBackupOutcome {
    Complete,
    Partial,
    Failed,
    Interrupted,
    Unknown,
}

#[path = "vault_cloud_backup_receipt.rs"]
mod receipt;
pub use receipt::{VaultCloudBackupConfirmation, VaultCloudBackupReceipt};

fn valid_observed_time(value: &str) -> bool {
    !value.is_empty() && value.len() <= 40 && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(rename_all = "snake_case")]
pub enum VaultCloudConnectionCheck { #[default] Unknown, Connected, SignedOut }

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(rename_all = "snake_case")]
pub enum VaultCloudRemoteCheck { #[default] Unknown, Present, Missing }

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupResult {
    #[serde(default)]
    pub connection_check: VaultCloudConnectionCheck,
    #[serde(default)]
    pub remote_check: VaultCloudRemoteCheck,
    #[serde(default)]
    pub checked_at: Option<String>,
    #[serde(default)]
    pub remote_path: Option<String>,
    pub mount_receipt: Option<String>,
    pub engine_ready: bool,
    pub connected: bool,
    pub configured: bool,
    pub automatic: bool,
    pub watching: bool,
    pub running: bool,
    pub can_configure: bool,
    pub configuration: Option<VaultCloudBackupConfiguration>,
    pub last_run: Option<VaultCloudBackupReceipt>,
    #[serde(default)]
    pub last_backup: Option<VaultCloudBackupReceipt>,
    #[serde(default)]
    pub last_verification: Option<VaultCloudBackupReceipt>,
    #[serde(default)]
    pub last_restore: Option<VaultCloudBackupReceipt>,
    pub reason_code: Option<String>,
}

impl VaultCloudBackupResult {
    pub fn valid(&self) -> bool {
        self.checked_at.as_deref().is_none_or(valid_observed_time)
            && self.remote_path.as_deref().is_none_or(|path| path.starts_with("/my-files/") && path.len() <= 512 && !path.chars().any(char::is_control))
            && self.mount_receipt
            .as_deref()
            .is_none_or(valid_sync_mount_receipt)
            && self.configured == self.configuration.is_some()
            && self
                .configuration
                .as_ref()
                .is_none_or(|config| config.valid())
            && self
                .configuration
                .as_ref()
                .is_none_or(|config| self.automatic == config.automatic)
            && self.last_run.as_ref().is_none_or(|run| run.valid())
            && self
                .last_backup
                .as_ref()
                .is_none_or(|run| run.valid() && run.action == Some(VaultCloudBackupAction::Start))
            && self
                .last_verification
                .as_ref()
                .is_none_or(|run| run.valid() && run.action == Some(VaultCloudBackupAction::Verify))
            && self.last_restore.as_ref().is_none_or(|run| {
                run.valid() && run.action == Some(VaultCloudBackupAction::Restore)
            })
            && (!self.running || (self.engine_ready && self.connected && self.configured))
            && (!self.watching
                || (self.engine_ready && self.connected && self.configured && self.automatic))
            && self.reason_code.as_deref().is_none_or(|reason| {
                reason.starts_with("cloud_backup_")
                    && reason.len() <= 80
                    && reason.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
            })
    }
}

#[cfg(test)]
#[path = "vault_cloud_backup_tests.rs"]
mod tests;
