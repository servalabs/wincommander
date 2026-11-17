// SPDX-License-Identifier: AGPL-3.0-or-later
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::path::Path;
use wincmd_shared::encvol_driver_activation::{
    ActivationRecord, ActivationStatus, ACTIVATION_LOCK_PATH, ACTIVATION_RECORD_PATH,
    MAX_RECORD_BYTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::EnsureDriverError;
use crate::personal_settings::platform;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RunningAction {
    RecordPending,
    RestartRequired,
    ConfirmActive,
}

pub(super) fn running_action(
    record: Option<&ActivationRecord>,
    hash: &str,
    boot: &str,
) -> RunningAction {
    match record {
        None => RunningAction::RecordPending,
        Some(record) if !record.sha256.eq_ignore_ascii_case(hash) => RunningAction::RecordPending,
        Some(record) if record.is_current_boot(boot) => match record.status {
            ActivationStatus::PendingReboot => RunningAction::RestartRequired,
            ActivationStatus::Active => RunningAction::ConfirmActive,
        },
        Some(record) if record.confirms_later_boot(boot) => RunningAction::ConfirmActive,
        Some(_) => RunningAction::RecordPending,
    }
}

pub(super) struct ActivationStore {
    _parents: Vec<File>,
    _lock: File,
    pub boot_id: String,
}

impl ActivationStore {
    pub fn open() -> Result<Self, EnsureDriverError> {
        let unavailable = || EnsureDriverError::ActivationState;
        let root = Path::new(ACTIVATION_RECORD_PATH)
            .parent()
            .ok_or_else(unavailable)?;
        let mut parents = Vec::new();
        for path in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let file = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | 0x0002_0000)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
                .map_err(|_| unavailable())?;
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // Handles without DELETE sharing pin the checked ancestors through the transaction.
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
                || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            {
                return Err(unavailable());
            }
            if path.starts_with(r"C:\ProgramData\WinCommander") {
                crate::peer_auth_deployment_acl::verify_file_handle(&file)
                    .map_err(|_| unavailable())?;
            }
            parents.push(file);
        }
        let lock_path = Path::new(ACTIVATION_LOCK_PATH);
        let lock = match platform::open_record(lock_path, false) {
            Ok(lock) => lock,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match platform::open_record(lock_path, true) {
                    Ok(lock) => lock,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        platform::open_record(lock_path, false).map_err(|_| unavailable())?
                    }
                    Err(_) => return Err(unavailable()),
                }
            }
            Err(_) => return Err(unavailable()),
        };
        Ok(Self {
            _parents: parents,
            _lock: lock,
            boot_id: current_boot_id()?,
        })
    }

    pub fn read(&self) -> Result<Option<ActivationRecord>, EnsureDriverError> {
        let file = match platform::open_record(Path::new(ACTIVATION_RECORD_PATH), false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(EnsureDriverError::ActivationState),
        };
        let mut bytes = Vec::new();
        file.take((MAX_RECORD_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| EnsureDriverError::ActivationState)?;
        ActivationRecord::parse(&bytes)
            .map(Some)
            .map_err(|_| EnsureDriverError::ActivationState)
    }

    pub fn write(&self, hash: &str, status: ActivationStatus) -> Result<(), EnsureDriverError> {
        let record = ActivationRecord::new(hash, &self.boot_id, status)
            .map_err(|_| EnsureDriverError::ActivationState)?;
        // Do not silently replace an untrusted or malformed existing receipt.
        self.read()?;
        let path = Path::new(ACTIVATION_RECORD_PATH);
        let temporary =
            path.with_file_name(format!("driver-activation.{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let bytes = record
                .encode()
                .map_err(|_| EnsureDriverError::ActivationState)?;
            let mut file = platform::open_record(&temporary, true)
                .map_err(|_| EnsureDriverError::ActivationState)?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| EnsureDriverError::ActivationState)?;
            drop(file);
            platform::replace(&temporary, path).map_err(|_| EnsureDriverError::ActivationState)?;
            if self.read()?.as_ref() != Some(&record) {
                return Err(EnsureDriverError::ActivationState);
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

fn current_boot_id() -> Result<String, EnsureDriverError> {
    wincmd_shared::encvol_driver_activation::current_boot_id()
        .map_err(|_| EnsureDriverError::ActivationState)
}

#[cfg(test)]
#[path = "encvol_driver_activation_tests.rs"]
mod tests;
