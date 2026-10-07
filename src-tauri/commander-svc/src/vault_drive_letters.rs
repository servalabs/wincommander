// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded inventory of machine and every logon's DOS drive names.

#[path = "vault_logon_links.rs"]
mod logon_links;
pub(crate) use logon_links::{
    cleanup_ended_logon_encrypted_link, ended_logon_encrypted_link_absent, logon_ended,
    release_orphaned_caller_encrypted_links, release_orphaned_logon_encrypted_links,
};

use std::{collections::HashSet, ffi::c_void, mem::size_of};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, UNICODE_STRING},
    Storage::FileSystem::QueryDosDeviceW,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *mut UNICODE_STRING,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[repr(C)]
struct DirectoryEntry {
    name: UNICODE_STRING,
    kind: UNICODE_STRING,
}

type OpenDirectory = unsafe extern "system" fn(*mut HANDLE, u32, *mut ObjectAttributes) -> i32;
type QueryDirectory =
    unsafe extern "system" fn(HANDLE, *mut c_void, u32, u8, u8, *mut u32, *mut u32) -> i32;

struct Directory(HANDLE);
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct Api {
    open: OpenDirectory,
    query: QueryDirectory,
}

impl Api {
    fn load() -> Result<Self, ()> {
        let name: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
        // ntdll is process-lifetime loaded by Windows; no external DLL is searched.
        unsafe {
            let module = GetModuleHandleW(name.as_ptr());
            if module.is_null() {
                return Err(());
            }
            let open = GetProcAddress(module, c"NtOpenDirectoryObject".as_ptr().cast()).ok_or(())?;
            let query =
                GetProcAddress(module, c"NtQueryDirectoryObject".as_ptr().cast()).ok_or(())?;
            Ok(Self {
                open: std::mem::transmute::<unsafe extern "system" fn() -> isize, OpenDirectory>(
                    open,
                ),
                query: std::mem::transmute::<unsafe extern "system" fn() -> isize, QueryDirectory>(
                    query,
                ),
            })
        }
    }

    fn entries(&self, path: &str, vanished_ok: bool) -> Result<Vec<(String, String)>, ()> {
        let mut name: Vec<u16> = path.encode_utf16().collect();
        let bytes = u16::try_from(name.len() * 2).map_err(|_| ())?;
        let mut unicode = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: name.as_mut_ptr(),
        };
        let mut attributes = ObjectAttributes {
            length: size_of::<ObjectAttributes>() as u32,
            root_directory: std::ptr::null_mut(),
            object_name: &mut unicode,
            attributes: 0x40, // OBJ_CASE_INSENSITIVE
            security_descriptor: std::ptr::null_mut(),
            security_quality_of_service: std::ptr::null_mut(),
        };
        let mut handle = std::ptr::null_mut();
        // All pointers remain valid for the synchronous call; access is query-only.
        let status = unsafe { (self.open)(&mut handle, 1, &mut attributes) };
        if vanished_ok && matches!(status as u32, 0xc0000034 | 0xc000003a) {
            return Ok(Vec::new());
        }
        if status < 0 {
            return Err(());
        }
        let handle = Directory(handle);
        let mut context = 0;
        let mut entries = Vec::new();
        // An aligned buffer also bounds kernel-returned names and parser work.
        let mut buffer = [0usize; 2048];
        for index in 0..8192 {
            let mut returned = 0;
            let status = unsafe {
                (self.query)(
                    handle.0,
                    buffer.as_mut_ptr().cast(),
                    size_of_val(&buffer) as u32,
                    1,
                    u8::from(index == 0),
                    &mut context,
                    &mut returned,
                )
            };
            if status as u32 == 0x8000001a {
                return Ok(entries);
            } // STATUS_NO_MORE_ENTRIES
            if status < 0
                || returned < size_of::<DirectoryEntry>() as u32
                || returned as usize > size_of_val(&buffer)
            {
                return Err(());
            }
            // The buffer has DirectoryEntry alignment and its full header was checked.
            let entry = unsafe { &*buffer.as_ptr().cast::<DirectoryEntry>() };
            entries.push((
                bounded_string(&entry.name, &buffer, returned as usize)?,
                bounded_string(&entry.kind, &buffer, returned as usize)?,
            ));
        }
        Err(())
    }
}

fn bounded_string(value: &UNICODE_STRING, buffer: &[usize], returned: usize) -> Result<String, ()> {
    let start = buffer.as_ptr() as usize;
    let pointer = value.Buffer as usize;
    let length = value.Length as usize;
    if length % 2 != 0
        || pointer % 2 != 0
        || pointer < start
        || pointer.checked_add(length).ok_or(())? > start.checked_add(returned).ok_or(())?
    {
        return Err(());
    }
    // Both the range and UTF-16 alignment were checked against the live buffer.
    String::from_utf16(unsafe { std::slice::from_raw_parts(value.Buffer, length / 2) })
        .map_err(|_| ())
}

fn add_drive_names(letters: &mut HashSet<String>, entries: Vec<(String, String)>) {
    for (name, _) in entries {
        let bytes = name.as_bytes();
        if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            letters.insert((bytes[0].to_ascii_uppercase() as char).to_string());
        }
    }
}

pub(crate) fn occupied_letters() -> Result<HashSet<String>, ()> {
    let api = Api::load()?;
    let mut letters = HashSet::new();
    add_drive_names(&mut letters, api.entries("\\GLOBAL??", false)?);
    // Every logon's DOS namespace lives here, including split UAC and disconnected
    // sessions. Enumerate names only, never read link targets or expose identities.
    for (logon, kind) in api.entries("\\Sessions\\0\\DosDevices", false)? {
        if kind != "Directory" {
            continue;
        }
        if logon.is_empty() || logon.len() > 128 || logon.contains(['\\', '/']) {
            return Err(());
        }
        add_drive_names(
            &mut letters,
            api.entries(&format!("\\Sessions\\0\\DosDevices\\{logon}"), true)?,
        );
    }
    Ok(letters)
}

