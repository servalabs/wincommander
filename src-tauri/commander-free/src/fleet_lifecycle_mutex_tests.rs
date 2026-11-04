// SPDX-License-Identifier: AGPL-3.0-or-later
//! Harmless native tests: every object name is unique to this test run.
use super::*;
use std::{
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
        },
        ACL, DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
    },
    System::Threading::{CreateEventW, CreateMutexExW, SetEvent, WaitForSingleObject},
};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn name() -> String {
    format!(
        r"Global\WinCommander.FleetLease.Test.{}",
        uuid::Uuid::new_v4()
    )
}

fn named_mutex(name: &str, sddl: &str) -> OwnedHandle {
    let mut descriptor = ptr::null_mut();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl).as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        },
        0
    );
    let attr = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe { CreateMutexExW(&attr, wide(name).as_ptr(), 0, ACCESS) };
    unsafe {
        LocalFree(descriptor);
    }
    assert!(
        !handle.is_null(),
        "test mutex creation failed: {}",
        unsafe { GetLastError() }
    );
    OwnedHandle(handle)
}

fn dacl_bytes(handle: HANDLE) -> Vec<u8> {
    let mut acl: *mut ACL = ptr::null_mut();
    let mut descriptor: *mut c_void = ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(status, 0, "test ACL read failed");
    assert!(!acl.is_null());
    let data = unsafe {
        std::slice::from_raw_parts(acl.cast::<u8>(), usize::from((*acl).AclSize)).to_vec()
    };
    unsafe {
        LocalFree(descriptor);
    }
    data
}

fn child(name: &str, mode: &str) {
    // libtest names omit the crate component included by module_path!.
    let (_, module) = module_path!().split_once("::").expect("test module has a crate prefix");
    let filter = format!("{module}::child_probe");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    let mut process = command
        .args(["--exact", &filter, "--nocapture"])
        .env("WINCMD_FLEET_TEST_MUTEX", name)
        .env("WINCMD_FLEET_TEST_MODE", mode)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while process.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            panic!("test child exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "test child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if mode != "abandon" {
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child fixture did not execute"
        );
    } else {
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("WINCMD_FLEET_ABANDON_ACQUIRED"),
            "child did not prove mutex ownership before exit"
        );
    }
}

#[test]
fn child_probe() {
    let Ok(name) = std::env::var("WINCMD_FLEET_TEST_MUTEX") else {
        return;
    };
    match std::env::var("WINCMD_FLEET_TEST_MODE").as_deref() {
        Ok("busy") => assert!(acquire_named(&name, 100).is_err()),
        Ok("free") => assert!(acquire_named(&name, 1_000).is_ok()),
        Ok("abandon") => {
            let _guard = acquire_named(&name, 1_000).expect("child must own mutex");
            use std::io::Write;
            println!("WINCMD_FLEET_ABANDON_ACQUIRED");
            std::io::stdout().flush().unwrap();
            // Deliberately terminate without running Rust destructors.
            std::process::exit(0);
        }
        other => panic!("unexpected child test mode: {other:?}"),
    }
}

#[test]
fn separate_process_waits_then_acquires_without_affecting_workers() {
    let name = name();
    let guard = acquire_named(&name, 1_000).expect("first acquisition");
    child(&name, "busy");
    drop(guard);
    child(&name, "free");
}

#[test]
fn abandoned_owner_transfers_exclusive_ownership_until_guard_is_dropped() {
    let name = name();
    // Keep the object alive while the child exits; otherwise its last handle
    // disappears and Windows creates a new mutex instead of reporting abandonment.
    let anchor = named_mutex(&name, SDDL);
    child(&name, "abandon");
    let recovered = acquire_named(&name, 1_000).expect("abandoned mutex transfers ownership");
    child(&name, "busy");
    drop(recovered);
    child(&name, "free");
    drop(anchor);
}

#[test]
fn precreated_wrong_object_is_rejected_without_modifying_it() {
    let name = name();
    let raw = unsafe { CreateEventW(ptr::null(), 1, 0, wide(&name).as_ptr()) };
    assert!(!raw.is_null());
    let event = OwnedHandle(raw);
    assert_eq!(unsafe { WaitForSingleObject(event.0, 0) }, WAIT_TIMEOUT);
    assert!(acquire_named(&name, 100).is_err());
    assert_eq!(unsafe { WaitForSingleObject(event.0, 0) }, WAIT_TIMEOUT);
    assert_ne!(unsafe { SetEvent(event.0) }, 0);
    assert_eq!(unsafe { WaitForSingleObject(event.0, 0) }, WAIT_OBJECT_0);
}

#[test]
fn precreated_incompatible_acl_is_rejected_without_repair() {
    let name = name();
    // This ACL permits our requested access but deliberately omits OW; simply
    // getting a handle is insufficient to establish compatible coordination.
    let object = named_mutex(
        &name,
        "D:P(A;;0x00120001;;;AU)(A;;0x001f0001;;;SY)(A;;0x001f0001;;;BA)S:(ML;;NW;;;ME)",
    );
    let before = dacl_bytes(object.0);
    assert!(acquire_named(&name, 100).is_err());
    assert_eq!(
        dacl_bytes(object.0),
        before,
        "an untrusted ACL must not be rewritten"
    );
}

#[test]
fn ace_parser_rejects_truncated_and_malformed_entries() {
    use windows_sys::Win32::Security::{ACE_HEADER, ACL};

    #[repr(align(8))]
    struct Storage([u8; 128]);
    let mut storage = Storage([0; 128]);
    let acl = storage.0.as_mut_ptr().cast::<ACL>();
    let entry = unsafe { storage.0.as_mut_ptr().add(std::mem::size_of::<ACL>()) };
    let header = entry.cast::<ACE_HEADER>();
    unsafe {
        (*acl).AclSize = (std::mem::size_of::<ACL>() + 20) as u16;
        (*header).AceType = 0;
        (*header).AceSize = 20;
        // SID header at ACE+8, followed by one four-byte subauthority.
        *entry.add(8) = 1;
        *entry.add(9) = 1;
        std::ptr::write_unaligned(entry.add(4).cast::<u32>(), ACCESS);
        assert_eq!(ace_sid(acl, entry.cast(), 0).unwrap().1, ACCESS);

        (*acl).AclSize = (std::mem::size_of::<ACL>() + 15) as u16;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "truncated ACE header/SID"
        );
        (*acl).AclSize = (std::mem::size_of::<ACL>() + 20) as u16;
        (*header).AceSize = 15;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "declared ACE too small"
        );
        (*header).AceSize = 24;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "declared ACE exceeds ACL"
        );
        (*header).AceSize = 20;
        (*header).AceType = 1;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "unexpected ACE type"
        );
        (*header).AceType = 0;
        (*header).AceFlags = 1;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "unexpected ACE flags"
        );
        (*header).AceFlags = 0;
        *entry.add(9) = 2;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "subauthority exceeds ACE"
        );
        *entry.add(9) = 16;
        assert!(
            ace_sid(acl, entry.cast(), 0).is_err(),
            "too many subauthorities"
        );
        assert!(
            ace_sid(acl, storage.0.as_mut_ptr().cast(), 0).is_err(),
            "entry before ACL body"
        );
    }
}

#[path = "fleet_lifecycle_mutex_security_tests.rs"]
mod security_tests;
