// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental one-component opens. The caller must authorize and retain the
//! parent. This is not a directory enumerator or a publication protocol.
use std::{ffi::OsStr, fs::File, os::windows::{ffi::OsStrExt, fs::MetadataExt, io::{AsRawHandle, FromRawHandle}}};
use windows_sys::{
    Wdk::{Foundation::OBJECT_ATTRIBUTES, Storage::FileSystem::{NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT}},
    Win32::{Foundation::{OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING}, Storage::FileSystem::{DELETE, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE}, System::IO::IO_STATUS_BLOCK},
};

/// Deliberately has no production constructor. A future constructor must
/// establish the Known Folder and all ancestors without pathname races, and
/// retain that authority through every operation. Comparing arbitrary handles
/// does not authorize a root.
pub(super) struct DirectoryAuthority { handle: File }

/// A directory whose identity was compared with an independently established
/// authority. This does not make pathname-based operations safe.
pub(super) struct BoundDirectory<'a> { handle: File, _authority: &'a DirectoryAuthority }

impl<'a> BoundDirectory<'a> {
    pub(super) fn bind(handle: File, authorized: &'a DirectoryAuthority) -> Result<Self, String> {
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        let mut a = BY_HANDLE_FILE_INFORMATION::default();
        let mut b = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: both handles and output structures remain valid for the calls.
        if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut a) } == 0
            || unsafe { GetFileInformationByHandle(authorized.handle.as_raw_handle(), &mut b) } == 0
            || (a.dwVolumeSerialNumber, a.nFileIndexHigh, a.nFileIndexLow)
                != (b.dwVolumeSerialNumber, b.nFileIndexHigh, b.nFileIndexLow)
            || a.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || b.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || !handle.metadata().map_err(|_| "settings directory unavailable")?.is_dir()
            || !authorized.handle.metadata().map_err(|_| "settings directory unavailable")?.is_dir() {
            return Err("settings directory identity or type mismatch".into());
        }
        Ok(Self { handle, _authority: authorized })
    }

    /// Each component is opened relative to the held parent and rejects a
    /// reparse point. Callers must retain this owner until their operation ends.
    pub(super) fn open_child(&self, name: &OsStr, directory: bool) -> Result<File, String> {
        open_component(&self.handle, name, directory, false)
    }

    /// Creates only a new child; never replaces a prior file.
    pub(super) fn create_child(&self, name: &OsStr) -> Result<File, String> {
        create_stage(&self.handle, name)
    }
}

/// Opens an existing child or exclusively creates a new child relative to a
/// held directory handle. It rejects path syntax, streams and reparse points.
pub(super) fn open_component(parent: &File, name: &OsStr, directory: bool, create: bool) -> Result<File, String> {
    open_component_inner(parent, name, directory, create, false)
}

/// Exclusively creates a stage with rename authority; retain this exact handle
/// through content verification and publication.
pub(super) fn create_stage(parent: &File, name: &OsStr) -> Result<File, String> {
    open_component_inner(parent, name, false, true, true)
}

fn open_component_inner(parent: &File, name: &OsStr, directory: bool, create: bool, can_rename: bool) -> Result<File, String> {
    let mut name: Vec<u16> = name.encode_wide().collect();
    if name.is_empty() || name.len() > 32767 || name == [46] || name == [46, 46]
        || name.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92)) {
        return Err("invalid settings path component".into());
    }
    let length = (name.len() * 2) as u16;
    let path = UNICODE_STRING { Length: length, MaximumLength: length, Buffer: name.as_mut_ptr() };
    let attributes = OBJECT_ATTRIBUTES { Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(), ObjectName: &path,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE, ..Default::default() };
    let mut handle = std::ptr::null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    let flags = FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT |
        if directory { FILE_DIRECTORY_FILE } else { FILE_NON_DIRECTORY_FILE };
    let access = FILE_GENERIC_READ | if create && !directory { FILE_GENERIC_WRITE } else { 0 } | if can_rename { DELETE } else { 0 };
    let share = FILE_SHARE_READ | if directory { FILE_SHARE_WRITE } else { 0 };
    // SAFETY: all arguments and the held parent remain valid throughout the call.
    let result = unsafe { NtCreateFile(&mut handle, access, &attributes, &mut status, std::ptr::null(),
        FILE_ATTRIBUTE_NORMAL, share, if create { FILE_CREATE } else { FILE_OPEN }, flags, std::ptr::null(), 0) };
    if result < 0 || handle.is_null() { return Err("settings component unavailable or redirected".into()); }
    // SAFETY: NtCreateFile returned a new owned handle.
    let file = unsafe { File::from_raw_handle(handle) };
    let metadata = file.metadata().map_err(|_| "settings component unavailable")?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 ||
        (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err("settings component is not an ordinary file or directory".into());
    }
    Ok(file)
}

