// Disposable, machine-specific probe only; never points at the installed product or ProgramData.
// The fixture uses an explicit protected DACL granting Users W,RX and SYSTEM
// and Administrators full control. The second handle requests 0x100 to isolate the
// metadata-only write access relevant to reparse mutation. A separate read handle
// retains FILE_SHARE_READ|FILE_SHARE_WRITE without FILE_SHARE_DELETE by default;
// --deny-write instead shares reads only.
// --restricted impersonates a LUA token with maximum privileges disabled and
// asserts Users is enabled and Administrators is disabled. This is not a
// separate signed-in user and does not reproduce the complete product path.
// Success demonstrates a reparse mutation in this fixture. Failure is not
// proof of safety: token privileges and filesystem policy can affect it.
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::PathBuf};
#[repr(C)]
struct Uni {
    len: u16,
    max: u16,
    buf: *mut u16,
}
#[repr(C)]
struct Obj {
    len: u32,
    root: Handle,
    name: *mut Uni,
    flags: u32,
    security: *mut c_void,
    quality: *mut c_void,
}
#[repr(C)]
struct IoStatus {
    status: usize,
    info: usize,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        out: *mut Handle,
        access: u32,
        attrs: *mut Obj,
        iosb: *mut IoStatus,
        alloc: *mut i64,
        attr: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *mut c_void,
        ea_len: u32,
    ) -> i32;
}

