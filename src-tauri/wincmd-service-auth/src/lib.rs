// SPDX-License-Identifier: AGPL-3.0-or-later
//! Verify the installed service before sending Hello, credentials, or requests.
//! The client-chosen handshake token is not proof of the server's identity.
#![cfg(windows)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use windows_service::service::{ServiceAccess, ServiceState, ServiceType};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Retain this guard for the connection's lifetime, pinning the process object.
#[derive(Debug)]
pub struct VerifiedServicePeer {
    _process: OwnedHandle,
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "service pipe identity could not be verified",
    )
}

/// Read the kernel-supplied server PID; never use a PID from a wire payload.
pub fn server_process_id(pipe: &impl AsRawHandle) -> io::Result<u32> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0
        || pid == 0
    {
        return Err(denied());
    }
    Ok(pid)
}

fn require_running_pid(running: bool, registered: Option<u32>, connected: u32) -> io::Result<()> {
    if !running || connected == 0 || registered != Some(connected) {
        return Err(denied());
    }
    Ok(())
}

fn service_executable(command: &Path) -> io::Result<PathBuf> {
    // Our installer registers one absolute executable with no arguments.
    // Do not guess where an unrecognized service command line ends.
    let command = command.to_str().ok_or_else(denied)?.trim();
    let path = if let Some(quoted) = command.strip_prefix('"') {
        let (path, rest) = quoted.split_once('"').ok_or_else(denied)?;
        if !rest.trim().is_empty() {
            return Err(denied());
        }
        PathBuf::from(path)
    } else {
        if command.contains('"') {
            return Err(denied());
        }
        PathBuf::from(command)
    };
    if !path.is_absolute() {
        return Err(denied());
    }
    Ok(path)
}

pub fn verify_service_peer(pipe: &impl AsRawHandle) -> io::Result<VerifiedServicePeer> {
    let pid = server_process_id(pipe)?;
    let process = pin_process(pid)?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|_| denied())?;
    let service = manager
        .open_service(
            wincmd_shared::svc::SVC_WINDOWS_SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        )
        .map_err(|_| denied())?;
    let status = service.query_status().map_err(|_| denied())?;
    require_running_pid(
        status.current_state == ServiceState::Running,
        status.process_id,
        pid,
    )?;
    let config = service.query_config().map_err(|_| denied())?;
    require_service_configuration(
        status.service_type,
        config.service_type,
        config.account_name.as_deref(),
    )?;
    let expected = std::fs::canonicalize(service_executable(&config.executable_path)?)
        .map_err(|_| denied())?;
    if !expected.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case("wincommander-svc.exe")
    }) {
        return Err(denied());
    }
    verify_process_image(&process, &expected)?;
    // Recheck the SCM after inspecting its configuration. The process handle
    // pins the PID even if the service exits while this connection is checked.
    let status = service.query_status().map_err(|_| denied())?;
    require_running_pid(
        status.current_state == ServiceState::Running,
        status.process_id,
        pid,
    )?;
    if status.service_type != ServiceType::OWN_PROCESS || server_process_id(pipe)? != pid {
        return Err(denied());
    }
    Ok(VerifiedServicePeer { _process: process })
}

fn require_service_configuration(
    running_type: ServiceType,
    configured_type: ServiceType,
    account: Option<&std::ffi::OsStr>,
) -> io::Result<()> {
    if running_type != ServiceType::OWN_PROCESS
        || configured_type != ServiceType::OWN_PROCESS
        || !account.is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("LocalSystem"))
    {
        return Err(denied());
    }
    Ok(())
}

fn pin_process(pid: u32) -> io::Result<OwnedHandle> {
    // The SYSTEM service grants the connecting SID image-query rights just
    // after accept. Bound that race here for every consumer (Free/Pro/helper),
    // on the same pipe and before any Hello, request, or credential is sent.
    retry_process_open(
        || {
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedHandle::from_raw_handle(process) })
        },
        || std::thread::sleep(std::time::Duration::from_millis(25)),
    )
}

fn retry_process_open<T>(
    mut open: impl FnMut() -> io::Result<T>,
    mut wait: impl FnMut(),
) -> io::Result<T> {
    for attempt in 0..=20 {
        match open() {
            Err(error) if error.raw_os_error() == Some(5) && attempt < 20 => wait(),
            result => return result,
        }
    }
    unreachable!("final attempt returns")
}

