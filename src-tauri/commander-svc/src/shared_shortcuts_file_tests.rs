// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use windows_sys::Win32::Security::{
    Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW, DACL_SECURITY_INFORMATION,
    GROUP_SECURITY_INFORMATION,
};

fn security_sddl(file: &File) -> String {
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        assert_eq!(status, 0);
        let mut text = std::ptr::null_mut();
        let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            1,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut text,
            std::ptr::null_mut(),
        );
        assert_ne!(ok, 0);
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let result = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
        LocalFree(text.cast());
        LocalFree(descriptor);
        result
    }
}

fn file_info(file: &File) -> BY_HANDLE_FILE_INFORMATION {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    assert_ne!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) },
        0
    );
    info
}

#[test]
fn handle_rename_preserves_the_verified_file_and_never_replaces_an_occupant() {
    let root = std::env::temp_dir().join(format!("wincmd-link-rename-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.lnk");
    let backup = root.join("backup.lnk");
    let occupant = root.join("occupant.lnk");
    std::fs::write(&source, b"original shortcut bytes").unwrap();
    std::fs::write(&occupant, b"unrelated occupant").unwrap();
    let directory_guards = hold_parent(&source).unwrap();
    let handle = unsafe {
        CreateFileW(
            wide(&source).as_ptr(),
            GENERIC_READ | DELETE,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(handle, INVALID_HANDLE_VALUE);
    let link = OpenedLink {
        file: unsafe { File::from_raw_handle(handle) },
        identity: LinkIdentity {
            sha256: String::new(),
            volume: 0,
            file_index: 0,
            attributes: 0,
        },
    };
    let before_sddl = security_sddl(&link.file);
    let before_info = file_info(&link.file);
    // Exercise different UTF-16 buffer alignments, including lengths that
    // previously left no room for the terminating NUL.
    for name in ["x.lnk", "xy.lnk", "xyz.lnk", "xyzw.lnk"] {
        let destination = root.join(name);
        move_link(&link, &destination).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"original shortcut bytes");
    }
    move_link(&link, &backup).unwrap();
    assert!(!source.exists());
    assert_eq!(std::fs::read(&backup).unwrap(), b"original shortcut bytes");
    let restored_view = File::open(&backup).unwrap();
    let after_info = file_info(&restored_view);
    assert_eq!(before_info.dwFileAttributes, after_info.dwFileAttributes);
    assert_eq!(
        before_info.dwVolumeSerialNumber,
        after_info.dwVolumeSerialNumber
    );
    assert_eq!(before_info.nFileIndexHigh, after_info.nFileIndexHigh);
    assert_eq!(before_info.nFileIndexLow, after_info.nFileIndexLow);
    assert_eq!(before_sddl, security_sddl(&restored_view));
    drop(restored_view);
    assert!(move_link(&link, &occupant).is_err());
    assert_eq!(std::fs::read(&occupant).unwrap(), b"unrelated occupant");
    delete_link(&link).unwrap();
    drop(link);
    assert!(!backup.exists());
    assert_eq!(std::fs::read(&occupant).unwrap(), b"unrelated occupant");
    drop(directory_guards);
    std::fs::remove_file(occupant).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn interrupted_calculator_moves_recover_and_collisions_preserve_both_files() {
    let root = std::env::temp_dir().join(format!(
        "wincmd-shortcut-state-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let public = root.join("public");
    let private = root.join("private");
    std::fs::create_dir_all(&public).unwrap();
    std::fs::create_dir_all(&private).unwrap();
    let original = public.join("WinCommander.lnk");
    let backup = private.join("original.lnk");
    let alias_private = private.join("calculator.lnk");
    let alias_public = public.join("calculator..lnk");
    let orphan_temp = private.join("shortcut-00000000000000000000000000000001.tmp");
    std::fs::write(&original, b"original exact bytes").unwrap();
    std::fs::write(&orphan_temp, b"interrupted temporary write").unwrap();
    std::fs::write(&alias_private, b"calculator alias bytes").unwrap();
    let open = |path: &Path| {
        let handle = unsafe {
            CreateFileW(
                wide(path).as_ptr(),
                GENERIC_READ | DELETE,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        OpenedLink {
            file: unsafe { File::from_raw_handle(handle) },
            identity: LinkIdentity {
                sha256: String::new(),
                volume: 0,
                file_index: 0,
                attributes: 0,
            },
        }
    };
    // Crash after a journaled alias exists privately while the original is public.
    let original_handle = open(&original);
    let alias_handle = open(&alias_private);
    move_link(&original_handle, &backup).unwrap();
    move_link(&alias_handle, &alias_public).unwrap();
    assert_eq!(
        std::fs::read(&alias_public).unwrap(),
        b"calculator alias bytes"
    );
    assert_eq!(std::fs::read(&backup).unwrap(), b"original exact bytes");
    // Crash after both moves: Original mode deletes only the known alias, then restores bytes.
    delete_link(&alias_handle).unwrap();
    drop(alias_handle);
    move_link(&original_handle, &original).unwrap();
    drop(original_handle);
    assert_eq!(std::fs::read(&original).unwrap(), b"original exact bytes");
    assert!(!alias_public.exists());
    // A foreign destination is never replaced during a retry.
    std::fs::write(&backup, b"foreign backup occupant").unwrap();
    let original_handle = open(&original);
    assert!(move_link(&original_handle, &backup).is_err());
    assert_eq!(std::fs::read(&original).unwrap(), b"original exact bytes");
    assert_eq!(std::fs::read(&backup).unwrap(), b"foreign backup occupant");
    drop(original_handle);
    std::fs::remove_file(original).unwrap();
    std::fs::remove_file(backup).unwrap();
    std::fs::remove_file(orphan_temp).unwrap();
    std::fs::remove_dir(public).unwrap();
    std::fs::remove_dir(private).unwrap();
    std::fs::remove_dir(root).unwrap();
}
