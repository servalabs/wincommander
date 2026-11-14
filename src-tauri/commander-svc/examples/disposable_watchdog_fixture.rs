// SPDX-License-Identifier: AGPL-3.0-or-later
// Disposable CI fixture only: this is not production recovery or an ACL boundary.
#[cfg(windows)]
fn main() {
    use std::os::windows::io::OwnedHandle;
    use std::{
        fs,
        os::windows::io::FromRawHandle,
        time::{Duration, Instant},
    };
    use windows_service::{
        service::{ServiceAccess, ServiceState},
        service_manager::{ServiceManager, ServiceManagerAccess},
    };
    use windows_sys::Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject},
    };

    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "parent PID, fixture service name, state marker, ready marker"
    );
    let pid: u32 = args[1].parse().unwrap();
    let name = &args[2];
    assert!(
        name.starts_with("WinCommanderSearchTestFixture")
            && name.bytes().all(|b| b.is_ascii_alphanumeric())
    );
    let marker = std::path::Path::new(&args[3]);
    let ready = std::path::Path::new(&args[4]);
    assert_eq!(fs::read(marker).unwrap(), b"original=running\n");
    // Capture the actual parent process object before announcing readiness, so
    // subsequent PID reuse cannot redirect the wait.
    let raw = unsafe { OpenProcess(0x0010_0000, 0, pid) }; // SYNCHRONIZE
    assert!(!raw.is_null());
    let _parent = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).unwrap();
    let service = manager
        .open_service(name, ServiceAccess::QUERY_STATUS | ServiceAccess::START)
        .unwrap();
    fs::write(ready, b"ready").unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(raw as HANDLE, u32::MAX) },
        WAIT_OBJECT_0
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match service.query_status().unwrap().current_state {
            ServiceState::Running => break,
            ServiceState::Stopped => {
                let _ = service.start::<&str>(&[]);
            }
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "fixture service was not restored"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    fs::remove_file(marker).unwrap();
}

#[cfg(not(windows))]
fn main() {
    panic!("Windows fixture only");
}
