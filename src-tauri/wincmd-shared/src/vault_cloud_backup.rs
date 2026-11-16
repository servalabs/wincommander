// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, provider-neutral requests and observations for mounted Vault backup.
use serde::{Deserialize, Serialize};

use crate::vault_sync::{valid_sync_folder_id, valid_sync_mount_receipt, valid_sync_path};

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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupReceipt {
    pub run_id: String,
    pub outcome: VaultCloudBackupOutcome,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub transferred_items: u32,
    pub transferred_bytes: String,
    pub skipped_items: u32,
    pub failed_items: u32,
}

impl VaultCloudBackupReceipt {
    pub fn valid(&self) -> bool {
        valid_sync_folder_id(&self.run_id)
            && valid_observed_time(&self.started_at)
            && self.finished_at.as_deref().is_none_or(valid_observed_time)
            && self
                .transferred_bytes
                .parse::<u64>()
                .is_ok_and(|value| value.to_string() == self.transferred_bytes)
            && (self.outcome != VaultCloudBackupOutcome::Complete || self.failed_items == 0)
    }
}

fn valid_observed_time(value: &str) -> bool {
    !value.is_empty() && value.len() <= 40 && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupResult {
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
    pub reason_code: Option<String>,
}

impl VaultCloudBackupResult {
    pub fn valid(&self) -> bool {
        self.mount_receipt
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
