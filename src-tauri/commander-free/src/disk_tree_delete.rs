//! Experimental handle-bound recursive deletion. Not connected to the UI or
//! any command until identity, reparse and race tests have passed review.
#![cfg(windows)]

use std::ffi::{c_void, OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf, Prefix};

const READ_ATTRIBUTES: u32 = 0x80;
const LIST_DIRECTORY: u32 = 0x01;
const TRAVERSE: u32 = 0x20;
const DELETE: u32 = 0x0001_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const SHARE_READ_WRITE: u32 = 0x3; // Deliberately omit FILE_SHARE_DELETE.
const BACKUP_SEMANTICS: u32 = 0x0200_0000;
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_OPEN: u32 = 1;
const FILE_DIRECTORY_FILE: u32 = 0x1;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const OBJ_CASE_INSENSITIVE: u32 = 0x40;
const OBJ_DONT_REPARSE: u32 = 0x1000;
const FILE_ID_BOTH_DIRECTORY_INFO: i32 = 10;
const FILE_ID_BOTH_DIRECTORY_RESTART_INFO: i32 = 11;
const FILE_DISPOSITION_INFO_CLASS: i32 = 4;
const FILE_DISPOSITION_INFO_EX_CLASS: i32 = 21;
const ERROR_NO_MORE_FILES: i32 = 18;
const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 128;

type Handle = *mut c_void;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: Handle,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}
#[repr(C)]
struct IoStatusBlock {
    status_or_pointer: usize,
    information: usize,
}
#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}
impl Guid {
    const fn from_u128(value: u128) -> Self {
        Self {
            data1: (value >> 96) as u32,
            data2: (value >> 80) as u16,
            data3: (value >> 64) as u16,
            data4: (value as u64).to_be_bytes(),
        }
    }
}
#[repr(C)]
#[derive(Default)]
struct ByHandleFileInformation {
    attributes: u32,
    creation_time: [u32; 2],
    access_time: [u32; 2],
    write_time: [u32; 2],
    volume_serial: u32,
    size_high: u32,
    size_low: u32,
    number_of_links: u32,
    index_high: u32,
    index_low: u32,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut Handle,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatusBlock,
        size: *mut i64,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *mut c_void,
        ea_len: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHGetKnownFolderPath(
        folder: *const Guid,
        flags: u32,
        token: Handle,
        path: *mut *mut u16,
    ) -> i32;
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(ptr: *const c_void);
}

fn known_folder(id: u128) -> io::Result<PathBuf> {
    let mut path: *mut u16 = std::ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(&Guid::from_u128(id), 0, std::ptr::null_mut(), &mut path) };
    if result < 0 || path.is_null() {
        if !path.is_null() {
            unsafe {
                CoTaskMemFree(path.cast());
            }
        }
        return Err(io::Error::other(
            "a required Windows protected location is unavailable",
        ));
    }
    struct Allocated(*mut u16);
    impl Drop for Allocated {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(self.0.cast());
            }
        }
    }
    let allocated = Allocated(path);
    let mut length = 0usize;
    while length < 32768 && unsafe { *allocated.0.add(length) } != 0 {
        length += 1;
    }
    if length == 32768 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Windows protected location",
        ));
    }
    Ok(PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(allocated.0, length)
    })))
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(file: Handle, info: *mut ByHandleFileInformation) -> i32;
    fn GetFileInformationByHandleEx(file: Handle, class: i32, info: *mut c_void, len: u32) -> i32;
    fn SetFileInformationByHandle(file: Handle, class: i32, info: *const c_void, len: u32) -> i32;
    fn GetFinalPathNameByHandleW(file: Handle, path: *mut u16, len: u32, flags: u32) -> u32;
    fn CompareStringOrdinal(
        first: *const u16,
        first_len: i32,
        second: *const u16,
        second_len: i32,
        ignore_case: i32,
    ) -> i32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetVolumeInformationW(
        root: *const u16,
        volume_name: *mut u16,
        volume_name_len: u32,
        serial: *mut u32,
        max_component: *mut u32,
        flags: *mut u32,
        filesystem: *mut u16,
        filesystem_len: u32,
    ) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Identity {
    pub volume: u32,
    pub index: u64,
}

fn identity(file: &File) -> io::Result<(Identity, u32)> {
    let mut info = ByHandleFileInformation::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        Identity {
            volume: info.volume_serial,
            index: (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        },
        info.attributes,
    ))
}

/// A differently named hard link can make path-based protection incomplete.
/// Fail closed when link information is unavailable or an additional link exists.
fn require_single_file_link(file: &File) -> io::Result<()> {
    let mut info = ByHandleFileInformation::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.number_of_links != 1 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "multiply linked files cannot be deleted"));
    }
    Ok(())
}

fn open_root(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(DELETE | READ_ATTRIBUTES | LIST_DIRECTORY | SYNCHRONIZE)
        .share_mode(SHARE_READ_WRITE)
        .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
        .open(path)
}

fn final_path(file: &File) -> io::Result<PathBuf> {
    final_path_with_flags(file, 0)
}

fn final_path_with_flags(file: &File, flags: u32) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let len = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            flags,
        )
    } as usize;
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    if len >= buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resolved path is too long",
        ));
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
}

fn ordinal_eq(a: &OsStr, b: &OsStr) -> bool {
    let a: Vec<u16> = a.encode_wide().collect();
    let b: Vec<u16> = b.encode_wide().collect();
    if a.len() > i32::MAX as usize || b.len() > i32::MAX as usize {
        return false;
    }
    unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) == 2 }
}

fn path_prefix(parent: &Path, child: &Path) -> bool {
    let mut children = child.components();
    parent.components().all(|part| {
        children
            .next()
            .is_some_and(|candidate| ordinal_eq(part.as_os_str(), candidate.as_os_str()))
    })
}

/// Add a verbatim prefix without resolving any component. Following a link
/// before obtaining no-follow handles could authorize its target instead.
fn lexical_windows_path(path: &Path) -> io::Result<PathBuf> {
    let mut components = path.components();
    let prefix = match components.next() {
        Some(Component::Prefix(prefix)) => prefix.kind(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an absolute Windows path is required",
            ))
        }
    };
    let mut result = match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => PathBuf::from(format!(
            r"\\?\{}:\",
            char::from(letter).to_ascii_uppercase()
        )),
        Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _) => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "network paths are not supported",
            ))
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported Windows path namespace",
            ))
        }
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an absolute Windows path is required",
        ));
    }
    for component in components {
        match component {
            Component::Normal(name) => result.push(name),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "relative path components are not allowed",
                ))
            }
        }
    }
    Ok(result)
}

