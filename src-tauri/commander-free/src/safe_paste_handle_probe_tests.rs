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