fn verify_process_image(process: &OwnedHandle, expected: &Path) -> io::Result<()> {
    let mut image = vec![0u16; 32768];
    let mut length = image.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(
            process.as_raw_handle() as HANDLE,
            0,
            image.as_mut_ptr(),
            &mut length,
        )
    } == 0
    {
        return Err(denied());
    }
    use std::os::windows::ffi::OsStringExt;
    let image = PathBuf::from(std::ffi::OsString::from_wide(&image[..length as usize]));
    let actual = std::fs::canonicalize(image).map_err(|_| denied())?;
    if !expected
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&actual.as_os_str().to_string_lossy())
    {
        return Err(denied());
    }
    Ok(())
}

#[cfg(test)]
mod restricted_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_stopped_zero_and_wrong_service_pids_are_denied() {
        for (running, expected, actual) in [
            (false, Some(7), 7),
            (true, None, 7),
            (true, Some(0), 0),
            (true, Some(7), 8),
        ] {
            assert_eq!(
                require_running_pid(running, expected, actual)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
        }
        assert!(require_running_pid(true, Some(7), 7).is_ok());
    }

    #[test]
    fn registered_executable_parser_is_fail_closed() {
        assert_eq!(
            service_executable(Path::new(
                r#""C:\Program Files\WinCommander\wincommander-svc.exe""#
            ))
            .unwrap(),
            PathBuf::from(r"C:\Program Files\WinCommander\wincommander-svc.exe")
        );
        for command in [
            "relative.exe",
            r#""C:\svc.exe" --unexpected"#,
            r#""C:\svc.exe"#,
        ] {
            assert!(service_executable(Path::new(command)).is_err());
        }
    }

    #[test]
    fn only_a_dedicated_localsystem_service_is_accepted() {
        use std::ffi::OsStr;
        assert!(require_service_configuration(
            ServiceType::OWN_PROCESS,
            ServiceType::OWN_PROCESS,
            Some(OsStr::new("LocalSystem")),
        )
        .is_ok());
        for (running, configured, account) in [
            (
                ServiceType::SHARE_PROCESS,
                ServiceType::OWN_PROCESS,
                Some("LocalSystem"),
            ),
            (
                ServiceType::OWN_PROCESS,
                ServiceType::SHARE_PROCESS,
                Some("LocalSystem"),
            ),
            (
                ServiceType::OWN_PROCESS,
                ServiceType::OWN_PROCESS,
                Some("LocalService"),
            ),
            (
                ServiceType::OWN_PROCESS,
                ServiceType::OWN_PROCESS,
                Some("SomeUser"),
            ),
            (ServiceType::OWN_PROCESS, ServiceType::OWN_PROCESS, None),
        ] {
            assert!(
                require_service_configuration(running, configured, account.map(OsStr::new))
                    .is_err()
            );
        }
    }

    #[test]
    fn process_image_mismatch_is_not_accepted() {
        let process = pin_process(std::process::id()).unwrap();
        assert!(
            verify_process_image(&process, Path::new(r"C:\unrelated\wincommander-svc.exe"))
                .is_err()
        );
        assert!(pin_process(u32::MAX).is_err());
    }

    #[test]
    fn process_query_retry_is_bounded_and_only_for_access_denied() {
        let mut attempts = 0;
        let mut waits = 0;
        let result: io::Result<()> = retry_process_open(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(5))
            },
            || waits += 1,
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
        assert_eq!((attempts, waits), (21, 20));
        for code in [6, 87, 299] {
            let result: io::Result<()> = retry_process_open(
                || Err(io::Error::from_raw_os_error(code)),
                || panic!("unrelated errors must fail immediately"),
            );
            assert_eq!(result.unwrap_err().raw_os_error(), Some(code));
        }
        let mut attempts = 0;
        assert_eq!(
            retry_process_open(
                || {
                    attempts += 1;
                    if attempts == 3 {
                        Ok(7)
                    } else {
                        Err(io::Error::from_raw_os_error(5))
                    }
                },
                || {}
            )
            .unwrap(),
            7
        );
        assert_eq!(attempts, 3);
    }

    #[tokio::test]
    async fn a_real_pipe_owned_by_this_test_process_is_not_the_installed_service() {
        use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
        let name = format!(r"\\.\pipe\wincmd-peer-auth-test-{}", std::process::id());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let client = ClientOptions::new().open(&name).unwrap();
        server.connect().await.unwrap();
        assert_eq!(server_process_id(&client).unwrap(), std::process::id());
        assert_eq!(
            verify_service_peer(&client).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
