// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pin the object returned by a native picker while its grant is live.
//! A sidecar must reopen the authorized path and compare the same identity.
#![allow(dead_code)]
use crate::file_package_grants::Purpose;
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Component, Path, PathBuf, Prefix},
};
pub(crate) use wincmd_shared::file_package::PackageObjectIdentity as Identity;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, GetFileType, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, SECURITY_IDENTIFICATION,
};
use zeroize::Zeroize;

pub(crate) struct BoundSelection {
    path: PathBuf,
    identity: Identity,
    _held: File,
}
impl BoundSelection {
    pub fn open(path: PathBuf, purpose: Purpose) -> Result<Self, &'static str> {
        validate_path(&path)?;
        let is_output = matches!(purpose, Purpose::Output);
        let target = if is_output {
            path.parent().ok_or("invalid selection")?
        } else {
            path.as_path()
        };
        let directory =
            is_output || matches!(purpose, Purpose::SourceDirectory | Purpose::Destination);
        let mut ancestors: Vec<_> = target
            .parent()
            .map(|parent| parent.ancestors().collect())
            .unwrap_or_default();
        ancestors.reverse();
        let mut pins = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            let held = open_attributes(ancestor, true).map_err(|_| "selection unavailable")?;
            inspect(&held, true).map_err(|_| "unsafe selection")?;
            pins.push(held);
        }
        let held = open_attributes(target, directory).map_err(|_| "selection unavailable")?;
        let mut identity = inspect(&held, directory).map_err(|_| "unsafe selection")?;
        identity.parent = is_output;
        if is_output {
            match std::fs::symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                _ => return Err("output already exists or cannot be checked"),
            }
        }
        // The leaf stays open without delete sharing. Its identity therefore
        // cannot be recycled while the sidecar independently reopens it.
        Ok(Self {
            path,
            identity,
            _held: held,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
}
impl Drop for BoundSelection {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.path)
            .into_os_string()
            .into_encoded_bytes();
        bytes.zeroize();
    }
}

fn validate_path(path: &Path) -> Result<(), &'static str> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        || !matches!(components.next(), Some(Component::RootDir))
        || path.file_name().is_none()
    {
        return Err("choose a local file or folder");
    }
    for component in components {
        let Component::Normal(name) = component else {
            return Err("invalid selection");
        };
        let name = name.to_str().ok_or("invalid selection")?;
        if name.contains(':') || name.ends_with(['.', ' ']) || name.chars().any(char::is_control) {
            return Err("invalid selection");
        }
    }
    Ok(())
}
fn open_attributes(path: &Path, directory: bool) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        )
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(path)
}
fn inspect(file: &File, directory: bool) -> io::Result<Identity> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let is_dir = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if is_dir != directory
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    if index == 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(Identity {
        volume: info.dwVolumeSerialNumber,
        index,
        created: (u64::from(info.ftCreationTime.dwHighDateTime) << 32)
            | u64::from(info.ftCreationTime.dwLowDateTime),
        directory,
        parent: false,
    })
}

#[cfg(test)]
#[path = "file_package_identity_tests.rs"]
mod tests;
