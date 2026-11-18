//! Request Windows administrator consent for a foreground split-token launch.
//!
//! The executable manifest deliberately remains `asInvoker`: a standard user
//! must always be able to open WinCommander. For a foreground launch made by
//! a split-token administrator, Windows may start a second elevated copy.
//! Standard accounts always continue normally; accepting UAC consent starts
//! the administrator copy, while cancelling it leaves the original untouched.
//!
//! This is intentionally not a generic privilege broker.  It only chooses the
//! integrity level for the normal desktop process.  Individual machine-wide
//! operations still enforce their own authorization checks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupElevationResult {
    /// No prompt was appropriate, or Windows declined/cancelled the request.
    ContinueNormally,
    /// A verified elevated desktop owns or will take over this launch.
    UseElevatedDesktop,
}

const ELEVATED_RELAUNCH_FLAG: &str = "--elevated-relaunch";

/// Only foreground launches may request consent. Scheduler already chooses
/// the account's highest available token for the automatic sign-in task.
pub fn should_offer_startup_elevation(
    cli_mode: bool,
    args: &[String],
    calculator_lock_active: bool,
) -> bool {
    if cli_mode || calculator_lock_active || args.iter().any(|arg| arg == ELEVATED_RELAUNCH_FLAG) {
        return false;
    }

    // `--minimized` is the old Registry/Startup-folder background contract.
    // It must never independently request elevation: if it survived an
    // upgrade and fires beside the canonical `--autostart` task, an elevation
    // request here can create a second launch race before either process owns
    // the per-session mutex.  It is deliberately a quiet duplicate instead.
    !is_helper_launch(args) && !is_logon_router_launch(args) && !is_legacy_background_launch(args)
}

fn is_helper_launch(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--safe-copy" | "--context-shred" | "--scrub" | "--safe-paste" | "--recovery-reveal"
        )
    })
}

fn is_logon_router_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--autostart")
}

fn is_legacy_background_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--minimized") && !is_logon_router_launch(args)
}

pub fn is_elevated_relaunch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == ELEVATED_RELAUNCH_FLAG)
}

#[cfg(windows)]
pub fn is_current_process_elevated() -> bool {
    crate::process_privileges::current_process_elevation() == Ok(true)
}

/// A UAC-filtered Administrator token is not elevated, so IsUserAnAdmin()
/// returns false. TokenElevationTypeLimited is the Windows-supported way to
/// distinguish that account from a standard account without parsing localized
/// group names or invoking a shell command. This only decides whether to ask
/// Windows for consent; it grants no authority.
#[cfg(windows)]
fn current_user_has_split_admin_token() -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{
            GetTokenInformation, TokenElevationType, TokenElevationTypeLimited,
            TOKEN_ELEVATION_TYPE, TOKEN_QUERY,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation_type: TOKEN_ELEVATION_TYPE = 0;
        let mut bytes_returned = 0;
        let read_ok = GetTokenInformation(
            token,
            TokenElevationType,
            &mut elevation_type as *mut TOKEN_ELEVATION_TYPE as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut bytes_returned,
        ) != 0;
        CloseHandle(token);
        read_ok
            && bytes_returned == std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32
            && elevation_type == TokenElevationTypeLimited
    }
}

#[cfg(not(windows))]
pub fn is_current_process_elevated() -> bool {
    false
}

/// Recognize the foreground relaunch marker. Actual handoff still requires a
/// verified elevated sender, whether launched here or directly by Explorer.
pub fn should_handoff_existing_instance(args: &[String]) -> bool {
    !is_helper_launch(args)
        && !is_logon_router_launch(args)
        && !is_legacy_background_launch(args)
        && is_elevated_relaunch(args)
}

pub fn is_focus_only_launch(args: &[String]) -> bool {
    args.iter().skip(1).all(|arg| arg == "--focus")
}

pub fn can_promote_existing_instance(args: &[String], is_elevated: bool) -> bool {
    is_elevated && (should_handoff_existing_instance(args) || is_focus_only_launch(args))
}

/// Build the child arguments without passing the internal sentinel on again.
/// Windows receives one command-line string, so quote every user-supplied
/// argument with the documented backslash-before-quote rule.
fn elevated_parameters(args: &[String]) -> String {
    let mut parameters = vec![quote_windows_argument(ELEVATED_RELAUNCH_FLAG)];
    parameters.extend(
        args.iter()
            .skip(1)
            .filter(|arg| arg.as_str() != ELEVATED_RELAUNCH_FLAG)
            .map(|arg| quote_windows_argument(arg)),
    );
    parameters.join(" ")
}

fn quote_windows_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return argument.to_string();
    }

    let mut quoted = String::from("\"");
    let mut slashes = 0usize;
    for character in argument.chars() {
        match character {
            '\\' => slashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(slashes.saturating_mul(2).saturating_add(1)));
                quoted.push('"');
                slashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(slashes));
                quoted.push(character);
                slashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(slashes.saturating_mul(2)));
    quoted.push('"');
    quoted
}

fn should_request_admin_handoff(is_elevated: bool, has_split_admin_token: bool) -> bool {
    !is_elevated && has_split_admin_token
}

