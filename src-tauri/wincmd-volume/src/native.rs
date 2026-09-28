// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{driver, private_slot, VolumeInfo};
use std::{
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*};

pub(crate) struct Handle(pub HANDLE);
impl Handle {
    pub(crate) fn new(handle: HANDLE) -> Result<Self, String> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err("Native volume access unavailable".into())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(crate) fn wide(value: &str) -> Result<Vec<u16>, String> {
    if value.contains('\0') {
        return Err("Invalid path".into());
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

fn checked_path(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_str().ok_or("Invalid path encoding")?;
    let text = text
        .strip_prefix(r"\\?\")
        .unwrap_or(text)
        .replace('/', "\\");
    let bytes = text.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1..3] != *b":\\"
        || text.contains('\0')
    {
        return Err("Use an absolute local drive path".into());
    }
    for component in text[3..].split('\\').filter(|part| !part.is_empty()) {
        if component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component.contains(':')
        {
            return Err("Ambiguous path components are not supported".into());
        }
    }
    let path = PathBuf::from(text);
    for ancestor in path.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)
            .map_err(|_| "Volume path is absent or inaccessible")?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Reparse-point paths are not supported for private search".into());
        }
    }
    Ok(path)
}

fn device_for_drive(letter: char) -> Result<Option<String>, String> {
    let name = wide(&format!("{letter}:"))?;
    let mut target = vec![0u16; 32768];
    let length =
        unsafe { QueryDosDeviceW(name.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
    if length == 0 {
        return match unsafe { GetLastError() } {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
            _ => Err("Drive identity unavailable".into()),
        };
    }
    let end = target
        .iter()
        .position(|c| *c == 0)
        .ok_or("Invalid device mapping")?;
    let device = String::from_utf16(&target[..end]).map_err(|_| "Invalid device mapping")?;
    Ok(Some(device))
}

fn final_device_matches(path: &Path, device: &str) -> Result<(), String> {
    let name = wide(path.to_str().ok_or("Invalid path encoding")?)?;
    let handle = Handle::new(unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    })?;
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle.0,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            VOLUME_NAME_NT,
        )
    };
    if length == 0 || length as usize >= buffer.len() {
        return Err("Final volume identity unavailable".into());
    }
    let final_path = String::from_utf16(&buffer[..length as usize])
        .map_err(|_| "Invalid final volume identity")?
        .to_ascii_lowercase();
    let prefix = format!("{}\\", device.to_ascii_lowercase());
    if !final_path.starts_with(&prefix) {
        return Err("Volume path changed or resolves through another device".into());
    }
    Ok(())
}

pub(crate) fn inspect_path(path: &Path) -> Result<VolumeInfo, String> {
    let path = checked_path(path)?;
    let letter = path.to_str().ok_or("Invalid path")?.as_bytes()[0].to_ascii_uppercase() as char;
    let root = PathBuf::from(format!("{letter}:\\"));
    let device = device_for_drive(letter)?.ok_or("Volume is not mounted")?;
    let lower = device.to_ascii_lowercase();
    // A SUBST drive can hide a private filesystem; direct inspection must not trust it.
    if !lower.starts_with(r"\device\") {
        return Err("Drive aliases are not supported for private search".into());
    }
    if lower.starts_with(r"\device\mup") || lower.starts_with(r"\device\lanmanredirector") {
        return Err("Remote volumes are not supported for private search".into());
    }
    final_device_matches(&path, &device)?;
    let root_wide = wide(root.to_str().unwrap())?;
    let (mut serial, mut flags) = (0, 0);
    if unsafe {
        GetVolumeInformationW(
            root_wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            &mut flags,
            std::ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err("Mounted filesystem identity unavailable".into());
    }
    let is_private = lower.contains("veracrypt") || lower.contains("truecrypt");
    let (stable_id, identity, read_only) = if is_private {
        let native = driver::properties(private_slot(&device)?)?;
        let identity = format!("{}:{}", native.stable_id, native.generation);
        (native.stable_id, identity, native.read_only)
    } else {
        let stable_id = format!("{lower}:{serial:08x}");
        (stable_id.clone(), stable_id, false)
    };
    if device_for_drive(letter)?.as_deref() != Some(&device) {
        return Err("Mounted volume changed during inspection".into());
    }
    Ok(VolumeInfo {
        root,
        stable_id,
        identity,
        device,
        is_private,
        read_only: read_only || flags & 0x0008_0000 != 0,
    })
}

pub(crate) fn mounted_private_volumes() -> Result<Vec<VolumeInfo>, String> {
    let mut volumes = Vec::new();
    for letter in 'A'..='Z' {
        let device = match device_for_drive(letter) {
            Ok(Some(device)) => device,
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        let lower = device.to_ascii_lowercase();
        // Aliases are rejected when used, but must not disable unrelated direct volumes.
        if !lower.starts_with(r"\device\") {
            continue;
        }
        if lower.contains("veracrypt") || lower.contains("truecrypt") {
            volumes.push(inspect_path(Path::new(&format!("{letter}:\\")))?);
        }
    }
    Ok(volumes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_paths_and_device_or_relative_aliases() {
        for value in [
            r"C:relative",
            r"\\server\share",
            r"\\.\C:\",
            r"C:\..\Windows",
            r"C:\file:stream",
            r"C:\ambiguous.\child",
        ] {
            assert!(checked_path(Path::new(value)).is_err(), "{value}");
        }
        assert!(checked_path(Path::new(r"C:\wincmd-nonexistent-volume-probe-73c5b62b")).is_err());
    }

    #[test]
    fn inspects_existing_local_working_directory_without_creating_files() {
        let cwd = std::env::current_dir().unwrap();
        let info = inspect_path(&cwd).expect("inspect local test checkout");
        assert!(!info.device.is_empty());
        assert!(!info.identity.is_empty());
        assert!(info.root.is_absolute());
    }
}
