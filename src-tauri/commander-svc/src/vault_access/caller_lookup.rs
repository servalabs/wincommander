// SPDX-License-Identifier: AGPL-3.0-or-later
//! A password change, password probe or rename resolves the selected container
//! in the caller's own token, like its later write proof. The SYSTEM service can
//! hold no ACE on a folder the caller owns, and its refusal must not read as a
//! missing file.

use super::*;

/// Why the caller's own Windows access could not reach the selected container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFault {
    NotFound,
    AccessDenied,
    InUse,
}

impl FileFault {
    pub fn from_io(error: &std::io::Error) -> Option<Self> {
        const ERROR_FILE_NOT_FOUND: i32 = 2;
        const ERROR_PATH_NOT_FOUND: i32 = 3;
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_INVALID_DRIVE: i32 = 15;
        const ERROR_NOT_READY: i32 = 21;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        const ERROR_BAD_NETPATH: i32 = 53;
        const ERROR_BAD_NET_NAME: i32 = 67;
        const ERROR_INVALID_NAME: i32 = 123;
        const ERROR_BAD_PATHNAME: i32 = 161;
        match error.raw_os_error()? {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_INVALID_DRIVE | ERROR_NOT_READY
            | ERROR_BAD_NETPATH | ERROR_BAD_NET_NAME | ERROR_INVALID_NAME | ERROR_BAD_PATHNAME => Some(Self::NotFound),
            ERROR_ACCESS_DENIED => Some(Self::AccessDenied),
            ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION => Some(Self::InUse),
            _ => None,
        }
    }

    /// The bare code the app maps to its own wording.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "vault_file_not_found",
            Self::AccessDenied => "vault_access_denied",
            Self::InUse => "vault_file_in_use",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetLookupError {
    File(FileFault),
    Vault(VaultError),
}

impl From<VaultError> for TargetLookupError {
    fn from(error: VaultError) -> Self {
        Self::Vault(error)
    }
}

/// The first refusal the caller gets on the opens the lookup and the write proof
/// make; `None` when it can reach the container or the refusal has no code.
#[cfg(windows)]
fn container_fault(container_path: &str, writable: bool) -> Option<FileFault> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    if wincmd_shared::vault_access::is_supported_vault_device_path(container_path) {
        return None;
    }
    let path = Path::new(container_path);
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::canonicalize(parent) {
            return FileFault::from_io(&error);
        }
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(writable)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
        .err()
        .and_then(|error| FileFault::from_io(&error))
}

#[cfg(windows)]
fn resolve_as_caller<T>(
    container_path: &str,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
    lookup: impl FnOnce() -> Result<T, VaultError>,
) -> Result<T, TargetLookupError> {
    with_caller_impersonation(caller_token, || {
        Ok(lookup().map_err(|error| match container_fault(container_path, false) {
            Some(fault) => TargetLookupError::File(fault),
            None => TargetLookupError::Vault(error),
        }))
    })?
}

/// After a failed write proof: the container's own refusal. A keyfile refusal stays `None`.
#[cfg(windows)]
pub(crate) fn container_fault_as_caller(
    container_path: &str,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
) -> Option<FileFault> {
    with_caller_impersonation(caller_token, || Ok(container_fault(container_path, true)))
        .ok()
        .flatten()
}

#[cfg(windows)]
impl VaultAccessStore {
    pub fn password_change_target_as_caller(
        &self,
        container_path: &str,
        caller_sid: &str,
        caller_session: u32,
        caller_token: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<PasswordChangeTarget, TargetLookupError> {
        resolve_as_caller(container_path, caller_token, || {
            self.password_change_target(container_path, caller_sid, caller_session)
        })
    }
}

#[cfg(all(windows, test))]
#[path = "caller_lookup_tests.rs"]
mod tests;