fn require_fixed_ntfs(volume: &Path) -> io::Result<()> {
    let root: Vec<u16> = volume.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { GetDriveTypeW(root.as_ptr()) } != 3 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "only fixed local NTFS volumes are supported",
        ));
    }
    let mut filesystem = [0u16; 261];
    if unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let len = filesystem
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(filesystem.len());
    if !String::from_utf16_lossy(&filesystem[..len]).eq_ignore_ascii_case("NTFS") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "only fixed local NTFS volumes are supported",
        ));
    }
    Ok(())
}

fn protected_scope_intersects(selected: &Path, scopes: &[(PathBuf, bool)]) -> bool {
    scopes.iter().any(|(path, protect_descendants)| {
        path_prefix(selected, path) || (*protect_descendants && path_prefix(path, selected))
    })
}

fn open_child(parent: &File, name: &OsStr, is_dir: bool, for_delete: bool, backup_intent: bool) -> io::Result<File> {
    if backup_intent && for_delete { return Err(io::Error::new(io::ErrorKind::InvalidInput, "backup privilege cannot be used for deletion")); }
    let mut utf16: Vec<u16> = name.encode_wide().collect();
    if utf16.is_empty()
        || utf16.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92))
        || utf16.len() > u16::MAX as usize / 2
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid directory entry name",
        ));
    }
    let mut unicode = UnicodeString {
        length: (utf16.len() * 2) as u16,
        maximum_length: (utf16.len() * 2) as u16,
        buffer: utf16.as_mut_ptr(),
    };
    let mut attrs = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut unicode,
        attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut status = IoStatusBlock {
        status_or_pointer: 0,
        information: 0,
    };
    let mut handle: Handle = std::ptr::null_mut();
    let access = READ_ATTRIBUTES
        | SYNCHRONIZE
        | if for_delete { DELETE | READ_CONTROL } else { 0 }
        | if is_dir {
            if for_delete {
                LIST_DIRECTORY
            } else {
                TRAVERSE
            }
        } else {
            0
        };
    let options = OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if backup_intent { 0x0000_4000 } else { 0 }
        | if is_dir {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    let rc = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attrs,
            &mut status,
            std::ptr::null_mut(),
            0,
            SHARE_READ_WRITE,
            FILE_OPEN,
            options,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(rc) } as i32,
        ));
    }
    if handle.is_null() {
        return Err(io::Error::other("Windows did not return a file handle"));
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ListedEntry {
    name: OsString,
    is_dir: bool,
    index: u64,
}

fn same_directory_contents(expected: &[ListedEntry], mut current: Vec<ListedEntry>) -> bool {
    current.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    current == expected
}

fn u32_at(buf: &[u8], offset: usize) -> io::Result<u32> {
    let bytes: [u8; 4] = buf
        .get(offset..offset + 4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated directory entry"))?
        .try_into()
        .unwrap();
    Ok(u32::from_le_bytes(bytes))
}
fn u64_at(buf: &[u8], offset: usize) -> io::Result<u64> {
    let bytes: [u8; 8] = buf
        .get(offset..offset + 8)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated directory entry"))?
        .try_into()
        .unwrap();
    Ok(u64::from_le_bytes(bytes))
}

fn list_directory(dir: &File) -> io::Result<Vec<ListedEntry>> {
    // Aligned storage is required by GetFileInformationByHandleEx.
    let mut storage = vec![0u64; 8192];
    let mut all = Vec::new();
    let mut class = FILE_ID_BOTH_DIRECTORY_RESTART_INFO;
    loop {
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle(),
                class,
                storage.as_mut_ptr().cast(),
                (storage.len() * 8) as u32,
            )
        };
        class = FILE_ID_BOTH_DIRECTORY_INFO;
        if ok == 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(ERROR_NO_MORE_FILES) {
                break;
            }
            return Err(e);
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), storage.len() * 8) };
        let mut offset = 0usize;
        loop {
            // FILE_ID_BOTH_DIR_INFO has an aligned FileId at 96 and FileName at 104.
            let next = u32_at(bytes, offset)? as usize;
            let attrs = u32_at(bytes, offset + 56)?;
            let len = u32_at(bytes, offset + 60)? as usize;
            let index = u64_at(bytes, offset + 96)?;
            if len % 2 != 0
                || len > 65534
                || offset
                    .checked_add(104 + len)
                    .is_none_or(|end| end > bytes.len())
                || (next != 0
                    && (next < 104 + len
                        || offset.checked_add(next).is_none_or(|end| end > bytes.len())))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid directory entry",
                ));
            }
            let name_bytes = &bytes[offset + 104..offset + 104 + len];
            let words: Vec<u16> = name_bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let name = OsString::from_wide(&words);
            if name != "." && name != ".." {
                if attrs & ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "folder contains a link or mount point",
                    ));
                }
                all.push(ListedEntry {
                    name,
                    is_dir: attrs & ATTRIBUTE_DIRECTORY != 0,
                    index,
                });
                if all.len() > MAX_ENTRIES {
                    return Err(io::Error::other("folder exceeds the safe deletion limit"));
                }
            }
            if next == 0 {
                break;
            }
            offset += next;
        }
    }
    Ok(all)
}

struct Pinned {
    file: File,
    is_dir: bool,
    owner: Option<crate::duplicate_finder::verified_file::OwnerClass>,
}

/// Hold protected identities without delete sharing until the operation ends,
/// so a protected root cannot be renamed underneath the preflight checks.
pub(crate) struct ProtectedRoots {
    handles: Vec<(PathBuf, File, bool)>,
    identities: Vec<Identity>,
    scopes: Vec<(PathBuf, bool)>,
}

