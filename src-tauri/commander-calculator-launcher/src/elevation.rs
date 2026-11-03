use crate::LaunchError;
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    Security::{
        GetTokenInformation, TokenElevation, TokenElevationType, TokenElevationTypeLimited,
        TOKEN_ELEVATION, TOKEN_ELEVATION_TYPE, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaunchToken {
    Current { elevated: bool },
    ElevatedCopyStarted,
}

fn decide(
    elevated: bool,
    limited_admin: bool,
    request_consent: impl FnOnce() -> bool,
) -> LaunchToken {
    if !elevated && limited_admin && request_consent() {
        LaunchToken::ElevatedCopyStarted
    } else {
        LaunchToken::Current { elevated }
    }
}

fn current_token() -> Result<(bool, bool), LaunchError> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(LaunchError::InspectionFailed);
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut kind: TOKEN_ELEVATION_TYPE = 0;
        let mut size = 0;
        let read_elevation = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        ) != 0;
        let read_kind = GetTokenInformation(
            token,
            TokenElevationType,
            (&mut kind as *mut TOKEN_ELEVATION_TYPE).cast(),
            std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut size,
        ) != 0;
        CloseHandle(token);
        if !read_elevation || !read_kind {
            return Err(LaunchError::InspectionFailed);
        }
        Ok((
            elevation.TokenIsElevated != 0,
            kind == TokenElevationTypeLimited,
        ))
    }
}

pub(super) fn offer(current: &Path, directory: &Path) -> Result<LaunchToken, LaunchError> {
    let (elevated, limited_admin) = current_token()?;
    Ok(decide(elevated, limited_admin, || {
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let image: Vec<u16> = current.as_os_str().encode_wide().chain(Some(0)).collect();
        let directory: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        // Consent names this real installed executable; no arguments or child
        // path can be supplied by a shortcut or forwarded to the elevated copy.
        (unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                image.as_ptr(),
                std::ptr::null(),
                directory.as_ptr(),
                SW_SHOWNORMAL,
            )
        } as isize)
            > 32
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_filtered_administrator_requests_consent() {
        assert_eq!(
            decide(false, false, || panic!("standard account must not prompt")),
            LaunchToken::Current { elevated: false }
        );
        assert_eq!(
            decide(true, false, || panic!("elevated copy must not loop")),
            LaunchToken::Current { elevated: true }
        );
        assert_eq!(
            decide(true, true, || panic!("an elevated copy must not prompt")),
            LaunchToken::Current { elevated: true }
        );
        assert_eq!(
            decide(false, true, || true),
            LaunchToken::ElevatedCopyStarted
        );
    }

    #[test]
    fn cancelled_consent_never_claims_an_elevated_token() {
        assert_eq!(
            decide(false, true, || false),
            LaunchToken::Current { elevated: false }
        );
    }
}
