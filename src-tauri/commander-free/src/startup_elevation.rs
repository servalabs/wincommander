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

/// A UAC prompt is useful only for an interactive, foreground GUI launch.
/// It must never appear at logon, for command-line work, context-menu helpers,
/// or when the newly elevated process re-enters this function.
pub fn should_offer_startup_elevation(cli_mode: bool, args: &[String]) -> bool {
    if cli_mode || args.iter().any(|arg| arg == ELEVATED_RELAUNCH_FLAG) {
        return false;
    }

    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--autostart"
                | "--minimized"
                | "--safe-copy"
                | "--context-shred"
                | "--scrub"
                | "--safe-paste"
        )
    })
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
    use windows_sys::Win32::UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    };

    // A user may explicitly choose "Run as administrator" from Explorer.
    // Do not spawn a needless second elevated instance in that case.
    if unsafe { IsUserAnAdmin() } != 0 {
        return StartupElevationResult::ContinueNormally;
    }

    // The session mutex is deliberately handed off before ShellExecuteW.
    // UAC starts the elevated child before this call returns; keeping the
    // mutex here would make the child look like a duplicate and exit instead.
    crate::session_instance::relinquish_for_elevation_handoff();

    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            crate::session_instance::reclaim_after_elevation_cancel();
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
        crate::session_instance::reclaim_after_elevation_cancel();
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
    fn background_and_helper_launches_never_prompt() {
        for flag in [
            "--autostart",
            "--minimized",
            "--safe-copy",
            "--context-shred",
            "--scrub",
            "--safe-paste",
        ] {
            assert!(!should_offer_startup_elevation(
                false,
                &["app.exe".into(), flag.into()]
            ));
        }
        assert!(!should_offer_startup_elevation(true, &["app.exe".into()]));
    }

    #[test]
    fn elevated_child_cannot_loop_back_into_uac() {
        assert!(!should_offer_startup_elevation(
            false,
            &["app.exe".into(), ELEVATED_RELAUNCH_FLAG.into()]
        ));
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
