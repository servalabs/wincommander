// SPDX-License-Identifier: AGPL-3.0-or-later
//! TEST-ONLY primitive probe. This is not a Safe Paste implementation.
//! It does not authorize the destination, confine a mutable parent namespace,
//! protect scrubbed contents, or provide crash recovery. Do not route product
//! operations through this module.

    use std::{ffi::OsStr, fs::{File, OpenOptions}, io::Write, os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle}, path::Path};
    use windows_sys::{Wdk::Storage::FileSystem::{FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION, FILE_RENAME_INFORMATION_0}, Win32::{Storage::FileSystem::{DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE}, System::IO::IO_STATUS_BLOCK}};

    fn pin_directory(path: &Path) -> File {
        OpenOptions::new().access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path).unwrap()
    }
    fn pin_stage(path: &Path) -> File {
        OpenOptions::new().access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path).unwrap()
    }

    #[test]
    fn existing_writer_prevents_exclusive_stage_pin() {
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("item");
        std::fs::write(&path, b"synthetic").unwrap();
        let writer = OpenOptions::new().write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path).unwrap();
        let pin = OpenOptions::new().access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path);
        assert!(pin.is_err(), "a pre-existing writer must prevent this pin");
        drop(writer);
        drop(pin_stage(&path));
    }

    /// LAB ONLY: an existing writable section may outlive its file handle.
    /// Record whether it blocks the proposed pin or can change pinned bytes;
    /// neither outcome proves a scrubber or namespace boundary is secure.
    #[test]
    #[ignore = "requires an explicitly authorized disposable Windows fixture"]
    fn mapped_writer_against_stage_pin_diagnostic() {
        use std::{ffi::c_void, ptr};
        use windows_sys::Win32::{Foundation::CloseHandle, Storage::FileSystem::FILE_SHARE_DELETE};

        #[link(name = "kernel32")]
        extern "system" {
            fn CreateFileMappingW(file: *mut c_void, security: *const c_void, protect: u32,
                maximum_high: u32, maximum_low: u32, name: *const u16) -> *mut c_void;
            fn MapViewOfFile(mapping: *mut c_void, access: u32, offset_high: u32,
                offset_low: u32, count: usize) -> *mut c_void;
            fn UnmapViewOfFile(address: *const c_void) -> i32;
        }
        struct Mapping { handle: *mut c_void, view: *mut c_void }
        impl Drop for Mapping {
            fn drop(&mut self) { unsafe { UnmapViewOfFile(self.view); CloseHandle(self.handle); } }
        }

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("item");
        std::fs::write(&path, b"original").unwrap();
        let writer = OpenOptions::new().read(true).write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path).unwrap();
        let handle = unsafe { CreateFileMappingW(writer.as_raw_handle(), ptr::null(), 4, 0, 0, ptr::null()) };
        assert!(!handle.is_null(), "CreateFileMappingW failed: {}", std::io::Error::last_os_error());
        let view = unsafe { MapViewOfFile(handle, 2, 0, 0, 0) };
        if view.is_null() { unsafe { CloseHandle(handle); } panic!("MapViewOfFile failed: {}", std::io::Error::last_os_error()); }
        let mapping = Mapping { handle, view };
        drop(writer);

        let pin = OpenOptions::new().access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path);
        match pin {
            Err(error) => eprintln!("mapped writer blocked the pin: {error}"),
            Ok(owner) => {
                unsafe { *mapping.view.cast::<u8>() = b'X'; }
                let after = std::fs::read(&path).unwrap();
                eprintln!("pin succeeded; mapped write changed content: {}", after[0] == b'X');
                assert_eq!(after[0], b'X', "mapped view and file should describe the same fixture");
                drop(owner);
            }
        }
        drop(mapping);
    }
    fn rename_handle_to(stage: &File, parent: &File, name: &OsStr) -> Result<(), i32> {
        let name: Vec<u16> = name.encode_wide().collect();
        assert!(!name.is_empty() && !name.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92)));
        let count = name.len() * 2;
        let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>() + count;
        let mut memory = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        let info = memory.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        let mut status = IO_STATUS_BLOCK::default();
        let result = unsafe {
            (*info).Anonymous = FILE_RENAME_INFORMATION_0 { ReplaceIfExists: false };
            (*info).RootDirectory = parent.as_raw_handle();
            (*info).FileNameLength = count as u32;
            std::ptr::copy_nonoverlapping(name.as_ptr(), std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(), name.len());
            NtSetInformationFile(stage.as_raw_handle(), &mut status, info.cast(), bytes as u32, FileRenameInformation)
        };
        if result < 0 { Err(result) } else { Ok(()) }
    }

    // LAB MODEL ONLY. A synchronous mock transforms the very handle later
    // handed to publication. This does not establish a broker, protect the
    // parent namespace, isolate a helper process, handle helper-created
    // replacement files, or recover after a crash. ExifTool/QPDF do not use
    // this interface. Never route product operations through this model.
    struct MockHandleScrubbedStage(File);
    fn mock_scrub_into_new_held_file(
        path: &Path,
        scrub: impl FnOnce(&mut File) -> std::io::Result<()>,
    ) -> std::io::Result<MockHandleScrubbedStage> {
        let mut file = OpenOptions::new()
            .read(true).write(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .create_new(true)
            .open(path)?;
        scrub(&mut file)?;
        file.sync_all()?;
        Ok(MockHandleScrubbedStage(file))
    }

    #[test]
    fn mock_scrub_and_publish_use_the_same_restrictively_shared_handle() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging");
        let destination = root.path().join("destination");
        std::fs::create_dir(&staging).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let source = staging.join("item");
        let published = destination.join("item");
        let parent = pin_directory(&destination);
        let scrubbed = mock_scrub_into_new_held_file(&source, |file| {
            file.write_all(b"synthetic scrubbed bytes")?;
            assert!(OpenOptions::new().write(true).open(&source).is_err());
            assert!(std::fs::rename(&source, staging.join("replacement")).is_err());
            Ok(())
        }).unwrap();
        rename_handle_to(&scrubbed.0, &parent, OsStr::new("item")).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&published).unwrap(), b"synthetic scrubbed bytes");
        assert!(OpenOptions::new().write(true).open(&published).is_err());
        drop(scrubbed);
    }

    #[test]
    fn mock_scrub_failure_cannot_return_a_publishable_handle() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        let result = mock_scrub_into_new_held_file(&stage, |file| {
            file.write_all(b"synthetic partial output")?;
            Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
        });
        assert!(result.is_err());
        // Any partial output remains only in the disposable fixture, and no
        // MockHandleScrubbedStage was returned for publication.
        assert!(stage.exists());
    }
    #[test]
    fn owned_handle_retains_identity_across_publish_and_rollback() {
        const CHILD: &str = "WINCMD_SAFE_PASTE_HANDLE_FIXTURE";
        if let Some(root) = std::env::var_os(CHILD) {
            let root = std::path::PathBuf::from(root);
            assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
            assert!(root.file_name().and_then(OsStr::to_str).is_some_and(|n| n.starts_with("wc-safe-paste-probe-")));
            assert_eq!(std::fs::read(root.join("fixture-marker")).unwrap(), b"safe-paste-probe");
            let path = root.join("dest").join("item");
            assert!(OpenOptions::new().write(true).open(&path).is_err());
            assert!(OpenOptions::new().access_mode(DELETE).open(&path).is_err());
            return;
        }
        let root = tempfile::Builder::new().prefix("wc-safe-paste-probe-").tempdir().unwrap();
        std::fs::write(root.path().join("fixture-marker"), b"safe-paste-probe").unwrap();
        let staging = root.path().join("staging"); let dest = root.path().join("dest");
        std::fs::create_dir(&staging).unwrap(); std::fs::create_dir(&dest).unwrap();
        let src = staging.join("item"); let dst = dest.join("item");
        File::create(&src).unwrap().write_all(b"fixture").unwrap();
        let stage_parent = pin_directory(&staging); let dest_parent = pin_directory(&dest);
        let owner = pin_stage(&src);
        rename_handle_to(&owner, &dest_parent, OsStr::new("item")).expect("relative publish");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "safe_clip::safe_paste_handle_probe_tests::owned_handle_retains_identity_across_publish_and_rollback", "--test-threads=1"])
            .env(CHILD, root.path()).status().unwrap();
        assert!(child.success());
        rename_handle_to(&owner, &stage_parent, OsStr::new("item")).expect("relative rollback");
        assert!(!dst.exists()); assert_eq!(std::fs::read(&src).unwrap(), b"fixture");
        drop(owner);
    }
    #[test]
    fn collision_does_not_replace_existing_item() {
        let root = tempfile::tempdir().unwrap(); let staging = root.path().join("staging"); let dest = root.path().join("dest");
        std::fs::create_dir(&staging).unwrap(); std::fs::create_dir(&dest).unwrap();
        let src = staging.join("item"); let dst = dest.join("item");
        std::fs::write(&src, b"ours").unwrap(); std::fs::write(&dst, b"other").unwrap();
        let owner = pin_stage(&src); let parent = pin_directory(&dest);
        assert!(rename_handle_to(&owner, &parent, OsStr::new("item")).is_err());
        assert_eq!(std::fs::read(&src).unwrap(), b"ours"); assert_eq!(std::fs::read(&dst).unwrap(), b"other");
    }
    #[test]
    fn occupied_rollback_name_is_preserved_and_published_object_remains() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging"); let dest = root.path().join("dest");
        std::fs::create_dir(&staging).unwrap(); std::fs::create_dir(&dest).unwrap();
        let src = staging.join("item"); let dst = dest.join("item");
        std::fs::write(&src, b"ours").unwrap();
        let stage_parent = pin_directory(&staging); let dest_parent = pin_directory(&dest);
        let owner = pin_stage(&src);
        rename_handle_to(&owner, &dest_parent, OsStr::new("item")).unwrap();
        std::fs::write(&src, b"other").unwrap();
        assert!(rename_handle_to(&owner, &stage_parent, OsStr::new("item")).is_err());
        assert_eq!(std::fs::read(&src).unwrap(), b"other");
        assert_eq!(std::fs::read(&dst).unwrap(), b"ours");
    }

    /// LAB ONLY: demonstrates that a retained directory handle is not a
    /// guarantee against later reparse metadata changes. The fixture uses only
    /// directories under a private temporary root. It uses the current token;
    /// it does not establish another user's effective ACL rights.
    #[test]
    #[ignore = "requires an explicitly authorized disposable NTFS fixture"]
    fn held_parent_can_become_a_junction() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::{Storage::FileSystem::{FILE_SHARE_DELETE, FILE_WRITE_ATTRIBUTES}, System::{Ioctl::{FSCTL_DELETE_REPARSE_POINT, FSCTL_SET_REPARSE_POINT}, IO::DeviceIoControl}};
        let root = tempfile::Builder::new().prefix("wc-safe-paste-reparse-probe-").tempdir().unwrap().keep();
        let parent_path = root.join("held-parent");
        let target_path = root.join("synthetic-target");
        let staging_path = root.join("staging");
        std::fs::create_dir(&parent_path).unwrap();
        std::fs::create_dir(&target_path).unwrap();
        std::fs::create_dir(&staging_path).unwrap();
        std::fs::write(target_path.join("sentinel"), b"synthetic").unwrap();
        let parent = pin_directory(&parent_path);
        let mutate = OpenOptions::new().access_mode(FILE_WRITE_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&parent_path).expect("open disposable directory attributes");
        let mut substitute = std::ffi::OsString::from_wide(&[92, 63, 63, 92]);
        substitute.push(target_path.as_os_str());
        let sub: Vec<u8> = substitute.encode_wide().flat_map(u16::to_le_bytes).collect();
        let print: Vec<u8> = target_path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect();
        let length = 8 + sub.len() + 2 + print.len() + 2;
        assert!(length <= u16::MAX as usize);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xa0000003u32.to_le_bytes());
        bytes.extend_from_slice(&(length as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(sub.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&((sub.len() + 2) as u16).to_le_bytes());
        bytes.extend_from_slice(&(print.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&sub); bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&print); bytes.extend_from_slice(&[0, 0]);
        let mut returned = 0;
        let changed = unsafe { DeviceIoControl(mutate.as_raw_handle(), FSCTL_SET_REPARSE_POINT,
            bytes.as_ptr().cast(), bytes.len() as u32, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) } != 0;
        let result = if changed {
            use std::os::windows::fs::MetadataExt;
            let reparse = parent.metadata().unwrap().file_attributes() & 0x400 != 0;
            let stage_path = staging_path.join("source");
            std::fs::write(&stage_path, b"synthetic").unwrap();
            let stage = pin_stage(&stage_path);
            let rename_ok = rename_handle_to(&stage, &parent, OsStr::new("new-entry")).is_ok();
            let target_observed = target_path.join("new-entry").exists();
            drop(stage);
            Some((reparse, rename_ok, target_observed))
        } else { None };
        // Always remove the fixture's reparse point before any recursive cleanup.
        let mut delete = Vec::new(); delete.extend_from_slice(&0xa0000003u32.to_le_bytes());
        delete.extend_from_slice(&0u16.to_le_bytes()); delete.extend_from_slice(&0u16.to_le_bytes());
        let removed = !changed || unsafe { DeviceIoControl(mutate.as_raw_handle(), FSCTL_DELETE_REPARSE_POINT,
            delete.as_ptr().cast(), delete.len() as u32, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) } != 0;
        drop(mutate); drop(parent);
        assert!(removed, "temporary reparse point could not be removed; fixture was retained");
        let sentinel = std::fs::read(target_path.join("sentinel")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(sentinel, b"synthetic");
        let (reparse, rename_ok, target_observed) = result.expect("local fixture filesystem refused junction mutation");
        assert!(reparse, "the held directory must expose the changed reparse attribute");
        println!("relative rename accepted={rename_ok}, synthetic target received entry={target_observed}");
    }
