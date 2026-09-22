//! Optional Windows elevation at an interactive launch.
//!
//! The executable manifest deliberately remains `asInvoker`: a standard user
//! must always be able to open WinCommander.  For a foreground launch we ask
//! Windows to start a second, elevated copy.  Accepting the UAC consent starts
//! that copy; cancelling it leaves the already-running copy untouched.
//!
//! This is intentionally not a generic privilege broker.  It only chooses the
//! integrity level for the normal desktop process.  Individual machine-wide
//! operations still enforce their own authorization checks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupElevationResult {
    /// No prompt was appropriate, or Windows declined/cancelled the request.
    ContinueNormally,
    /// Windows accepted the elevation request and the original process should exit.
    ElevatedCopyStarted,
}

const ELEVATED_RELAUNCH_FLAG: &str = "--elevated-relaunch";
const ELEVATED_LAUNCH_TASK: &str = "WinCommander Elevated Launcher";

/// A UAC prompt is useful for every interactive desktop launch, including the
/// logon launch. This lets Windows show its normal consent/credential prompt
/// instead of silently pinning WinCommander to a limited token.
pub fn should_offer_startup_elevation(cli_mode: bool, args: &[String]) -> bool {
    if cli_mode || args.iter().any(|arg| arg == ELEVATED_RELAUNCH_FLAG) {
        return false;
    }

    !is_helper_launch(args)
}

fn is_helper_launch(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--safe-copy" | "--context-shred" | "--scrub" | "--safe-paste"
        )
    })
}

fn is_logon_router_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--autostart")
}

fn should_continue_normal_logon(args: &[String], user_has_split_admin_token: bool) -> bool {
    is_logon_router_launch(args) && !user_has_split_admin_token
}

pub fn is_elevated_relaunch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == ELEVATED_RELAUNCH_FLAG)
}

#[cfg(windows)]
pub fn is_current_process_elevated() -> bool {
    (unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() }) != 0
}

/// A UAC-filtered Administrator token is not elevated, so IsUserAnAdmin()
/// returns false. TokenElevationTypeLimited is the Windows-supported way to
/// distinguish that account from a standard account without parsing localized
/// group names or invoking a shell command. This only decides whether a broken
/// trusted logon route should fall back to consent; it grants no authority.
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
        read_ok && elevation_type == TokenElevationTypeLimited
    }
}

#[cfg(not(windows))]
pub fn is_current_process_elevated() -> bool {
    false
}

/// Only the child created by this module can replace a normal primary through
/// a cooperative exit request. Other duplicate launches remain ordinary
/// forwards, even when Explorer already started them elevated.
pub fn should_handoff_existing_instance(args: &[String]) -> bool {
    !is_helper_launch(args) && is_elevated_relaunch(args)
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

#[cfg(windows)]
pub fn offer_startup_elevation(args: &[String]) -> StartupElevationResult {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    };

    // A user may explicitly choose "Run as administrator" from Explorer.
    // Do not spawn a needless second elevated instance in that case.
    if unsafe { IsUserAnAdmin() } != 0 {
        return StartupElevationResult::ContinueNormally;
    }

    // The machine installer owns this Administrators-group task. Task
    // Scheduler verifies group membership and starts the configured high-token
    // child without another consent dialog. If the task is absent, blocked by
    // policy, or this is a standard user, deliberately fall through to UAC.
    let task_name = ELEVATED_LAUNCH_TASK;
    let task_status = std::process::Command::new("schtasks.exe")
        .args(["/Run", "/TN", task_name])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .status();
    if matches!(task_status, Ok(status) if status.success()) {
        crate::log_message_src(
            "info",
            "core",
            &format!("[StartupElevation] trusted task started: {task_name}"),
        );
        return StartupElevationResult::ElevatedCopyStarted;
    }

    // The sole logon task runs at the caller's normal token. For a standard
    // user the Administrators-only launcher is correctly unavailable; logon
    // must continue normally without a disruptive UAC prompt. A split-token
    // Administrator instead falls through to Windows UAC if the installer
    // task is missing or blocked, rather than silently losing elevation.
    if should_continue_normal_logon(args, current_user_has_split_admin_token()) {
        crate::log_message_src(
            "info",
            "core",
            "[StartupElevation] elevated launcher unavailable at logon; continuing with the normal user token",
        );
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

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            executable.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        // UAC cancel is intentionally quiet. It is a normal choice, not an
        // application error; the original app carries on with its user token.
        crate::log_message_src(
            "info",
            "core",
            "[StartupElevation] elevation was declined or unavailable; continuing normally",
        );
        StartupElevationResult::ContinueNormally
    } else {
        crate::log_message_src("info", "core", "[StartupElevation] elevated copy started");
        StartupElevationResult::ElevatedCopyStarted
    }
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
            &["wincommander-free.exe".into()]
        ));
    }

    #[test]
    fn helpers_never_prompt_but_autostart_is_routed_without_ui() {
        for flag in ["--safe-copy", "--context-shred", "--scrub", "--safe-paste"] {
            assert!(!should_offer_startup_elevation(
                false,
                &["app.exe".into(), flag.into()]
            ));
        }
        assert!(should_offer_startup_elevation(
            false,
            &["app.exe".into(), "--autostart".into()]
        ));
        assert!(!should_offer_startup_elevation(true, &["app.exe".into()]));
    }

    #[test]
    fn logon_router_is_distinguished_from_foreground_launches() {
        assert!(is_logon_router_launch(&[
            "app.exe".into(),
            "--autostart".into()
        ]));
        assert!(!is_logon_router_launch(&["app.exe".into()]));
    }

    #[test]
    fn only_standard_user_logon_skips_uac_when_the_trusted_task_is_unavailable() {
        let logon = ["app.exe".into(), "--autostart".into()];
        assert!(should_continue_normal_logon(&logon, false));
        assert!(!should_continue_normal_logon(&logon, true));
        assert!(!should_continue_normal_logon(&["app.exe".into()], false));
    }

    #[test]
    fn elevated_child_cannot_loop_back_into_uac() {
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into(), ELEVATED_RELAUNCH_FLAG.into()]
        ));
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
