// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::{Directory, ObjectAttributes};
use std::mem::size_of;
use windows_sys::Win32::{
    Foundation::{HANDLE, UNICODE_STRING},
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};

type OpenLink = unsafe extern "system" fn(*mut HANDLE, u32, *mut ObjectAttributes) -> i32;
type QueryLink = unsafe extern "system" fn(HANDLE, *mut UNICODE_STRING, *mut u32) -> i32;
type MakeTemporary = unsafe extern "system" fn(HANDLE) -> i32;

pub(super) struct LinkApi {
    open: OpenLink,
    query: QueryLink,
    temporary: MakeTemporary,
}

impl LinkApi {
    pub(super) fn load() -> Result<Self, ()> {
        let dll: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
        unsafe {
            let module = GetModuleHandleW(dll.as_ptr());
            if module.is_null() {
                return Err(());
            }
            let open =
                GetProcAddress(module, c"NtOpenSymbolicLinkObject".as_ptr().cast()).ok_or(())?;
            let query =
                GetProcAddress(module, c"NtQuerySymbolicLinkObject".as_ptr().cast()).ok_or(())?;
            let temporary =
                GetProcAddress(module, c"NtMakeTemporaryObject".as_ptr().cast()).ok_or(())?;
            Ok(Self {
                open: std::mem::transmute::<unsafe extern "system" fn() -> isize, OpenLink>(open),
                query: std::mem::transmute::<unsafe extern "system" fn() -> isize, QueryLink>(
                    query,
                ),
                temporary: std::mem::transmute::<unsafe extern "system" fn() -> isize, MakeTemporary>(
                    temporary,
                ),
            })
        }
    }

    pub(super) fn open(&self, path: &str, delete: bool) -> Result<Option<Directory>, ()> {
        let mut buffer: Vec<u16> = path.encode_utf16().collect();
        let bytes = u16::try_from(buffer.len() * 2).map_err(|_| ())?;
        let mut name = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: buffer.as_mut_ptr(),
        };
        let mut attributes = ObjectAttributes {
            length: size_of::<ObjectAttributes>() as u32,
            root_directory: std::ptr::null_mut(),
            object_name: &mut name,
            attributes: 0x40, // NtOpenSymbolicLinkObject already opens the link itself.
            security_descriptor: std::ptr::null_mut(),
            security_quality_of_service: std::ptr::null_mut(),
        };
        let mut handle = std::ptr::null_mut();
        let access = 1 | if delete { 0x0001_0000 } else { 0 }; // SYMBOLIC_LINK_QUERY | DELETE
        let status = unsafe { (self.open)(&mut handle, access, &mut attributes) };
        if matches!(status as u32, 0xc0000034 | 0xc000003a) {
            return Ok(None);
        }
        if status < 0 || handle.is_null() {
            return Err(());
        }
        Ok(Some(Directory(handle)))
    }

    pub(super) fn target(&self, handle: &Directory) -> Result<String, ()> {
        let mut buffer = [0u16; 1024];
        let mut text = UNICODE_STRING {
            Length: 0,
            MaximumLength: (buffer.len() * 2) as u16,
            Buffer: buffer.as_mut_ptr(),
        };
        let mut required = 0;
        let status = unsafe { (self.query)(handle.0, &mut text, &mut required) };
        if status < 0
            || text.Length % 2 != 0
            || text.Length as usize > buffer.len() * 2
            || text.Buffer != buffer.as_mut_ptr()
        {
            return Err(());
        }
        String::from_utf16(&buffer[..text.Length as usize / 2]).map_err(|_| ())
    }

    pub(super) fn remove(&self, handle: &Directory) -> Result<(), ()> {
        let status = unsafe { (self.temporary)(handle.0) };
        (status >= 0).then_some(()).ok_or(())
    }
}
