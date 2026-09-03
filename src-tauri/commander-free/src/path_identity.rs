// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::{Path, PathBuf};
use wincmd_shared::{
    CanonicalDecimal, DestructiveRequestV2, DestructiveTargetIdentityV2, MutationReceiptV2,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedFileIdentity {
    canonical_path: PathBuf,
    target: DestructiveTargetIdentityV2,
}

impl ExpectedFileIdentity {
    pub(crate) fn capture(path: &Path) -> Result<Self, String> {
        let canonical_path = std::fs::canonicalize(path)
            .map_err(|error| format!("cannot resolve path '{}': {error}", path.display()))?;
        let (volume_serial_number, file_index) =
            crate::routine_cleaner::file_identity(&canonical_path).ok_or_else(|| {
                format!(
                    "cannot read stable file identity for '{}'",
                    canonical_path.display()
                )
            })?;
        Ok(Self {
            canonical_path,
            target: DestructiveTargetIdentityV2::File {
                volume_serial_number: CanonicalDecimal::from_u64(volume_serial_number.into()),
                file_index: CanonicalDecimal::from_u64(file_index),
            },
        })
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) fn request(&self) -> DestructiveRequestV2 {
        DestructiveRequestV2::new(self.target.clone())
    }

    #[cfg(windows)]
    pub(crate) fn delete_file(self) -> Result<(), String> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, GetFileInformationByHandle, SetFileInformationByHandle,
            BY_HANDLE_FILE_INFORMATION, DELETE, FILE_DISPOSITION_INFO,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        };
        let file = std::fs::OpenOptions::new()
            .access_mode(DELETE | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&self.canonical_path)
            .map_err(|error| format!("cannot open file for verified deletion: {error}"))?;
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(format!(
                "cannot read file identity before deletion: {}",
                std::io::Error::last_os_error()
            ));
        }
        let current = DestructiveTargetIdentityV2::File {
            volume_serial_number: CanonicalDecimal::from_u64(
                information.dwVolumeSerialNumber.into(),
            ),
            file_index: CanonicalDecimal::from_u64(
                (u64::from(information.nFileIndexHigh) << 32)
                    | u64::from(information.nFileIndexLow),
            ),
        };
        if current != self.target {
            return Err("file identity changed before deletion".to_string());
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                std::ptr::from_ref(&disposition).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(format!(
                "verified file deletion failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

pub(crate) fn bitlocker_identity(mount_point: &str) -> Result<DestructiveRequestV2, String> {
    Ok(DestructiveRequestV2::new(
        DestructiveTargetIdentityV2::BitlockerVolume {
            volume_guid: bitlocker_volume_guid(mount_point)?,
        },
    ))
}

pub(crate) fn raw_partition_identity(
    disk_number: u32,
    partition_number: u32,
    partition_guid: &str,
    offset_bytes: u64,
    size_bytes: u64,
    disk_unique_id: &str,
) -> DestructiveRequestV2 {
    DestructiveRequestV2::new(DestructiveTargetIdentityV2::RawPartition {
        disk_number: CanonicalDecimal::from_u64(disk_number.into()),
        partition_number: CanonicalDecimal::from_u64(partition_number.into()),
        partition_guid: partition_guid.to_string(),
        offset_bytes: CanonicalDecimal::from_u64(offset_bytes),
        size_bytes: CanonicalDecimal::from_u64(size_bytes),
        disk_unique_id: disk_unique_id.to_string(),
    })
}

pub(crate) fn insert_request(
    payload: &mut serde_json::Map<String, serde_json::Value>,
    request: &DestructiveRequestV2,
) -> Result<(), String> {
    payload.insert(
        "destructiveIdentity".to_string(),
        serde_json::to_value(request).map_err(|error| error.to_string())?,
    );
    Ok(())
}

pub(crate) fn verify_receipt(
    result: &serde_json::Value,
    request: &DestructiveRequestV2,
) -> Result<(), String> {
    let receipt: MutationReceiptV2 = serde_json::from_value(
        result
            .get("mutation_receipt")
            .cloned()
            .ok_or_else(|| "destructive response omitted its mutation receipt".to_string())?,
    )
    .map_err(|error| format!("invalid mutation receipt: {error}"))?;
    if !receipt.verified || receipt.version != request.version || receipt.target != request.target {
        return Err(
            "destructive response identity does not match the authorized target".to_string(),
        );
    }
    Ok(())
}

pub(crate) fn bitlocker_volume_guid(mount_point: &str) -> Result<String, String> {
    let trimmed = mount_point.trim().trim_end_matches(['\\', '/']);
    let bytes = trimmed.as_bytes();
    if !matches!(bytes.len(), 1 | 2)
        || !bytes[0].is_ascii_alphabetic()
        || (bytes.len() == 2 && bytes[1] != b':')
    {
        return Err(format!("invalid BitLocker mount point: {mount_point}"));
    }
    volume_guid_for_mount_root(&format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()))
}

#[cfg(windows)]
fn volume_guid_for_mount_root(mount_root: &str) -> Result<String, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
    let mount: Vec<u16> = std::ffi::OsStr::new(mount_root)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut name = [0u16; 50];
    if unsafe { GetVolumeNameForVolumeMountPointW(mount.as_ptr(), name.as_mut_ptr(), 50) } == 0 {
        return Err(format!(
            "cannot resolve Windows volume GUID for {mount_root}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let length = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    let value = String::from_utf16(&name[..length])
        .map_err(|_| "Windows returned an invalid volume GUID".to_string())?;
    if !is_canonical_volume_guid_path(&value) {
        return Err("Windows returned a non-canonical volume GUID".to_string());
    }
    Ok(value)
}

fn is_canonical_volume_guid_path(value: &str) -> bool {
    value
        .strip_prefix(r"\\?\Volume{")
        .and_then(|value| value.strip_suffix(r"}\"))
        .is_some_and(|guid| uuid::Uuid::parse_str(guid).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_must_match_exact_authorized_identity() {
        let request = raw_partition_identity(1, 2, "g", 9_007_199_254_740_993, u64::MAX, "d");
        let receipt = MutationReceiptV2 {
            version: request.version.clone(),
            target: request.target.clone(),
            verified: true,
        };
        assert!(
            verify_receipt(&serde_json::json!({"mutation_receipt": receipt}), &request).is_ok()
        );
        let other = raw_partition_identity(1, 3, "g", 9_007_199_254_740_993, u64::MAX, "d");
        assert!(verify_receipt(
            &serde_json::json!({"mutation_receipt": MutationReceiptV2 { version: request.version.clone(), target: other.target, verified: true }}),
            &request
        ).is_err());
    }

    #[test]
    fn volume_guid_shape_is_strict() {
        assert!(is_canonical_volume_guid_path(
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\"
        ));
        assert!(!is_canonical_volume_guid_path("C:\\"));
    }
}
