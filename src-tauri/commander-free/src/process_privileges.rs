// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read the desktop process token without inferring privileges from its account.

#[cfg(windows)]
fn query_elevation(token: windows_sys::Win32::Foundation::HANDLE) -> Result<bool, String> {
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION};
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0;
    let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
    // The output is an initialized, correctly sized buffer; Windows validates the handle.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            size,
            &mut returned,
        )
    };
    if read == 0 || returned != size {
        return Err("Windows could not read this app's elevation status.".into());
    }
    Ok(elevation.TokenIsElevated != 0)
}

#[cfg(windows)]
pub(crate) fn current_process_elevation() -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::TOKEN_QUERY,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    // Query only our own primary token; release the acquired handle on either query result.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err("Windows could not open this app's process token.".into());
    }
    let result = query_elevation(token);
    unsafe { CloseHandle(token) };
    result
}

#[cfg(not(windows))]
pub(crate) fn current_process_elevation() -> Result<bool, String> {
    Err("Windows process elevation is unavailable on this platform.".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn unreadable_token_is_unknown_instead_of_claiming_standard_privileges() {
        assert!(query_elevation(std::ptr::null_mut()).is_err());
    }

    #[test]
    fn actual_process_elevation_agrees_with_effective_administrator_check() {
        let expected = unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } != 0;
        assert_eq!(current_process_elevation(), Ok(expected));
    }
}
