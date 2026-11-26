//! The reported ACL shape on a temp tree: a folder with no SYSTEM entry,
//! inheritance removed, holding the selected container. A restricted copy of
//! this process's token stands in for the SYSTEM service; the plain token is
//! the caller who owns the folder.

use super::*;
use std::os::windows::fs::OpenOptionsExt;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::{
    CreateRestrictedToken, SID_AND_ATTRIBUTES, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
    TOKEN_IMPERSONATE, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const AUTHENTICATED_USERS: &str = "S-1-5-11";

struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn process_token() -> Token {
    let mut token = std::ptr::null_mut();
    let access = TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE | TOKEN_ASSIGN_PRIMARY;
    assert_ne!(unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) }, 0);
    Token(token)
}

/// This process's token with Authenticated Users deny-only: like SYSTEM, it matches no ACE on the folder.
fn service_stand_in(from: &Token) -> Token {
    let sid_text: Vec<u16> = AUTHENTICATED_USERS.encode_utf16().chain([0]).collect();
    let mut sid = std::ptr::null_mut();
    assert_ne!(unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) }, 0);
    let disabled = [SID_AND_ATTRIBUTES { Sid: sid, Attributes: 0 }];
    let mut restricted = std::ptr::null_mut();
    let created = unsafe {
        CreateRestrictedToken(
            from.0,
            0,
            1,
            disabled.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &mut restricted,
        )
    };
    unsafe { LocalFree(sid as _) };
    assert_ne!(created, 0, "{}", std::io::Error::last_os_error());
    Token(restricted)
}

fn current_user_sid() -> String {
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().rsplit(',').next().unwrap().trim_matches('"').to_owned()
}

fn icacls(path: &Path, args: &[&str]) {
    let status = std::process::Command::new("icacls")
        .arg(path)
        .args(args)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "icacls {} {args:?}", path.display());
}

