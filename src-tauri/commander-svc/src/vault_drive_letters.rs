// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, read-only inventory of machine and every logon's DOS drive names.

use std::{collections::HashSet, ffi::c_void, mem::size_of};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, UNICODE_STRING},
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
    #[ignore = "read-only Windows acceptance probe; requires access to all logon DOS namespaces"]
    fn live_read_only_namespace_inventory() {
        let letters = occupied_letters().expect("all namespaces must be readable");
        assert!(letters.contains("C"));
        assert!(letters.iter().all(|letter| letter.len() == 1));
        eprintln!("{} occupied drive letters observed", letters.len());
    }
}