impl ProtectedRoots {
    #[cfg(test)]
    pub(crate) fn fixture(path: &Path) -> io::Result<Self> {
        Self::pin(&[(path.to_path_buf(), false)])
    }
    /// Obtain security-sensitive roots from Windows known-folder APIs rather
    /// than process environment variables, which are caller-controlled.
    pub(crate) fn system() -> io::Result<Self> {
        let mut roots = vec![
            (known_folder(0xf38bf404_1d43_42f2_9305_67de0b28fc23)?, true), // Windows
            (known_folder(0x905e63b6_c1bf_494e_b29c_65b732d3d21a)?, true), // Program Files
            (known_folder(0x7c5a40ef_a0fb_4bfc_874a_c0f2e0b9fa8e)?, true), // Program Files x86
            (known_folder(0x62ab5d82_fdc1_4dc3_a9dd_070d1d495d97)?, true), // ProgramData
            (known_folder(0xdfdf76a2_c82a_4d63_906a_5644ac457385)?, false), // Public
            (known_folder(0x5e6c858f_0e22_4760_9afe_ea3317b67173)?, false), // current profile
        ];
        // Every registered profile must resolve successfully: an incomplete
        // inventory cannot safely distinguish another user's data.
        #[cfg(feature = "experimental-disk-tree-delete")]
        {
            let (current_sid, profiles) = crate::duplicate_finder::profile_policy::profiles_unresolved()
                .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
            roots.extend(other_profile_roots(&current_sid, profiles));
        }
        let exe = std::env::current_exe()?;
        roots.push((
            exe.parent()
                .ok_or_else(|| io::Error::other("application directory is unavailable"))?
                .to_path_buf(),
            true,
        ));
        crate::disk_backup_privilege::with_backup_privilege(|| Self::pin_inner(&roots, true))
            .map_err(|error| {
                if error.raw_os_error() == Some(1300) {
                    io::Error::new(io::ErrorKind::PermissionDenied,
                        "Protected-folder verification requires administrator permissions. Restart WinCommander as administrator to delete a folder.")
                } else {
                    contextual_error("protected-root discovery", error)
                }
            })
    }
    /// Resolve each component relative to an already pinned parent handle.
    /// All non-volume ancestors are held for traversal without
    /// FILE_SHARE_DELETE to prevent namespace replacement during the operation.
    fn lock_directory(&mut self, path: &Path, backup_intent: bool) -> io::Result<usize> {
        let volume = path
            .ancestors()
            .last()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing volume root"))?;
        require_fixed_ntfs(volume).map_err(|error| contextual_error("volume eligibility check", error))?;
        let mut cursor = volume.to_path_buf();
        let mut index = if let Some(i) = self
            .handles
            .iter()
            .position(|(p, _, privileged)| *privileged == backup_intent && path_prefix(p, &cursor) && path_prefix(&cursor, p))
        {
            i
        } else {
            // A volume root itself cannot be renamed. Asking DELETE on it is
            // unnecessary and commonly denied by Windows.
            let root = OpenOptions::new()
                .access_mode(READ_ATTRIBUTES | TRAVERSE | SYNCHRONIZE)
                .share_mode(SHARE_READ_WRITE)
                .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
                .open(volume).map_err(|error| contextual_error("open volume root", error))?;
            let (id, attrs) = identity(&root).map_err(|error| contextual_error("volume identity", error))?;
            // Normalizing a volume root can require permissions that the
            // restricted backup token deliberately does not carry. A root has
            // no intermediate components to resolve, so its opened name is
            // sufficient here, alongside the volume identity and reparse
            // checks below. Descendant names still require normalization.
            let final_name = final_path_with_flags(&root, 0x8 /* FILE_NAME_OPENED */)
                .map_err(|error| contextual_error("volume final path", error))?;
            if id.index == 0
                || attrs & ATTRIBUTE_REPARSE_POINT != 0
                || attrs & ATTRIBUTE_DIRECTORY == 0
                || !path_prefix(&cursor, &final_name)
                || !path_prefix(&final_name, &cursor)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "volume root identity is unsafe",
                ));
            }
            self.handles.push((cursor.clone(), root, backup_intent));
            self.handles.len() - 1
        };
        let root_volume = identity(&self.handles[index].1)?.0.volume;
        let relative = path
            .strip_prefix(volume)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid volume path"))?;
        for (depth, part) in relative.components().enumerate() {
            let std::path::Component::Normal(name) = part else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid path component",
                ));
            };
            cursor.push(name);
            if let Some(existing) = self
                .handles
                .iter()
                .position(|(p, _, privileged)| *privileged == backup_intent && path_prefix(p, &cursor) && path_prefix(&cursor, p))
            {
                index = existing;
                continue;
            }
            let child = open_child(&self.handles[index].1, name, true, false, backup_intent)
                .map_err(|error| contextual_error(&format!("cannot open component {}", depth + 1), error))?;
            let (id, attrs) = identity(&child)
                .map_err(|error| contextual_error(&format!("component {} identity", depth + 1), error))?;
            let final_name = final_path(&child)
                .map_err(|error| contextual_error(&format!("component {} final path", depth + 1), error))?;
            if id.index == 0
                || id.volume != root_volume
                || attrs & ATTRIBUTE_REPARSE_POINT != 0
                || attrs & ATTRIBUTE_DIRECTORY == 0
                || !path_prefix(&cursor, &final_name)
                || !path_prefix(&final_name, &cursor)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "folder ancestor changed or is a link",
                ));
            }
            self.handles.push((cursor.clone(), child, backup_intent));
            index = self.handles.len() - 1;
        }
        Ok(index)
    }

    /// `protect_descendants` distinguishes Windows/app system roots from user
    /// roots, where deleting a specifically selected child may be allowed.
    fn pin(paths: &[(PathBuf, bool)]) -> io::Result<Self> {
        Self::pin_inner(paths, false)
    }

    fn pin_inner(paths: &[(PathBuf, bool)], backup_intent: bool) -> io::Result<Self> {
        let mut pinned = Self {
            handles: Vec::new(),
            identities: Vec::new(),
            scopes: Vec::new(),
        };
        for (root_index, (path, protect_descendants)) in paths.iter().enumerate() {
            let resolved = lexical_windows_path(path)?;
            if let Some((_, protect)) = pinned.scopes.iter_mut().find(|(existing, _)| {
                path_prefix(existing, &resolved) && path_prefix(&resolved, existing)
            }) {
                *protect |= *protect_descendants;
                continue;
            }
            let index = pinned.lock_directory(&resolved, backup_intent).map_err(|error| {
                contextual_error(&format!("cannot verify protected root {}", root_index + 1), error)
            })?;
            let (id, attrs) = identity(&pinned.handles[index].1)?;
            if id.index == 0 || attrs & ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "protected root identity is unavailable",
                ));
            }
            pinned.identities.push(id);
            pinned.scopes.push((resolved, *protect_descendants));
        }
        Ok(pinned)
    }
}

fn contextual_error(context: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{context}: {error}"))
}