/// `root\New folder\test3`, shaped like the reported `D:\Vault\New folder\test3`.
struct Fixture {
    root: PathBuf,
    folder: PathBuf,
    file: PathBuf,
    user: String,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("wc-chpw-acl-{tag}-{}", std::process::id()));
        let folder = root.join("New folder");
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("test3");
        std::fs::write(&file, vec![7u8; 1 << 16]).unwrap();
        let user = format!("*{}", current_user_sid());
        // The file grants its reader directly, like a WinCommander container ACL that names SYSTEM.
        icacls(&file, &["/inheritance:r", "/grant:r", &format!("{user}:F")]);
        icacls(&folder, &["/inheritance:r", "/grant:r", &format!("*{AUTHENTICATED_USERS}:(OI)(CI)F")]);
        Self { root, folder, file, user }
    }

    fn path(&self) -> &str {
        self.file.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The owner keeps WRITE_DAC, so the folder can always be handed back before deletion.
        let _ = std::process::Command::new("icacls")
            .arg(&self.folder)
            .args(["/grant", &format!("{}:(OI)(CI)F", self.user)])
            .stdout(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("icacls")
            .arg(&self.file)
            .args(["/grant", &format!("{}:F", self.user)])
            .stdout(std::process::Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn os(result: std::io::Result<impl Sized>) -> String {
    match result {
        Ok(_) => "ok".into(),
        Err(error) => format!("os error {}", error.raw_os_error().unwrap_or(-1)),
    }
}

/// Prints, for the service stand-in, the OS result of every step the target lookup takes.
#[test]
fn the_service_lookup_steps_and_their_os_errors_on_a_folder_without_a_system_entry() {
    let fixture = Fixture::new("steps");
    let store = crate::vault_access::test_windows_fs_store(fixture.root.join("policy"));
    let caller = process_token();
    let service = service_stand_in(&caller);
    let fs = WindowsVaultFs;
    let (steps, mount_route, change, rename) = with_caller_impersonation(service.0, || {
        let canonical = std::fs::canonicalize(&fixture.file);
        let steps = vec![
            ("std::fs::symlink_metadata(file)", os(std::fs::symlink_metadata(&fixture.file))),
            ("std::fs::canonicalize(file)", os(std::fs::canonicalize(&fixture.file))),
            ("std::fs::File::open(file)", os(std::fs::File::open(&fixture.file))),
            ("std::fs::canonicalize(parent folder)", os(std::fs::canonicalize(&fixture.folder))),
            (
                "WindowsVaultFs::normalize_existing_container_path",
                format!("{:?}", fs.normalize_existing_container_path(&fixture.file).map(drop)),
            ),
            ("WindowsVaultFs::stable_file_identity", format!("{:?}", fs.stable_file_identity(&fixture.file).map(drop))),
            (
                "WindowsVaultFs::normalize_personal_creation_path(canonical)",
                format!("{:?}", fs.normalize_personal_creation_path(canonical.as_ref().unwrap()).map(drop)),
            ),
        ];
        Ok((
            steps,
            store.selected_container_mount_route(fixture.path(), "S-1-5-21-owner", 7).map(drop),
            store.password_change_target(fixture.path(), "S-1-5-21-owner", 7).map(drop),
            store.rename_target(fixture.path(), "S-1-5-21-owner", 7).map(drop),
        ))
    })
    .unwrap();
    for (step, result) in &steps {
        println!("service stand-in | {step} -> {result}");
    }
    println!("service stand-in | selected_container_mount_route (mount) -> {mount_route:?}");
    println!("service stand-in | password_change_target -> {change:?}");
    println!("service stand-in | rename_target -> {rename:?}");
    assert_eq!(steps[3].1, "os error 5", "the folder refuses the service");
    assert_eq!(steps[6].1, "Err(Validation)", "and that refusal becomes Validation");
    assert_eq!(mount_route, Ok(()), "a mount never canonicalizes the parent");
    assert_eq!(change, Err(VaultError::Validation));
    assert_eq!(rename, Err(VaultError::Validation));
}

#[test]
fn a_caller_who_owns_the_folder_resolves_the_target_even_when_the_service_cannot() {
    let fixture = Fixture::new("owner");
    let store = crate::vault_access::test_windows_fs_store(fixture.root.join("policy"));
    let caller = process_token();
    let service = service_stand_in(&caller);
    let (change, rename) = with_caller_impersonation(service.0, || {
        Ok((
            store.password_change_target_as_caller(fixture.path(), "S-1-5-21-owner", 7, caller.0),
            store.rename_target_as_caller(fixture.path(), "S-1-5-21-owner", 7, caller.0),
        ))
    })
    .unwrap();
    let target = change.expect("the caller can open its own folder");
    assert!(target.container_path.ends_with(r"New folder\test3"));
    assert_eq!(rename.expect("rename resolves too"), (target.clone(), RenameRecord::Unregistered));
    assert!(hold_password_change_files(&target, &[], caller.0).is_ok());
}

#[test]
fn a_caller_the_folder_refuses_gets_access_denied_never_not_found() {
    let fixture = Fixture::new("denied");
    let store = crate::vault_access::test_windows_fs_store(fixture.root.join("policy"));
    let refused = service_stand_in(&process_token());
    assert_eq!(
        store.password_change_target_as_caller(fixture.path(), "S-1-5-21-owner", 7, refused.0),
        Err(TargetLookupError::File(FileFault::AccessDenied))
    );
    assert_eq!(
        store.rename_target_as_caller(fixture.path(), "S-1-5-21-owner", 7, refused.0),
        Err(TargetLookupError::File(FileFault::AccessDenied))
    );
}

#[test]
fn a_missing_container_or_folder_is_not_found() {
    let fixture = Fixture::new("missing");
    let store = crate::vault_access::test_windows_fs_store(fixture.root.join("policy"));
    let caller = process_token();
    for missing in [fixture.folder.join("gone.hc"), fixture.root.join("no folder").join("gone.hc")] {
        assert_eq!(
            store.password_change_target_as_caller(missing.to_str().unwrap(), "S-1-5-21-owner", 7, caller.0),
            Err(TargetLookupError::File(FileFault::NotFound)),
            "{}",
            missing.display()
        );
    }
}

#[test]
fn a_container_another_program_holds_exclusively_is_in_use() {
    let fixture = Fixture::new("in-use");
    let store = crate::vault_access::test_windows_fs_store(fixture.root.join("policy"));
    let caller = process_token();
    let exclusive = std::fs::OpenOptions::new().read(true).share_mode(0).open(&fixture.file).unwrap();
    assert_eq!(
        store.password_change_target_as_caller(fixture.path(), "S-1-5-21-owner", 7, caller.0),
        Err(TargetLookupError::File(FileFault::InUse))
    );
    assert_eq!(container_fault_as_caller(fixture.path(), caller.0), Some(FileFault::InUse));
    drop(exclusive);
    assert_eq!(container_fault_as_caller(fixture.path(), caller.0), None);
}

#[test]
fn os_errors_map_to_the_contract_codes() {
    for (code, fault) in [
        (2, Some(FileFault::NotFound)),
        (3, Some(FileFault::NotFound)),
        (21, Some(FileFault::NotFound)),
        (123, Some(FileFault::NotFound)),
        (5, Some(FileFault::AccessDenied)),
        (32, Some(FileFault::InUse)),
        (33, Some(FileFault::InUse)),
        (1, None),
    ] {
        assert_eq!(FileFault::from_io(&std::io::Error::from_raw_os_error(code)), fault, "{code}");
    }
    assert_eq!(FileFault::NotFound.code(), "vault_file_not_found");
    assert_eq!(FileFault::AccessDenied.code(), "vault_access_denied");
    assert_eq!(FileFault::InUse.code(), "vault_file_in_use");
}

#[test]
fn a_rename_is_saved_by_the_service_even_when_it_cannot_open_the_folder() {
    let fixture = Fixture::new("rename");
    let store = crate::vault_access::test_windows_fs_store(fixture.root.join("policy"));
    let caller = process_token();
    let service = service_stand_in(&caller);
    let saved = with_caller_impersonation(service.0, || {
        let (target, record) = store
            .rename_target_as_caller(fixture.path(), "S-1-5-21-owner", 7, caller.0)
            .map_err(|_| VaultError::Validation)?;
        assert_eq!(record, RenameRecord::Unregistered);
        // The impersonation above ended; act as the stand-in again for the save.
        with_caller_impersonation(service.0, || store.rename_unregistered_container(&target, "S-1-5-21-owner", "Test three"))
    });
    assert_eq!(saved, Ok(()));
    let names = store.display_names_for_caller("S-1-5-21-owner", |_| false);
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].name, "Test three");
}