// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::engine::UNVERIFIED;
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
};

struct Module(HMODULE);
impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

fn verified_registration(result: i32, registered: i32) -> Result<bool, String> {
    if result != 0 {
        return Err(UNVERIFIED.into());
    }
    Ok(registered != 0)
}

pub(super) fn registered() -> Result<bool, String> {
    let name: Vec<u16> = "MDMRegistration.dll\0".encode_utf16().collect();
    let module = unsafe {
        LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(UNVERIFIED.into());
    }
    let module = Module(module);
    let address = unsafe {
        GetProcAddress(
            module.0,
            c"IsDeviceRegisteredWithManagement".as_ptr().cast(),
        )
    }
    .ok_or(UNVERIFIED)?;
    type Query = unsafe extern "system" fn(*mut i32, u32, *mut u16) -> i32;
    let query =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(address) };
    let mut registered = 0;
    // No UPN buffer: only the management flag is needed, never an identity.
    let result = unsafe { query(&mut registered, 0, std::ptr::null_mut()) };
    verified_registration(result, registered)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_failed_management_query_never_means_unmanaged() {
        for registered in [0, 1] {
            assert!(super::verified_registration(-1, registered).is_err());
        }
        assert!(!super::verified_registration(0, 0).unwrap());
        assert!(super::verified_registration(0, 1).unwrap());
    }
}