#[cfg(feature = "experimental-disk-tree-delete")]
fn other_profile_roots(current_sid: &str, profiles: Vec<(String, PathBuf)>) -> Vec<(PathBuf, bool)> {
    profiles.into_iter().filter_map(|(sid, path)| {
        (!sid.eq_ignore_ascii_case(current_sid)).then_some((path, true))
    }).collect()
}

#[cfg(all(test, feature = "experimental-disk-tree-delete"))]
#[test]
fn other_profile_and_descendants_are_protected() {
    let roots = other_profile_roots("S-1-5-21-1", vec![
        ("s-1-5-21-1".into(), PathBuf::from(r"C:\Users\Current")),
        ("S-1-5-21-2".into(), PathBuf::from(r"D:\Users\Other")),
    ]);
    assert_eq!(roots.len(), 1);
    assert!(protected_scope_intersects(Path::new(r"D:\Users\Other"), &roots));
    assert!(protected_scope_intersects(Path::new(r"D:\Users\Other\Documents"), &roots));
    assert!(!protected_scope_intersects(Path::new(r"C:\Users\Current\Documents"), &roots));
}

pub(crate) struct PreparedTree {
    entries: Vec<Pinned>,
    directory_snapshots: Vec<(usize, Vec<ListedEntry>)>,
    protected: ProtectedRoots,
    authorization_path: PathBuf,
    authorization_identity: Identity,
    enforce_user_policy: bool,
}

/// Owns the exact target and its protected ancestors throughout confirmation.
/// Dropping an expired or rejected capability releases all held handles.
pub(crate) struct AuthorizedRoot {
    root: File,
    protected: ProtectedRoots,
    path: PathBuf,
    id: Identity,
}

impl AuthorizedRoot {
    pub(crate) fn target(&self) -> (&Path, Identity) {
        (&self.path, self.id)
    }

    pub(crate) fn open(root_path: &Path, mut protected: ProtectedRoots) -> io::Result<Self> {
        let (root, path, id) = open_selected(root_path, &mut protected)?;
        Ok(Self { root, protected, path, id })
    }
}

impl PreparedTree {
    pub(crate) fn authorization_target(&self) -> (&Path, Identity) {
        (&self.authorization_path, self.authorization_identity)
    }
    pub(crate) fn item_counts(&self) -> (usize, usize) {
        let folders = self.entries.iter().filter(|entry| entry.is_dir).count();
        (self.entries.len() - folders, folders)
    }
    pub(crate) fn has_shared_administrator_owned_items(&self) -> bool {
        self.entries.iter().any(|entry| entry.owner == Some(crate::duplicate_finder::verified_file::OwnerClass::SharedAdministrators))
    }
}

fn open_selected(
    root_path: &Path,
    protected: &mut ProtectedRoots,
) -> io::Result<(File, PathBuf, Identity)> {
    let resolved = lexical_windows_path(root_path)?;
    if resolved.parent().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "drive roots cannot be deleted",
        ));
    }
    if protected_scope_intersects(&resolved, &protected.scopes) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "selected folder intersects a protected location",
        ));
    }
    let parent = resolved
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    let name = resolved
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing folder name"))?;
    let parent_index = protected.lock_directory(parent, false)?;
    // A volume may have multiple DOS names. Compare identities of every
    // selected ancestor as well as lexical paths against protected roots.
    for (path, handle, _) in &protected.handles {
        if !path_prefix(path, parent) {
            continue;
        }
        let ancestor_id = identity(handle)?.0;
        if protected
            .identities
            .iter()
            .zip(&protected.scopes)
            .any(|(id, (_, descendants))| *descendants && *id == ancestor_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "selected folder is inside a protected location",
            ));
        }
    }
    let root = open_child(&protected.handles[parent_index].1, name, true, true, false)?;
    let opened_path = final_path(&root)?;
    if !path_prefix(&resolved, &opened_path) || !path_prefix(&opened_path, &resolved) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "selected folder path changed",
        ));
    }
    let (actual, attrs) = identity(&root)?;
    if actual.index == 0
        || attrs & ATTRIBUTE_REPARSE_POINT != 0
        || attrs & ATTRIBUTE_DIRECTORY == 0
        || protected.identities.contains(&actual)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "selected folder identity is unsafe or changed",
        ));
    }
    Ok((root, opened_path, actual))
}

/// Prepare every handle before deleting anything. Root identity must have been
/// independently captured at authorization time. Protected root handles must
/// be acquired before this call and stay pinned throughout deletion.
#[cfg(test)]
fn prepare(
    root_path: &Path,
    expected: Identity,
    mut protected: ProtectedRoots,
) -> io::Result<PreparedTree> {
    let (root, opened_path, actual) = open_selected(root_path, &mut protected)?;
    if actual != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "selected folder identity changed",
        ));
    }
    prepare_opened(root, opened_path, actual, protected, false)
}

/// Call only after consuming the capability which owned this exact root.
pub(crate) fn prepare_authorized(target: AuthorizedRoot) -> io::Result<PreparedTree> {
    let AuthorizedRoot { root, protected, path, id } = target;
    let (current, attrs) = identity(&root)?;
    if current != id || attrs & ATTRIBUTE_REPARSE_POINT != 0 || attrs & ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "authorized folder identity changed"));
    }
    prepare_opened(root, path, id, protected, true)
}

#[cfg(test)]
pub(crate) fn prepare_fixture_authorized(target: AuthorizedRoot) -> io::Result<PreparedTree> {
    prepare_opened(target.root, target.path, target.id, target.protected, false)
}

fn prepare_opened(root: File, opened_path: PathBuf, actual: Identity, protected: ProtectedRoots, enforce_user_policy: bool) -> io::Result<PreparedTree> {
    let policy = enforce_user_policy.then(crate::duplicate_finder::safety::RemovalPolicy::load);
    prepare_opened_with_policy(root, opened_path, actual, protected, enforce_user_policy, policy)
}

