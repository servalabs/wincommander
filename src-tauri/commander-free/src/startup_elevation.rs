//! Request Windows elevation for a foreground launch of a non-elevated process.
//!
//! The executable manifest deliberately remains `asInvoker`: a standard user
//! must always be able to open WinCommander. For a foreground launch Windows
//! may start a second elevated copy: a split-token administrator gets the UAC
//! consent prompt, a standard account gets the administrator-credential prompt
//! (the copy then runs as that administrator). Accepting starts the elevated
//! copy; cancelling or failing leaves the original running untouched.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Elevated,
    /// UAC-filtered administrator: Windows offers the consent prompt.
    SplitAdmin,
    /// Standard account, or an administrator token with no elevated pair.
    Other,
}

/// A UAC-filtered Administrator token is not elevated, so IsUserAnAdmin()
/// returns false. TokenElevationTypeLimited is the Windows-supported way to
/// tell that account from the rest without parsing localized group names or
/// invoking a shell command. `None` means the token could not be read, which
/// never prompts. This only decides whether to ask Windows; it grants nothing.
#[cfg(windows)]
fn current_token_kind() -> Option<TokenKind> {
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
            return None;
        }
        let mut elevation_type: TOKEN_ELEVATION_TYPE = 0;
        let mut bytes_returned = 0;
        let read_ok = GetTokenInformation(
            token,
            TokenElevationType,
            &mut elevation_type as *mut TOKEN_ELEVATION_TYPE as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut bytes_returned,
        ) != 0
            && bytes_returned == std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32;
        let elevated = crate::process_privileges::query_elevation(token);
        CloseHandle(token);
        match (elevated, read_ok) {
            (Ok(true), _) => Some(TokenKind::Elevated),
            (Ok(false), true) if elevation_type == TokenElevationTypeLimited => {
                Some(TokenKind::SplitAdmin)
            }
            (Ok(false), true) => Some(TokenKind::Other),
            _ => None,
        }
    }
}

/// The machine's UAC switches that decide whether asking can ever work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UacPolicy {
    enabled: bool,
    standard_requests_denied: bool,
}

/// Missing values mean Windows' defaults: UAC on, standard users prompted.
/// ConsentPromptBehaviorUser 0 is "automatically deny elevation requests".
fn uac_policy_from_values(enable_lua: Option<u32>, consent_prompt_user: Option<u32>) -> UacPolicy {
    UacPolicy {
        enabled: enable_lua != Some(0),
        standard_requests_denied: consent_prompt_user == Some(0),
    }
}

#[cfg(windows)]
fn read_uac_policy() -> UacPolicy {
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System")
        .ok();
    let read = |name: &str| key.as_ref().and_then(|key| key.get_value::<u32, _>(name).ok());
    uac_policy_from_values(read("EnableLUA"), read("ConsentPromptBehaviorUser"))
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

/// A credential prompt runs the copy as another account, which cannot share
/// this account's per-user instance mutex: ask standard accounts only when no
/// instance is running, otherwise the launch just focuses it.
fn should_request_elevation(token: TokenKind, policy: UacPolicy, primary_running: bool) -> bool {
    match token {
        TokenKind::Elevated => false,
        _ if !policy.enabled => false,
        TokenKind::SplitAdmin => true,
        TokenKind::Other => !policy.standard_requests_denied && !primary_running,
    }
}

/// A token that cannot be read (another account's process) is judged by
/// whether the child is still alive after its own startup guard has had time
/// to reject a non-elevated copy.
fn startup_outcome(verified: Option<bool>, child_still_running: bool) -> StartupElevationResult {
    if verified.unwrap_or(child_still_running) {
        StartupElevationResult::UseElevatedDesktop
    } else {
        StartupElevationResult::ContinueNormally
    }
}

#[cfg(windows)]
const UNVERIFIED_CHILD_GRACE_MS: u32 = 5_000;

#[cfg(windows)]
pub fn offer_startup_elevation(args: &[String]) -> StartupElevationResult {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, WAIT_TIMEOUT},
        System::Threading::WaitForSingleObject,
        UI::{
            Shell::{
                ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
                SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    if !should_offer_startup_elevation(false, args, false)
        || !current_token_kind().is_some_and(|token| {
            should_request_elevation(
                token,
                read_uac_policy(),
                crate::session_instance::primary_running(),
            )
        })
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
    let mut verified = None;
    let mut child_still_running = false;
    if !child.is_null() {
        verified = crate::process_privileges::process_elevation(child).ok();
        if verified.is_none() {
            child_still_running =
                unsafe { WaitForSingleObject(child, UNVERIFIED_CHILD_GRACE_MS) } == WAIT_TIMEOUT;
        }
        unsafe { CloseHandle(child) };
    }
    let outcome = startup_outcome(verified, child_still_running);
    if outcome == StartupElevationResult::ContinueNormally {
        crate::log_message_src(
            "warn",
            "core",
            "[StartupElevation] Windows could not verify the new desktop's elevated token",
        );
    }
    outcome
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

    const UAC_ON: UacPolicy = UacPolicy {
        enabled: true,
        standard_requests_denied: false,
    };

    #[test]
    fn split_token_administrator_is_asked_for_consent_even_beside_a_running_copy() {
        assert!(should_request_elevation(TokenKind::SplitAdmin, UAC_ON, false));
        assert!(should_request_elevation(TokenKind::SplitAdmin, UAC_ON, true));
    }

    #[test]
    fn standard_account_is_asked_for_credentials_only_when_no_copy_is_running() {
        assert!(should_request_elevation(TokenKind::Other, UAC_ON, false));
        assert!(!should_request_elevation(TokenKind::Other, UAC_ON, true));
    }

    #[test]
    fn an_already_elevated_process_is_never_asked_again() {
        for running in [false, true] {
            assert!(!should_request_elevation(TokenKind::Elevated, UAC_ON, running));
        }
    }

    #[test]
    fn disabled_uac_never_prompts_any_account() {
        let off = uac_policy_from_values(Some(0), None);
        for token in [TokenKind::SplitAdmin, TokenKind::Other, TokenKind::Elevated] {
            assert!(!should_request_elevation(token, off, false));
        }
    }

    #[test]
    fn policy_that_auto_denies_standard_requests_skips_only_standard_accounts() {
        let deny = uac_policy_from_values(Some(1), Some(0));
        assert!(!should_request_elevation(TokenKind::Other, deny, false));
        assert!(should_request_elevation(TokenKind::SplitAdmin, deny, false));
    }

    #[test]
    fn missing_uac_registry_values_mean_windows_defaults() {
        assert_eq!(uac_policy_from_values(None, None), UAC_ON);
        assert_eq!(uac_policy_from_values(Some(1), Some(3)), UAC_ON);
        assert!(!uac_policy_from_values(Some(0), Some(3)).enabled);
    }

    #[test]
    fn unreadable_child_token_trusts_a_child_that_survived_its_startup_guard() {
        assert_eq!(startup_outcome(Some(true), false), StartupElevationResult::UseElevatedDesktop);
        assert_eq!(startup_outcome(Some(false), true), StartupElevationResult::ContinueNormally);
        assert_eq!(startup_outcome(None, true), StartupElevationResult::UseElevatedDesktop);
        assert_eq!(startup_outcome(None, false), StartupElevationResult::ContinueNormally);
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
