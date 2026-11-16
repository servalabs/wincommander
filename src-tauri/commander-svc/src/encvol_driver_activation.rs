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
        Some(record)
            if record.status == ActivationStatus::PendingReboot && record.boot_id == boot =>
        {
            RunningAction::RestartRequired
        }
        Some(_) => RunningAction::ConfirmActive,
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

#[repr(C)]
#[derive(Default)]
struct BootEnvironment {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
    firmware_type: u32,
    boot_flags: u64,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        data: *mut core::ffi::c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

fn current_boot_id() -> Result<String, EnsureDriverError> {
    let mut data = BootEnvironment::default();
    let mut returned = 0;
    // BootIdentifier is kernel-provided; wall-clock adjustments cannot clear a pending restart.
    let status = unsafe {
        NtQuerySystemInformation(
            90,
            (&mut data as *mut BootEnvironment).cast(),
            std::mem::size_of::<BootEnvironment>() as u32,
            &mut returned,
        )
    };
    if status < 0 || returned < 20 {
        return Err(EnsureDriverError::ActivationState);
    }
    let boot = format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        data.data1,
        data.data2,
        data.data3,
        data.data4[0],
        data.data4[1],
        data.data4[2],
        data.data4[3],
        data.data4[4],
        data.data4[5],
        data.data4[6],
        data.data4[7]
    );
    ActivationRecord::new(&"0".repeat(64), &boot, ActivationStatus::Active)
        .map_err(|_| EnsureDriverError::ActivationState)?;
    Ok(boot)
}

#[cfg(test)]
#[path = "encvol_driver_activation_tests.rs"]
mod tests;