fn prepare_opened_with_policy(root: File, opened_path: PathBuf, actual: Identity, protected: ProtectedRoots, enforce_user_policy: bool, policy: Option<crate::duplicate_finder::safety::RemovalPolicy>) -> io::Result<PreparedTree> {
    let expected = actual;
    let root_owner = policy.as_ref().map(|policy| check_owned_target(policy, &opened_path, &root)).transpose()?;
    let mut pinned = vec![Pinned {
        file: root,
        is_dir: true,
        owner: root_owner,
    }];
    let mut pending = vec![(0usize, 0usize, opened_path.clone())];
    let mut directory_snapshots = Vec::new();
    while let Some((index, depth, parent_path)) = pending.pop() {
        if depth >= MAX_DEPTH {
            return Err(io::Error::other("folder exceeds the safe depth limit"));
        }
        let mut children = list_directory(&pinned[index].file)?;
        children.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        directory_snapshots.push((index, children.clone()));
        for child in children {
            if pinned.len() >= MAX_ENTRIES {
                return Err(io::Error::other("folder exceeds the safe deletion limit"));
            }
            let file = open_child(&pinned[index].file, &child.name, child.is_dir, true, false)?;
            let child_path = parent_path.join(&child.name);
            let (id, attrs) = identity(&file)?;
            if child.index == 0
                || id.volume != expected.volume
                || id.index != child.index
                || attrs & ATTRIBUTE_REPARSE_POINT != 0
                || (attrs & ATTRIBUTE_DIRECTORY != 0) != child.is_dir
                || protected.identities.contains(&id)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "folder contents changed or contain a protected item",
                ));
            }
            if !child.is_dir { require_single_file_link(&file)?; }
            let owner = policy.as_ref().map(|policy| check_owned_target(policy, &child_path, &file)).transpose()?;
            let child_index = pinned.len();
            pinned.push(Pinned {
                file,
                is_dir: child.is_dir,
                owner,
            });
            if child.is_dir {
                pending.push((child_index, depth + 1, child_path));
            }
        }
    }
    Ok(PreparedTree {
        entries: pinned,
        directory_snapshots,
        protected,
        authorization_path: opened_path,
        authorization_identity: actual,
        enforce_user_policy,
    })
}

fn check_owned_target(policy: &crate::duplicate_finder::safety::RemovalPolicy, path: &Path, handle: &File) -> io::Result<crate::duplicate_finder::verified_file::OwnerClass> {
    if let Some(reason) = policy.protected_reason(path) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, reason));
    }
    let owner = crate::duplicate_finder::verified_file::classify_owner(handle)
        .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
    match owner {
        crate::duplicate_finder::verified_file::OwnerClass::CurrentUser | crate::duplicate_finder::verified_file::OwnerClass::SharedAdministrators => Ok(owner),
        crate::duplicate_finder::verified_file::OwnerClass::Other => Err(io::Error::new(io::ErrorKind::PermissionDenied, "refused removal: file is not owned by the current user")),
    }
}

fn owner_unchanged(expected: Option<crate::duplicate_finder::verified_file::OwnerClass>, actual: crate::duplicate_finder::verified_file::OwnerClass) -> bool {
    use crate::duplicate_finder::verified_file::OwnerClass;
    expected == Some(actual) && actual != OwnerClass::Other
}

#[test]
fn confirmed_ownership_must_remain_unchanged() {
    use crate::duplicate_finder::verified_file::OwnerClass::{CurrentUser, SharedAdministrators, Other};
    assert!(owner_unchanged(Some(CurrentUser), CurrentUser));
    assert!(owner_unchanged(Some(SharedAdministrators), SharedAdministrators));
    assert!(!owner_unchanged(Some(CurrentUser), SharedAdministrators));
    assert!(!owner_unchanged(Some(SharedAdministrators), CurrentUser));
    assert!(!owner_unchanged(Some(Other), Other));
    assert!(!owner_unchanged(None, CurrentUser));
}

#[derive(Debug)]
pub(crate) struct DeleteFailure {
    pub processed: usize,
    pub error: io::Error,
}

