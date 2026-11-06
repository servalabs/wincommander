// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental one-component opens. The caller must authorize and retain the
//! parent. This is not a directory enumerator or a publication protocol.
use std::{ffi::OsStr, fs::File, os::windows::{ffi::OsStrExt, fs::MetadataExt, io::{AsRawHandle, FromRawHandle}}};
use windows_sys::{
    Wdk::{Foundation::OBJECT_ATTRIBUTES, Storage::FileSystem::{NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT}},
    Win32::{Foundation::{OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING}, Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE}, System::IO::IO_STATUS_BLOCK},
};

/// Opens an existing child or exclusively creates a new child relative to a
/// held directory handle. It rejects path syntax, streams and reparse points.
pub(super) fn open_component(parent: &File, name: &OsStr, directory: bool, create: bool) -> Result<File, String> {
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
    let access = FILE_GENERIC_READ | if create && !directory { FILE_GENERIC_WRITE } else { 0 };
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
