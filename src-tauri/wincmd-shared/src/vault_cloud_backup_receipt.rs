// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{
    valid_backup_folder_name, valid_backup_path, valid_observed_time, VaultCloudBackupAction,
    VaultCloudBackupOutcome,
};
use crate::vault_sync::valid_sync_folder_id;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(rename_all = "snake_case")]
pub enum VaultCloudBackupConfirmation {
    RemoteSampleMatched,
    RestoredSampleMatched,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct VaultCloudBackupReceipt {
    #[serde(default)]
    pub action: Option<VaultCloudBackupAction>,
    #[serde(default)]
    pub remote_folder_name: Option<String>,
    #[serde(default)]
    pub restore_relative_path: Option<String>,
    #[serde(default)]
    pub confirmation: Option<VaultCloudBackupConfirmation>,
    #[serde(default)]
    pub checked_files: u32,
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
            && self.action.is_none_or(|action| {
                matches!(
                    action,
                    VaultCloudBackupAction::Start
                        | VaultCloudBackupAction::Verify
                        | VaultCloudBackupAction::Restore
                )
            })
            && (self.action.is_none()
                || self.outcome != VaultCloudBackupOutcome::Complete
                || self.finished_at.is_some())
            && self
                .remote_folder_name
                .as_deref()
                .is_none_or(valid_backup_folder_name)
            && self.restore_relative_path.as_deref().is_none_or(|path| {
                self.action == Some(VaultCloudBackupAction::Restore) && valid_backup_path(path)
            })
            && match self.confirmation {
                None => self.checked_files == 0,
                Some(confirmation) => {
                    self.outcome == VaultCloudBackupOutcome::Complete
                        && self.finished_at.is_some()
                        && self.checked_files > 0
                        && self.remote_folder_name.is_some()
                        && match confirmation {
                            VaultCloudBackupConfirmation::RemoteSampleMatched => matches!(
                                self.action,
                                Some(
                                    VaultCloudBackupAction::Start | VaultCloudBackupAction::Verify
                                )
                            ),
                            VaultCloudBackupConfirmation::RestoredSampleMatched => {
                                self.action == Some(VaultCloudBackupAction::Restore)
                                    && self.restore_relative_path.is_some()
                            }
                        }
                }
            }
    }
}