type Handle = *mut c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        attrs: *const c_void,
        disposition: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn DeviceIoControl(
        handle: Handle,
        code: u32,
        input: *const c_void,
        input_len: u32,
        output: *mut c_void,
        output_len: u32,
        returned: *mut u32,
        overlap: *mut c_void,
    ) -> i32;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn CreateRestrictedToken(
        token: Handle,
        flags: u32,
        disable_count: u32,
        disable_sids: *const c_void,
        delete_count: u32,
        delete_privileges: *const c_void,
        restrict_count: u32,
        restrict_sids: *const c_void,
        result: *mut Handle,
    ) -> i32;
    fn ImpersonateLoggedOnUser(token: Handle) -> i32;
    fn RevertToSelf() -> i32;
    fn ConvertStringSidToSidW(sid: *const u16, result: *mut *mut c_void) -> i32;
    fn CheckTokenMembership(token: Handle, sid: *mut c_void, member: *mut i32) -> i32;
}
fn wide(path: &std::path::Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn main() {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .expect("supply a new disposable fixture directory"),
    );
    let flags: Vec<_> = args.collect();
    assert!(
        flags
            .iter()
            .all(|value| value == "--restricted" || value == "--deny-write"),
        "unexpected argument"
    );
    let restricted = flags.iter().any(|value| value == "--restricted");
    let deny_write = flags.iter().any(|value| value == "--deny-write");
    let base = std::fs::canonicalize(r"C:\_cargo-target\safe-copy-native-fixture-20261106")
        .expect("fixture base unavailable");
    let parent = root
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .expect("fixture parent unavailable");
    assert_eq!(
        parent, base,
        "fixture must be a direct child of the disposable base"
    );
    assert!(
        root.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("reparse-fixture-")),
        "invalid fixture name"
    );
    assert!(!root.exists(), "fixture path must not already exist");
    std::fs::create_dir(&root).expect("create fixture root");
    let source = root.join("source");
    let target = root.join("target");
    let readcase = root.join("readcase");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::create_dir(&readcase).unwrap();
    std::fs::write(readcase.join("settings.dat"), b"fixture settings").unwrap();
    for dir in [&source, &readcase] {
        let status = std::process::Command::new(r"C:\Windows\System32\icacls.exe")
            .arg(dir)
            .args([
                "/inheritance:r",
                "/grant:r",
                "*S-1-5-32-544:(OI)(CI)F",
                "*S-1-5-18:(OI)(CI)F",
                "*S-1-5-32-545:(OI)(CI)(W,RX)",
            ])
            .status()
            .unwrap();
        assert!(
            status.success(),
            "cannot grant fixture Users the requested rights"
        );
    }
    let mut token: Handle = std::ptr::null_mut();
    let mut limited: Handle = std::ptr::null_mut();
    if restricted {
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), 0x000e, &mut token) },
            0,
            "open token: {}",
            unsafe { GetLastError() }
        );
        assert_ne!(
            unsafe {
                CreateRestrictedToken(
                    token,
                    0x5,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    &mut limited,
                )
            },
            0,
            "restrict token: {}",
            unsafe { GetLastError() }
        );
        assert_ne!(
            unsafe { ImpersonateLoggedOnUser(limited) },
            0,
            "impersonate: {}",
            unsafe { GetLastError() }
        );
        let admin_sid: Vec<u16> = "S-1-5-32-544\0".encode_utf16().collect();
        let mut sid = std::ptr::null_mut();
        let mut is_admin = 1;
        assert_ne!(
            unsafe { ConvertStringSidToSidW(admin_sid.as_ptr(), &mut sid) },
            0
        );
        assert_ne!(
            unsafe { CheckTokenMembership(std::ptr::null_mut(), sid, &mut is_admin) },
            0
        );
        unsafe {
            LocalFree(sid);
        }
        assert_eq!(
            is_admin, 0,
            "restricted effective token still has enabled Administrators membership"
        );
        let user_sid: Vec<u16> = "S-1-5-32-545\0".encode_utf16().collect();
        let mut sid = std::ptr::null_mut();
        let mut is_user = 0;
        assert_ne!(
            unsafe { ConvertStringSidToSidW(user_sid.as_ptr(), &mut sid) },
            0
        );
        assert_ne!(
            unsafe { CheckTokenMembership(std::ptr::null_mut(), sid, &mut is_user) },
            0
        );
        unsafe {
            LocalFree(sid);
        }
        assert_ne!(
            is_user, 0,
            "restricted effective token lacks the Users group"
        );
    }
    // Keep the first pin open while the second handle attempts mutation.
    let pinned = unsafe {
        CreateFileW(
            wide(&source).as_ptr(),
            0x8010_0003,
            if deny_write { 1 } else { 3 },
            std::ptr::null(),
            3,
            0x0220_0000,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(pinned as isize, -1, "fixture pin failed: {}", unsafe {
        GetLastError()
    });
    assert_eq!(std::fs::read_dir(&source).unwrap().count(), 0);
    let substitute = format!("\\??\\{}", target.display());
    let substitute_bytes: Vec<u8> = substitute
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let print_bytes: Vec<u8> = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut data = Vec::new();
    data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
    data.extend_from_slice(
        &((8 + substitute_bytes.len() + 2 + print_bytes.len() + 2) as u16).to_le_bytes(),
    );
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&(substitute_bytes.len() as u16).to_le_bytes());
    data.extend_from_slice(&((substitute_bytes.len() + 2) as u16).to_le_bytes());
    data.extend_from_slice(&(print_bytes.len() as u16).to_le_bytes());
    data.extend_from_slice(&substitute_bytes);
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&print_bytes);
    data.extend_from_slice(&[0, 0]);
    let handle = unsafe {
        CreateFileW(
            wide(&source).as_ptr(),
            0x100,
            7,
            std::ptr::null(),
            3,
            0x0220_0000,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        let error = unsafe { GetLastError() };
        unsafe {
            CloseHandle(pinned);
        }
        if restricted {
            assert_ne!(unsafe { RevertToSelf() }, 0);
            unsafe {
                CloseHandle(limited);
                CloseHandle(token);
            }
        }
        println!(
            "restricted={} deny_write={} read_ok=true mutation_open_error={} fixture={}",
            restricted,
            deny_write,
            error,
            root.display()
        );
        return;
    }
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            handle,
            0x0009_00a4,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    let error = if result == 0 {
        unsafe { GetLastError() }
    } else {
        0
    };
    let mut child: Vec<u16> = "relative-child".encode_utf16().collect();
    let mut uni = Uni {
        len: (child.len() * 2) as u16,
        max: (child.len() * 2) as u16,
        buf: child.as_mut_ptr(),
    };
    let mut attrs = Obj {
        len: std::mem::size_of::<Obj>() as u32,
        root: pinned,
        name: &mut uni,
        flags: 0x40,
        security: std::ptr::null_mut(),
        quality: std::ptr::null_mut(),
    };
    let mut iosb = IoStatus { status: 0, info: 0 };
    let mut child_handle: Handle = std::ptr::null_mut();
    let nt = unsafe {
        NtCreateFile(
            &mut child_handle,
            0x40100000,
            &mut attrs,
            &mut iosb,
            std::ptr::null_mut(),
            0x80,
            7,
            2,
            0x200060,
            std::ptr::null_mut(),
            0,
        )
    };
    if nt >= 0 {
        unsafe {
            CloseHandle(child_handle);
        }
    }
    println!(
        "relative_ntstatus=0x{:08x} target_child_exists={}",
        nt as u32,
        target.join("relative-child").exists()
    );
    let mut delete_data = Vec::new();
    delete_data.extend_from_slice(&0xa0000003u32.to_le_bytes());
    delete_data.extend_from_slice(&0u16.to_le_bytes());
    delete_data.extend_from_slice(&0u16.to_le_bytes());
    let mut deleted = 0;
    let remove = unsafe {
        DeviceIoControl(
            handle,
            0x000900acu32,
            delete_data.as_ptr().cast(),
            delete_data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut deleted,
            std::ptr::null_mut(),
        )
    };
    println!(
        "remove_reparse={} source_child_exists={}",
        remove != 0,
        source.join("relative-child").exists()
    );
    let nt_after = unsafe {
        NtCreateFile(
            &mut child_handle,
            0x40100000,
            &mut attrs,
            &mut iosb,
            std::ptr::null_mut(),
            0x80,
            7,
            2,
            0x200060,
            std::ptr::null_mut(),
            0,
        )
    };
    if nt_after >= 0 {
        unsafe {
            CloseHandle(child_handle);
        }
    }
    println!(
        "relative_after_remove=0x{:08x} source_child_exists={}",
        nt_after as u32,
        source.join("relative-child").exists()
    );
    unsafe {
        CloseHandle(handle);
        CloseHandle(pinned);
    }
    if restricted {
        assert_ne!(unsafe { RevertToSelf() }, 0);
        unsafe {
            CloseHandle(limited);
            CloseHandle(token);
        }
    }
    println!(
        "restricted={} deny_write={} read_ok=true set_reparse={} windows_error={} fixture={}",
        restricted,
        deny_write,
        result != 0,
        error,
        root.display()
    );
}