#[cfg(windows)]
pub fn offer_startup_elevation(args: &[String]) -> StartupElevationResult {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED},
        UI::{
            Shell::{
                ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
                SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    if !should_offer_startup_elevation(false, args, false)
        || !should_request_admin_handoff(
            is_current_process_elevated(),
            current_user_has_split_admin_token(),
        )
    {
        return StartupElevationResult::ContinueNormally;
    }

    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[StartupElevation] cannot resolve executable: {error}"),
            );
            return StartupElevationResult::ContinueNormally;
        }
    };
    if is_focus_only_launch(args) && crate::session_instance::reveal_elevated_primary(&executable) {
        return StartupElevationResult::UseElevatedDesktop;
    }

    // Scheduler acceptance is not proof of a high-token process. Use Windows'
    // interactive consent route directly when an elevated desktop is needed.
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let executable: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let parameters: Vec<u16> = elevated_parameters(args)
        .encode_utf16()
        .chain(Some(0))
        .collect();

    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: executable.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut request) } == 0 {
        let code = unsafe { GetLastError() };
        if code == ERROR_CANCELLED {
            crate::log_message_src(
                "info",
                "core",
                "[StartupElevation] consent declined; using standard mode",
            );
        } else {
            crate::log_message_src("warn", "core", &format!("[StartupElevation] Windows could not start the elevated desktop (error {code}); using standard mode"));
        }
        return StartupElevationResult::ContinueNormally;
    }

    let child = request.hProcess;
    if !child.is_null() {
        let elevated = crate::process_privileges::process_elevation(child);
        unsafe { CloseHandle(child) };
        if elevated == Ok(true) {
            return StartupElevationResult::UseElevatedDesktop;
        }
    }
    crate::log_message_src(
        "warn",
        "core",
        "[StartupElevation] Windows could not verify the new desktop's elevated token",
    );
    StartupElevationResult::ContinueNormally
}

#[cfg(not(windows))]
pub fn offer_startup_elevation(_args: &[String]) -> StartupElevationResult {
    StartupElevationResult::ContinueNormally
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_gui_launch_offers_elevation() {
        assert!(should_offer_startup_elevation(
            false,
            &["wincommander-free.exe".into()],
            false
        ));
    }

    #[test]
    fn calculator_cover_never_requests_optional_startup_elevation() {
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into()],
            true
        ));
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into(), "--focus".into()],
            true
        ));
        assert!(should_offer_startup_elevation(
            false,
            &["app.exe".into()],
            false
        ));
    }

    #[test]
    fn helpers_and_automatic_launches_never_request_elevation() {
        for flag in [
            "--safe-copy",
            "--context-shred",
            "--scrub",
            "--safe-paste",
            "--recovery-reveal",
        ] {
            assert!(!should_offer_startup_elevation(
                false,
                &["app.exe".into(), flag.into()],
                false
            ));
        }
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into(), "--autostart".into()],
            false
        ));
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into(), "--minimized".into()],
            false
        ));
        assert!(!should_offer_startup_elevation(
            true,
            &["app.exe".into()],
            false
        ));
    }

    #[test]
    fn logon_router_is_distinguished_from_foreground_launches() {
        assert!(is_logon_router_launch(&[
            "app.exe".into(),
            "--autostart".into()
        ]));
        assert!(!is_logon_router_launch(&["app.exe".into()]));
        assert!(is_legacy_background_launch(&[
            "app.exe".into(),
            "--minimized".into()
        ]));
        assert!(!is_legacy_background_launch(&[
            "app.exe".into(),
            "--autostart".into(),
            "--minimized".into()
        ]));
    }

    #[test]
    fn automatic_launch_never_retries_elevation_regardless_of_other_flags() {
        for extra in ["--focus", "--minimized", ELEVATED_RELAUNCH_FLAG] {
            assert!(!should_offer_startup_elevation(
                false,
                &["app.exe".into(), "--autostart".into(), extra.into()],
                false
            ));
        }
    }

    #[test]
    fn elevated_child_cannot_loop_back_into_uac() {
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into(), ELEVATED_RELAUNCH_FLAG.into()],
            false
        ));
    }

    #[test]
    fn only_a_split_token_administrator_needs_a_consent_request() {
        assert!(should_request_admin_handoff(false, true));
        assert!(!should_request_admin_handoff(false, false));
        assert!(!should_request_admin_handoff(true, true));
    }

    #[test]
    fn elevated_child_requests_a_cooperative_instance_handoff() {
        assert!(!should_handoff_existing_instance(&["app.exe".into()]));
        assert!(should_handoff_existing_instance(&[
            "app.exe".into(),
            ELEVATED_RELAUNCH_FLAG.into()
        ]));
        assert!(!should_handoff_existing_instance(&[
            "app.exe".into(),
            "--context-shred".into(),
            ELEVATED_RELAUNCH_FLAG.into()
        ]));
        assert!(!should_handoff_existing_instance(&[
            "app.exe".into(),
            ELEVATED_RELAUNCH_FLAG.into(),
            "--autostart".into()
        ]));
        assert!(!should_handoff_existing_instance(&[
            "app.exe".into(),
            ELEVATED_RELAUNCH_FLAG.into(),
            "--minimized".into()
        ]));
    }

    #[test]
    fn shell_elevated_foreground_launch_can_replace_a_standard_desktop() {
        for args in [
            vec!["app.exe".into()],
            vec!["app.exe".into(), "--focus".into()],
        ] {
            assert!(can_promote_existing_instance(&args, true));
            assert!(!can_promote_existing_instance(&args, false));
        }
        for flag in ["--autostart", "--minimized", "--safe-paste"] {
            assert!(!can_promote_existing_instance(
                &["app.exe".into(), flag.into()],
                true
            ));
        }
    }

    #[test]
    fn child_parameters_keep_the_sentinel_once_and_quote_paths() {
        assert_eq!(
            elevated_parameters(&[
                "app.exe".into(),
                "C:\\A Folder\\file.txt".into(),
                ELEVATED_RELAUNCH_FLAG.into()
            ]),
            "--elevated-relaunch \"C:\\A Folder\\file.txt\""
        );
    }
}