fn global_dos_name(letter: char) -> Vec<u16> {
    format!("Global\\{letter}:")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn global_dos_device_target(letter: char) -> Result<Option<String>, ()> {
    // Query the explicit global namespace.  The SYSTEM service must never
    // infer a caller's per-user mapping while deciding whether a stale
    // machine-wide name can be removed.
    let name = global_dos_name(letter);
    let mut target = vec![0u16; 32_768];
    let length =
        unsafe { QueryDosDeviceW(name.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
    if length == 0 {
        // A missing link is already clean. Other API failures are not guessed
        // at, because this routine is allowed to remove a live system name.
        let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        return if error == windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(())
        };
    }
    let end = target
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(length as usize);
    String::from_utf16(&target[..end]).map(Some).map_err(|_| ())
}

/// Result of checking one explicit global encrypted-volume drive name.  This
/// never inspects or changes a per-user namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobalEncryptedLinkCleanup {
    /// No global link exists for this letter.
    Absent,
    /// A dead, exact encrypted-volume link was removed.
    Removed,
    /// The name is foreign, usable, or could not safely be classified as dead.
    Retained,
}

/// Returns true only when the exact global DOS name is absent.  This is used
/// by durable-mount recovery as a read-only proof; unlike the repair action it
/// never removes a mapping based on a drive letter alone.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn global_drive_letter_absent(letter: char) -> Result<bool, ()> {
    Ok(global_dos_device_target(letter)?.is_none())
}

/// Confirm that one global encrypted-volume letter is absent, removing it
/// only when Windows still reports the exact dead VeraCrypt/TrueCrypt target.
/// A live/foreign mapping or an uncertain native-device check is retained.
pub(crate) fn cleanup_global_encrypted_link(letter: char) -> Result<GlobalEncryptedLinkCleanup, ()> {
    logon_links::cleanup_global_link(letter)
}

/// Remove only dead global VeraCrypt/TrueCrypt drive names.
/// Names are pinned until removal and the encrypted device must be absent.
/// Filesystem access failures are never treated as evidence that a drive is dead.
fn release_orphaned_global_encrypted_links_with(
    mut cleanup: impl FnMut(char) -> Result<GlobalEncryptedLinkCleanup, ()>,
) -> Result<usize, ()> {
    let mut released: usize = 0;
    for byte in b'A'..=b'Z' {
        let letter = char::from(byte);
        // An unreadable mapping stays blocked without preventing later repairs.
        if cleanup(letter) == Ok(GlobalEncryptedLinkCleanup::Removed) {
            released = released.saturating_add(1);
        }
    }
    Ok(released)
}

pub(crate) fn release_orphaned_global_encrypted_links() -> Result<usize, ()> {
    release_orphaned_global_encrypted_links_with(
        cleanup_global_encrypted_link,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unions_only_drive_letters_without_exposing_namespace_names() {
        let mut letters = HashSet::new();
        add_drive_names(
            &mut letters,
            vec![
                ("C:".into(), "SymbolicLink".into()),
                ("Volume{private}".into(), "SymbolicLink".into()),
            ],
        );
        add_drive_names(
            &mut letters,
            vec![
                ("p:".into(), "SymbolicLink".into()),
                ("C:".into(), "SymbolicLink".into()),
                ("COM1".into(), "SymbolicLink".into()),
            ],
        );
        assert_eq!(letters, HashSet::from(["C".into(), "P".into()]));
    }

    #[test]
    fn global_dos_names_are_explicit_and_nul_terminated() {
        assert_eq!(
            String::from_utf16(&global_dos_name('Y')[..global_dos_name('Y').len() - 1]).unwrap(),
            "Global\\Y:"
        );
        assert_eq!(*global_dos_name('Y').last().unwrap(), 0);
    }

    #[test]
    fn rejects_out_of_buffer_directory_strings() {
        let buffer = [0usize; 8];
        let value = UNICODE_STRING {
            Length: 2,
            MaximumLength: 2,
            Buffer: std::ptr::null_mut(),
        };
        assert!(bounded_string(&value, &buffer, size_of_val(&buffer)).is_err());
    }

    #[test]
    fn bulk_cleanup_keeps_scanning_after_an_uncertain_earlier_letter() {
        let released = release_orphaned_global_encrypted_links_with(|letter| match letter {
            'A' => Ok(GlobalEncryptedLinkCleanup::Retained),
            'B' => Ok(GlobalEncryptedLinkCleanup::Removed),
            _ => Ok(GlobalEncryptedLinkCleanup::Retained),
        }).unwrap();
        assert_eq!(released, 1);
    }

    #[test]
    fn bulk_cleanup_preserves_failed_aliases_and_continues_with_later_letters() {
        let mut seen = Vec::new();
        let result = release_orphaned_global_encrypted_links_with(|letter| {
            seen.push(letter);
            if letter == 'K' { Ok(GlobalEncryptedLinkCleanup::Removed) } else { Err(()) }
        });
        assert_eq!(result, Ok(1));
        assert_eq!(seen, ('A'..='Z').collect::<Vec<_>>());
    }

    #[test]
    #[ignore = "read-only Windows acceptance probe; requires access to all logon DOS namespaces"]
    fn live_read_only_namespace_inventory() {
        let letters = occupied_letters().expect("all namespaces must be readable");
        assert!(letters.contains("C"));
        assert!(letters.iter().all(|letter| letter.len() == 1));
        eprintln!("{} occupied drive letters observed", letters.len());
    }
}