/// Request deletion only for objects whose handles were pinned during preparation.
/// Counts mean handles processed, not bytes or physical space reclaimed. A failure
/// may follow earlier requests; callers must report partial results and invalidate
/// any displayed scan cache even when an error is returned.
pub(crate) fn delete_prepared(mut tree: PreparedTree) -> Result<usize, DeleteFailure> {
    // Keep protected handles alive until every deletion has finished.
    let _protected_count = tree.protected.handles.len();
    // Recheck the held directory handles before any unlink. This catches
    // changes since confirmation, but cannot prevent later concurrent writes.
    for (index, expected) in &tree.directory_snapshots {
        let current = list_directory(&tree.entries[*index].file)
            .map_err(|error| DeleteFailure { processed: 0, error })?;
        if !same_directory_contents(expected, current) {
            return Err(DeleteFailure { processed: 0, error: io::Error::new(io::ErrorKind::PermissionDenied, "folder contents changed after confirmation") });
        }
    }
    for entry in &tree.entries {
        if !entry.is_dir {
            require_single_file_link(&entry.file).map_err(|error| DeleteFailure { processed: 0, error })?;
        }
    }
    if tree.enforce_user_policy {
        for entry in &tree.entries {
            let owner = crate::duplicate_finder::verified_file::classify_owner(&entry.file)
                .map_err(|message| DeleteFailure { processed: 0, error: io::Error::new(io::ErrorKind::PermissionDenied, message) })?;
            if !owner_unchanged(entry.owner, owner) {
                return Err(DeleteFailure { processed: 0, error: io::Error::new(io::ErrorKind::PermissionDenied, "folder ownership changed after confirmation") });
            }
        }
    }
    let mut processed = 0usize;
    while let Some(Pinned { file, is_dir, owner }) = tree.entries.pop() {
        if !is_dir {
            require_single_file_link(&file).map_err(|error| DeleteFailure { processed, error })?;
        }
        if tree.enforce_user_policy {
            let current = crate::duplicate_finder::verified_file::classify_owner(&file)
                .map_err(|message| DeleteFailure { processed, error: io::Error::new(io::ErrorKind::PermissionDenied, message) })?;
            if !owner_unchanged(owner, current) {
                return Err(DeleteFailure { processed, error: io::Error::new(io::ErrorKind::PermissionDenied, "folder ownership changed after confirmation") });
            }
        }
        let disposition: u8 = 1; // FILE_DISPOSITION_INFO.DeleteFile
        // Unlink the verified handle on close even when another process has a
        // delete-sharing handle open. Keep Windows' image-section check.
        let extended_disposition: u32 = 0x1 | 0x2 | 0x4;
        let mut retries = 0;
        loop {
            let extended = unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(), FILE_DISPOSITION_INFO_EX_CLASS,
                    (&extended_disposition as *const u32).cast(), 4,
                )
            };
            let extended_error = if extended == 0 { Some(io::Error::last_os_error()) } else { None };
            let legacy = extended_error.as_ref().is_some_and(|error| matches!(error.raw_os_error(), Some(1 | 50 | 87)));
            if extended != 0 || (legacy && unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FILE_DISPOSITION_INFO_CLASS,
                    (&disposition as *const u8).cast(),
                    1,
                )
            } != 0)
            {
                break;
            }
            let error = if legacy { io::Error::last_os_error() } else { extended_error.unwrap() };
            // A child marked for deletion can remain pending briefly after its
            // handle closes. Never retry indefinitely when a new child exists.
            if !is_dir || error.raw_os_error() != Some(145) || retries >= 20 {
                return Err(DeleteFailure { processed, error });
            }
            retries += 1;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(file);
        processed += 1;
    }
    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let configured = std::env::var_os("WINCOMMANDER_DISK_TREE_TEST_BASE")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir);
            let base = std::fs::canonicalize(configured).unwrap();
            let root = base.join(format!(
                "fixture-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self { base, root }
        }
        fn target(&self) -> PathBuf {
            self.root.join("selected")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Ok(resolved) = std::fs::canonicalize(&self.root) {
                assert!(resolved.parent() == Some(self.base.as_path()));
                assert!(path_prefix(&resolved, &self.root) && path_prefix(&self.root, &resolved));
                std::fs::remove_dir_all(resolved).unwrap();
            }
        }
    }
    fn selected_identity(path: &Path) -> Identity {
        identity(&open_root(path).unwrap()).unwrap().0
    }

    #[test]
    #[ignore = "read-only host discovery requires administrator backup privilege"]
    fn administrator_can_pin_host_protected_roots() {
        let roots = ProtectedRoots::system().expect("protected-root discovery must fail closed");
        assert!(!roots.handles.is_empty());
    }

    #[test]
    fn target_traversal_does_not_reuse_backup_origin_handles() {
        let fixture = Fixture::new();
        let mut roots = ProtectedRoots::pin_inner(&[(fixture.root.clone(), false)], true).unwrap();
        let backup = roots.lock_directory(&fixture.root, true).unwrap();
        let normal = roots.lock_directory(&fixture.root, false).unwrap();
        assert_ne!(backup, normal);
        assert!(roots.handles[backup].2);
        assert!(!roots.handles[normal].2);
        assert_eq!(identity(&roots.handles[backup].1).unwrap().0, identity(&roots.handles[normal].1).unwrap().0);
    }

    #[test]
    fn protected_scope_checks_ancestors_descendants_and_case() {
        let roots = vec![
            (PathBuf::from(r"C:\Windows"), true),
            (PathBuf::from(r"C:\Users\person"), false),
        ];
        assert!(protected_scope_intersects(
            Path::new(r"c:\windows\System32"),
            &roots
        ));
        assert!(protected_scope_intersects(Path::new(r"C:\"), &roots));
        assert!(protected_scope_intersects(
            Path::new(r"C:\Users\PERSON"),
            &roots
        ));
        assert!(!protected_scope_intersects(
            Path::new(r"C:\Users\person\Downloads"),
            &roots
        ));
        assert!(!protected_scope_intersects(
            Path::new(r"C:\Windows.old"),
            &roots
        ));
    }

    #[test]
    fn rejects_network_and_device_namespaces() {
        assert!(lexical_windows_path(Path::new(r"\\server\share\folder")).is_err());
        assert!(lexical_windows_path(Path::new(
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\folder"
        ))
        .is_err());
        assert!(lexical_windows_path(Path::new(r"C:\safe\..\folder")).is_err());
    }

    #[test]
    fn protected_ancestor_identity_blocks_differently_named_alias() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let expected = selected_identity(&target);
        let mut protected = ProtectedRoots::pin(&[(f.root.clone(), true)]).unwrap();
        // Simulate a protected path registered under a different drive spelling
        // while retaining its real file identity and held handle.
        protected.scopes[0].0 = PathBuf::from(r"Q:\unrelated-alias");
        assert!(prepare(&target, expected, protected).is_err());
        assert!(target.exists());
    }

    #[test]
    #[ignore = "read-only host compatibility check; opens protected paths briefly"]
    fn current_windows_protected_roots_can_be_pinned_without_delete_access() {
        let pinned =
            ProtectedRoots::system().expect("protected directories could not all be safely pinned");
        assert!(!pinned.identities.is_empty());
    }

    #[test]
    fn read_attributes_handle_does_not_pin_directory_name_on_this_windows() {
        let f = Fixture::new();
        let target = f.target();
        let renamed = f.root.join("renamed");
        std::fs::create_dir(&target).unwrap();
        let handle = OpenOptions::new()
            .access_mode(READ_ATTRIBUTES)
            .share_mode(SHARE_READ_WRITE)
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
            .open(&target)
            .unwrap();
        std::fs::rename(&target, &renamed)
            .expect("read-attributes-only pin did not prevent a directory rename");
        drop(handle);
        assert!(renamed.exists());
    }

    #[test]
    fn probe_directory_handle_access_and_rename() {
        let f = Fixture::new();
        let target = f.target();
        let renamed = f.root.join("renamed");
        for (label, access) in [
            ("attributes", READ_ATTRIBUTES),
            ("list", LIST_DIRECTORY),
            ("traverse", 0x20),
            ("read-control", 0x0002_0000),
            ("write-attributes", 0x100),
            ("delete", DELETE),
        ] {
            std::fs::create_dir(&target).unwrap();
            let handle = OpenOptions::new()
                .access_mode(access)
                .share_mode(SHARE_READ_WRITE)
                .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
                .open(&target);
            match handle {
                Ok(handle) => {
                    let rename = std::fs::rename(&target, &renamed);
                    println!("{label}: rename_blocked={}", rename.is_err());
                    drop(handle);
                    if rename.is_ok() {
                        std::fs::rename(&renamed, &target).unwrap();
                    }
                }
                Err(error) => println!("{label}: open_error={:?}", error.raw_os_error()),
            }
            std::fs::remove_dir(&target).unwrap();
        }
    }

    #[test]
    fn authorized_root_pins_confirmed_identity_until_preparation() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("item"), b"fixture").unwrap();
        let expected_id = selected_identity(&target);
        let protected = ProtectedRoots::pin(&[]).unwrap();
        let authority = AuthorizedRoot::open(&target, protected).unwrap();
        let (confirmed_path, confirmed_id) = authority.target();
        assert!(path_prefix(confirmed_path, &target) && path_prefix(&target, confirmed_path));
        assert_eq!(confirmed_id, expected_id);
        assert!(std::fs::rename(&target, f.root.join("replacement")).is_err());
        // This handle-lifetime test uses the system temp directory; production
        // policy separately rejects app-data locations before mutation.
        let prepared = prepare_opened(authority.root, authority.path, authority.id, authority.protected, false).unwrap();
        assert_eq!(prepared.authorization_target().1, confirmed_id);
        assert_eq!(delete_prepared(prepared).unwrap(), 2);
        assert!(!target.exists());
    }

    #[test]
    fn production_preflight_rejects_protected_descendant_before_any_deletion() {
        // Run only when the caller supplies a disposable, non-AppData base.
        if std::env::var_os("WINCOMMANDER_DISK_TREE_TEST_BASE").is_none() { return; }
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("safe.txt"), b"keep").unwrap();
        std::fs::write(target.join("protected.exe"), b"keep").unwrap();
        let root = AuthorizedRoot::open(&target, ProtectedRoots::fixture(&f.root).unwrap()).unwrap();
        let error = match prepare_authorized(root) { Ok(_) => panic!("protected executable was accepted"), Err(error) => error };
        let identity_available = crate::duplicate_finder::profile_policy::profiles().is_ok();
        let expected = if identity_available { "application, executable or codec file is protected" } else { "user profile locations could not be verified" };
        assert_eq!(error.to_string(), expected);
        assert!(target.join("safe.txt").exists());
        assert!(target.join("protected.exe").exists());
    }

    #[test]
    fn controlled_profile_preflight_rejects_executable_before_deletion() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("safe.txt"), b"keep").unwrap();
        std::fs::write(target.join("protected.exe"), b"keep").unwrap();
        let root = AuthorizedRoot::open(&target, ProtectedRoots::fixture(&f.root).unwrap()).unwrap();
        let policy = crate::duplicate_finder::safety::RemovalPolicy::fixture(f.root.clone());
        let error = match prepare_opened_with_policy(root.root, root.path, root.id, root.protected, true, Some(policy)) {
            Ok(_) => panic!("protected executable was accepted"), Err(error) => error,
        };
        assert_eq!(error.to_string(), "application, executable or codec file is protected");
        assert_eq!(std::fs::read(target.join("safe.txt")).unwrap(), b"keep");
        assert_eq!(std::fs::read(target.join("protected.exe")).unwrap(), b"keep");
    }

    #[test]
    fn removes_nested_tree_with_file_at_each_level() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir_all(target.join("nested")).unwrap();
        std::fs::write(target.join("first.txt"), b"first").unwrap();
        std::fs::write(target.join("nested").join("second.txt"), b"second").unwrap();
        // Allow filesystem watchers to observe the folders, as in the app.
        std::thread::sleep(std::time::Duration::from_secs(2));
        let expected = selected_identity(&target);
        let handles = prepare(&target, expected, ProtectedRoots::pin(&[]).unwrap()).unwrap();
        delete_prepared(handles).unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn deletes_nested_tree_with_a_shared_directory_handle() {
        let f = Fixture::new();
        let target = f.target();
        let nested = target.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("second.txt"), b"second").unwrap();
        let watcher = OpenOptions::new()
            .access_mode(READ_ATTRIBUTES | LIST_DIRECTORY | SYNCHRONIZE)
            .share_mode(SHARE_READ_WRITE | 0x4)
            .custom_flags(BACKUP_SEMANTICS)
            .open(&nested).unwrap();
        let expected = selected_identity(&target);
        let handles = prepare(&target, expected, ProtectedRoots::pin(&[]).unwrap()).unwrap();
        let result = delete_prepared(handles);
        drop(watcher);
        result.unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn removes_only_the_pinned_selected_tree() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(target.join("nested")).unwrap();
        std::fs::write(target.join("nested").join("item.txt"), b"selected").unwrap();
        std::fs::write(f.root.join("outside.txt"), b"preserved").unwrap();
        let expected = selected_identity(&target);
        let handles = prepare(&target, expected, ProtectedRoots::pin(&[]).unwrap()).unwrap();
        assert!(std::fs::rename(&target, f.root.join("renamed")).is_err());
        assert!(std::fs::rename(
            target.join("nested").join("item.txt"),
            target.join("replacement.txt")
        )
        .is_err());
        delete_prepared(handles).unwrap();
        assert!(!target.exists());
        assert_eq!(
            std::fs::read(f.root.join("outside.txt")).unwrap(),
            b"preserved"
        );
    }

    #[test]
    fn identity_replacement_is_rejected_before_any_deletion() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let expected = selected_identity(&target);
        std::fs::rename(&target, f.root.join("original")).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("replacement.txt"), b"keep").unwrap();
        assert!(prepare(&target, expected, ProtectedRoots::pin(&[]).unwrap()).is_err());
        assert!(target.join("replacement.txt").exists());
    }

    #[test]
    fn rejects_hard_link_before_deleting_anything() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let original = f.root.join("outside.txt");
        std::fs::write(&original, b"preserve").unwrap();
        std::fs::hard_link(&original, target.join("alias.txt")).unwrap();
        std::fs::write(target.join("safe.txt"), b"keep").unwrap();
        let error = match prepare(&target, selected_identity(&target), ProtectedRoots::pin(&[]).unwrap()) {
            Ok(_) => panic!("hard link accepted"), Err(error) => error,
        };
        assert_eq!(error.to_string(), "multiply linked files cannot be deleted");
        assert_eq!(std::fs::read(&original).unwrap(), b"preserve");
        assert_eq!(std::fs::read(target.join("safe.txt")).unwrap(), b"keep");
    }

    #[test]
    fn hard_link_after_preparation_cannot_lead_to_deletion() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let original = target.join("item.txt");
        let alias = f.root.join("late-alias.txt");
        std::fs::write(&original, b"preserve").unwrap();
        let tree = prepare(&target, selected_identity(&target), ProtectedRoots::pin(&[]).unwrap()).unwrap();
        match std::fs::hard_link(&original, &alias) {
            Ok(()) => {
                let failure = delete_prepared(tree).unwrap_err();
                assert_eq!(failure.processed, 0);
                assert_eq!(failure.error.to_string(), "multiply linked files cannot be deleted");
                assert_eq!(std::fs::read(&alias).unwrap(), b"preserve");
            }
            Err(error) => {
                // These Windows errors mean access or sharing blocked the change.
                assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)), "unexpected hard-link failure: {error}");
                drop(tree);
            }
        }
        assert_eq!(std::fs::read(&original).unwrap(), b"preserve");
    }

    #[test]
    fn protected_descendant_is_rejected_before_any_deletion() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(target.join("protected")).unwrap();
        std::fs::write(target.join("other.txt"), b"keep").unwrap();
        let expected = selected_identity(&target);
        let protected = ProtectedRoots::pin(&[(target.join("protected"), false)]).unwrap();
        assert!(std::fs::rename(target.join("protected"), f.root.join("moved-protected")).is_err());
        assert!(prepare(&target, expected, protected).is_err());
        assert!(target.join("other.txt").exists());
    }

    #[test]
    fn duplicate_protected_roots_merge_to_strictest_scope() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let expected = selected_identity(&target);
        let protected =
            ProtectedRoots::pin(&[(f.root.clone(), false), (f.root.clone(), true)]).unwrap();
        assert_eq!(protected.identities.len(), 1);
        assert!(prepare(&target, expected, protected).is_err());
        assert!(target.exists());
    }

    #[test]
    fn user_root_allows_selected_child_but_ancestor_is_pinned() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let expected = selected_identity(&target);
        let protected = ProtectedRoots::pin(&[(f.root.clone(), false)]).unwrap();
        let prepared = prepare(&target, expected, protected).unwrap();
        let moved = f.root.with_extension("moved");
        assert!(!moved.exists());
        assert!(std::fs::rename(&f.root, moved).is_err());
        delete_prepared(prepared).unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn rejects_directory_links_without_touching_target() {
        let f = Fixture::new();
        let target = f.target();
        let outside = f.root.join("outside");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("preserve.txt"), b"keep").unwrap();
        std::os::windows::fs::symlink_dir(&outside, target.join("link"))
            .expect("directory symlink test requires Developer Mode or administrator rights");
        assert!(prepare(
            &target,
            selected_identity(&target),
            ProtectedRoots::pin(&[]).unwrap()
        )
        .is_err());
        assert!(outside.join("preserve.txt").exists());
    }

    #[test]
    fn rejects_selected_root_symlink_without_following_it() {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("preserve.txt"), b"keep").unwrap();
        std::os::windows::fs::symlink_dir(&outside, f.target()).unwrap();
        let expected = selected_identity(&outside);
        assert!(prepare(&f.target(), expected, ProtectedRoots::pin(&[]).unwrap()).is_err());
        assert!(outside.join("preserve.txt").exists());
    }

    #[test]
    fn rejects_selected_root_junction_without_following_it() {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("preserve.txt"), b"keep").unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(f.target())
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());
        let expected = selected_identity(&outside);
        assert!(prepare(&f.target(), expected, ProtectedRoots::pin(&[]).unwrap()).is_err());
        assert!(outside.join("preserve.txt").exists());
    }

    #[test]
    fn rejects_selected_path_with_linked_ancestor() {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        let selected = outside.join("selected");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(selected.join("preserve.txt"), b"keep").unwrap();
        let link = f.root.join("ancestor-link");
        std::os::windows::fs::symlink_dir(&outside, &link).unwrap();
        assert!(prepare(
            &link.join("selected"),
            selected_identity(&selected),
            ProtectedRoots::pin(&[]).unwrap()
        )
        .is_err());
        assert!(selected.join("preserve.txt").exists());
    }

    #[test]
    fn rejects_junctions_without_touching_target() {
        let f = Fixture::new();
        let target = f.target();
        let outside = f.root.join("outside");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("preserve.txt"), b"keep").unwrap();
        let link = target.join("junction");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success(), "junction creation failed");
        assert!(prepare(
            &target,
            selected_identity(&target),
            ProtectedRoots::pin(&[]).unwrap()
        )
        .is_err());
        assert!(outside.join("preserve.txt").exists());
    }

    #[test]
    fn new_unreviewed_child_is_rejected_before_any_deletion() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("initial.txt"), b"initial").unwrap();
        let handles = prepare(
            &target,
            selected_identity(&target),
            ProtectedRoots::pin(&[]).unwrap(),
        )
        .unwrap();
        std::fs::write(target.join("added.txt"), b"preserve").unwrap();
        let failure = delete_prepared(handles).unwrap_err();
        assert_eq!(failure.processed, 0);
        assert_eq!(failure.error.to_string(), "folder contents changed after confirmation");
        assert_eq!(std::fs::read(target.join("initial.txt")).unwrap(), b"initial");
        assert!(target.join("added.txt").exists());
    }

    #[test]
    fn new_nested_child_is_rejected_before_any_deletion() {
        let f = Fixture::new();
        let target = f.target();
        let nested = target.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(target.join("initial.txt"), b"initial").unwrap();
        let handles = prepare(&target, selected_identity(&target), ProtectedRoots::pin(&[]).unwrap()).unwrap();
        std::fs::write(nested.join("added.txt"), b"preserve").unwrap();
        let failure = delete_prepared(handles).unwrap_err();
        assert_eq!(failure.processed, 0);
        assert_eq!(failure.error.to_string(), "folder contents changed after confirmation");
        assert_eq!(std::fs::read(target.join("initial.txt")).unwrap(), b"initial");
        assert_eq!(std::fs::read(nested.join("added.txt")).unwrap(), b"preserve");
    }

    #[test]
    fn directory_snapshot_comparison_detects_removed_replaced_and_renamed_entries() {
        let original = ListedEntry { name: OsString::from("item"), is_dir: false, index: 42 };
        let expected = vec![original.clone()];
        assert!(same_directory_contents(&expected, vec![original.clone()]));
        assert!(!same_directory_contents(&expected, vec![]));
        assert!(!same_directory_contents(&expected, vec![ListedEntry { index: 43, ..original.clone() }]));
        assert!(!same_directory_contents(&expected, vec![ListedEntry { name: OsString::from("renamed"), ..original.clone() }]));
        assert!(!same_directory_contents(&expected, vec![ListedEntry { is_dir: true, ..original }]));
    }

    #[test]
    fn held_file_blocks_rename_and_removal_after_preparation() {
        let f = Fixture::new();
        let target = f.target();
        std::fs::create_dir(&target).unwrap();
        let original = target.join("item.txt");
        std::fs::write(&original, b"preserve").unwrap();
        let tree = prepare(&target, selected_identity(&target), ProtectedRoots::pin(&[]).unwrap()).unwrap();
        for error in [std::fs::rename(&original, target.join("renamed.txt")).unwrap_err(), std::fs::remove_file(&original).unwrap_err()] {
            assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)), "unexpected namespace failure: {error}");
        }
        drop(tree);
        assert_eq!(std::fs::read(&original).unwrap(), b"preserve");
    }
}