/// Publishes the held stage to one child name, refusing to replace an existing
/// target. This is experimental: callers must verify the parent has not been
/// redirected, journal the outcome and handle interrupted directory updates.
pub(super) fn publish_new(stage: &File, parent: &File, name: &OsStr) -> Result<(), String> {
    use windows_sys::Wdk::Storage::FileSystem::{FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION, FILE_RENAME_INFORMATION_0};
    let name: Vec<u16> = name.encode_wide().collect();
    if name.is_empty() || name.len() > 32767 || name == [46] || name == [46, 46]
        || name.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92)) {
        return Err("invalid settings destination".into());
    }
    stage.sync_all().map_err(|_| "settings stage could not be flushed")?;
    let name_bytes = name.len() * 2;
    let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>().checked_add(name_bytes)
        .ok_or("invalid settings destination")?;
    let mut aligned = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let info = aligned.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: `aligned` is aligned and large enough for the fixed fields and
    // UTF-16 name; both Windows handles stay live during this call.
    let result = unsafe {
        (*info).Anonymous = FILE_RENAME_INFORMATION_0 { ReplaceIfExists: false };
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = name_bytes as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(), name.len());
        let mut status = IO_STATUS_BLOCK::default();
        NtSetInformationFile(stage.as_raw_handle(), &mut status, info.cast(), bytes as u32, FileRenameInformation)
    };
    if result < 0 { return Err("settings publication was not confirmed; preserve both paths for recovery".into()); }
    stage.sync_all().map_err(|_| "settings publication outcome uncertain; preserve both paths for recovery".into())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, io::Write, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};

    fn directory(path: &std::path::Path) -> File {
        OpenOptions::new().read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path).unwrap()
    }

    #[test]
    fn binding_requires_same_directory_identity_before_relative_access() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first"); let second = root.path().join("second");
        std::fs::create_dir(&first).unwrap(); std::fs::create_dir(&second).unwrap();
        let second_authority = DirectoryAuthority { handle: directory(&second) };
        assert!(BoundDirectory::bind(directory(&first), &second_authority).is_err());
        let first_authority = DirectoryAuthority { handle: directory(&first) };
        let bound = BoundDirectory::bind(directory(&first), &first_authority).unwrap();
        let mut child = bound.create_child(OsStr::new("settings.dat")).unwrap();
        child.write_all(b"fixture").unwrap(); drop(child);
        let mut child = bound.open_child(OsStr::new("settings.dat"), false).unwrap();
        let mut value = String::new();
        use std::io::Read;
        child.read_to_string(&mut value).unwrap(); assert_eq!(value, "fixture");
        assert!(!second.join("settings.dat").exists());
    }

    #[test]
    fn publication_refuses_replacement_and_preserves_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let parent = OpenOptions::new().read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(dir.path()).unwrap();
        let mut stage = create_stage(&parent, OsStr::new("stage")).unwrap();
        stage.write_all(b"protected settings").unwrap();
        std::fs::write(dir.path().join("selected"), b"existing").unwrap();
        assert!(publish_new(&stage, &parent, OsStr::new("selected")).is_err());
        assert_eq!(std::fs::read(dir.path().join("selected")).unwrap(), b"existing");
        assert_eq!(std::fs::read(dir.path().join("stage")).unwrap(), b"protected settings");
        publish_new(&stage, &parent, OsStr::new("new-selected")).unwrap();
        assert_eq!(std::fs::read(dir.path().join("new-selected")).unwrap(), b"protected settings");
    }

    #[test]
    fn child_names_reject_traversal_and_streams() {
        let dir = tempfile::tempdir().unwrap();
        let parent = OpenOptions::new().read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(dir.path()).unwrap();
        for name in ["", ".", "..", "a/b", "a\\b", "a:stream"] {
            assert!(create_stage(&parent, OsStr::new(name)).is_err());
        }
    }
}
