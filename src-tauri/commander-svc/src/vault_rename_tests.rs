use super::*;
use crate::vault_access::PersonalVaultRegistrationState;
use serde_json::json;
use wincmd_shared::vault_access::{
    PersonalVaultRecord, VaultAccessEntry, VaultAccessPolicy, VaultContainerKind, VaultGrantInput,
    VaultMountPolicy, VaultPresentation,
};

const OWNER: &str = "S-1-5-21-owner";
const OTHER: &str = "S-1-5-21-other";
const PERSONAL_PATH: &str = r"D:\Vaults\notes.hc";
const POLICY_PATH: &str = r"D:\Vaults\shared.hc";

fn no_grant(_: &str) -> (bool, Option<VaultAccess>) {
    (false, None)
}

fn writable(_: &PasswordChangeTarget) -> Result<(), VaultError> {
    Ok(())
}

fn read_only(_: &PasswordChangeTarget) -> Result<(), VaultError> {
    Err(VaultError::AclReadback)
}

fn request(path: &str, name: &str) -> RenameRequest {
    validate_request(&json!({ "container_path": path, "name": name })).unwrap()
}

fn store_with_personal_vault() -> (VaultAccessStore, String) {
    let store = crate::vault_access::test_policy_store();
    let identity = store
        .password_change_target(PERSONAL_PATH, OWNER, 7)
        .unwrap()
        .container_identity;
    store.insert_personal_record_for_test(PersonalVaultRecord {
        container_path: PERSONAL_PATH.into(),
        container_identity: identity.clone(),
        owner_sid: OWNER.into(),
        scope: VaultPresentation::PerUser,
        created_by_session: 7,
        syncthing_opt_in: Some(false),
    });
    (store, identity)
}

fn store_with_policy_vault() -> VaultAccessStore {
    let store = crate::vault_access::test_policy_store();
    store
        .apply(
            VaultAccessPolicy {
                schema_version: 1,
                policy_id: "p".into(),
                version: 1,
                expected_previous_version: 0,
                entries: vec![VaultAccessEntry {
                    id: "shared".into(),
                    label: "Shared".into(),
                    container_path: POLICY_PATH.into(),
                    container_identity: None,
                    container_kind: VaultContainerKind::Standard,
                    primary_owner_sid: None,
                    owner_account: "Admin".into(),
                    access_pattern: None,
                    syncthing_opt_in: None,
                    cloud_backup: None,
                    grants: ["Admin", "Partner"]
                        .map(|name| VaultGrantInput { principal_name: name.into(), access: VaultAccess::Write })
                        .into(),
                    mount: VaultMountPolicy { presentation: VaultPresentation::Machine, preferred_letter: Some("V".into()) },
                }],
            },
            7,
        )
        .unwrap();
    store
}

fn personal_name(store: &VaultAccessStore) -> Option<String> {
    store
        .display_names_for_caller(OWNER, |_| false)
        .into_iter()
        .find(|row| row.path == PERSONAL_PATH)
        .map(|row| row.name)
}

#[test]
fn the_owner_renames_a_registered_personal_vault() {
    let (store, _) = store_with_personal_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let reply = rename_for_caller(&store, &broker, OWNER, 7, &request(PERSONAL_PATH, "  Taxes "), &no_grant, &writable).unwrap();
    assert_eq!(reply, json!({ "status": "renamed", "name": "Taxes" }));
    assert_eq!(personal_name(&store).as_deref(), Some("Taxes"));
}

#[test]
fn another_account_cannot_rename_a_personal_vault() {
    let (store, _) = store_with_personal_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let error = rename_for_caller(&store, &broker, OTHER, 8, &request(PERSONAL_PATH, "Mine"), &no_grant, &writable).unwrap_err();
    assert_eq!(error.kind, PERSONAL_VAULT_UNAUTHORIZED);
    assert_eq!(personal_name(&store), None);
}

#[test]
fn a_write_grant_renames_a_policy_vault_and_a_read_grant_does_not() {
    let store = store_with_policy_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let rename = |access: Option<VaultAccess>, name: &str| {
        rename_for_caller(&store, &broker, OTHER, 8, &request(POLICY_PATH, name), &move |_| (access.is_some(), access), &read_only)
    };
    assert_eq!(rename(Some(VaultAccess::Read), "Read only").unwrap_err().kind, PERSONAL_VAULT_UNAUTHORIZED);
    assert_eq!(rename(None, "No grant").unwrap_err().kind, PERSONAL_VAULT_UNAUTHORIZED);
    assert_eq!(store.policy().unwrap().entries[0].label, "Shared");
    assert_eq!(store.policy().unwrap().version, 1);
    rename(Some(VaultAccess::Write), "Finance").unwrap();
    assert_eq!(store.policy().unwrap().entries[0].label, "Finance");
}

#[test]
fn a_mounted_vault_is_refused_and_keeps_its_name() {
    let (store, identity) = store_with_personal_vault();
    let broker = crate::vault_mount::test_broker_with_mount(Some(&identity));
    let error = rename_for_caller(&store, &broker, OWNER, 7, &request(PERSONAL_PATH, "Taxes"), &no_grant, &writable).unwrap_err();
    assert_eq!(error.kind, "volume_mounted");
    assert_eq!(personal_name(&store), None);

    let policy = store_with_policy_vault();
    let policy_identity = policy.password_change_target(POLICY_PATH, OWNER, 7).unwrap().container_identity;
    let broker = crate::vault_mount::test_broker_with_mount(Some(&policy_identity));
    let error = rename_for_caller(&policy, &broker, OTHER, 8, &request(POLICY_PATH, "Finance"), &|_| {
        (true, Some(VaultAccess::Write))
    }, &read_only)
    .unwrap_err();
    assert_eq!(error.kind, "volume_mounted");
    assert_eq!(policy.policy().unwrap().entries[0].label, "Shared");
}

#[test]
fn an_unregistered_container_is_named_for_the_caller_only_after_a_write_proof() {
    let store = crate::vault_access::test_policy_store();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let names = |caller: &str| store.display_names_for_caller(caller, |_| false);
    let error = rename_for_caller(&store, &broker, OWNER, 7, &request(PERSONAL_PATH, "Taxes"), &no_grant, &read_only)
        .unwrap_err();
    assert_eq!(error.kind, "vault_caller_access_denied");
    assert!(names(OWNER).is_empty());
    for path in [PERSONAL_PATH, r"\Device\Harddisk1\Partition1"] {
        rename_for_caller(&store, &broker, OWNER, 7, &request(path, "Taxes"), &no_grant, &writable).unwrap();
    }
    assert_eq!(names(OWNER).len(), 2);
    assert!(names(OTHER).is_empty(), "another account never sees this caller's names");
    assert_eq!(store.rename_target(PERSONAL_PATH, OWNER, 7).unwrap().1, RenameRecord::Unregistered);
    assert_eq!(store.personal_registration_state(PERSONAL_PATH, OWNER), Ok(PersonalVaultRegistrationState::Unregistered));
}

#[test]
fn a_mounted_unregistered_container_is_refused_before_the_write_proof() {
    let store = crate::vault_access::test_policy_store();
    let identity = store.password_change_target(PERSONAL_PATH, OWNER, 7).unwrap().container_identity;
    let broker = crate::vault_mount::test_broker_with_mount(Some(&identity));
    let proved = std::cell::Cell::new(false);
    let error = rename_for_caller(&store, &broker, OWNER, 7, &request(PERSONAL_PATH, "Taxes"), &no_grant, &|_| {
        proved.set(true);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.kind, "volume_mounted");
    assert!(!proved.get());
    assert!(store.display_names_for_caller(OWNER, |_| false).is_empty());
}

#[test]
fn a_copied_as_path_selection_names_the_same_file() {
    let quoted = format!("  \"{PERSONAL_PATH}\" ");
    assert_eq!(request(&quoted, "Taxes").container_path, PERSONAL_PATH);
    assert!(validate_request(&json!({ "container_path": "\"notes.hc\"", "name": "Taxes" })).is_none());
}

/// Real files on this PC's filesystem, opened read/write in this test's own token.
#[cfg(windows)]
mod real_files {
    use super::*;

    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wc-rename-{tag}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(dir.join("New folder")).unwrap();
        TempDir(dir)
    }

    pub(super) struct ProcessToken(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for ProcessToken {
        fn drop(&mut self) {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }

    /// This test process's own token, standing in for the pipe caller's.
    pub(super) fn own_token() -> ProcessToken {
        use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = std::ptr::null_mut();
        let opened = unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE, &mut token)
        };
        assert_ne!(opened, 0);
        ProcessToken(token)
    }

    fn rename(store: &VaultAccessStore, path: &str, name: &str, token: &ProcessToken) -> Result<serde_json::Value, VerbError> {
        let broker = crate::vault_mount::test_broker_with_mount(None);
        let request = validate_request(&json!({ "container_path": path, "name": name })).expect("valid request");
        rename_for_caller(store, &broker, OWNER, 7, &request, &no_grant, &|target| {
            crate::vault_access::hold_password_change_files(target, &[], token.0).map(drop)
        })
    }

    #[test]
    fn an_extensionless_unregistered_container_in_a_folder_with_a_space_is_renamed() {
        let dir = temp_dir("plain");
        let file = dir.0.join("New folder").join("test3");
        std::fs::write(&file, vec![0x5a; 1 << 20]).unwrap();
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        let token = own_token();
        let path = file.to_string_lossy().into_owned();
        for (selection, name) in [(path.clone(), "Test three"), (format!("\"{path}\""), "Quoted")] {
            assert_eq!(rename(&store, &selection, name, &token).unwrap(), json!({ "status": "renamed", "name": name }));
        }
        let names = store.display_names_for_caller(OWNER, |_| false);
        assert_eq!(names.len(), 1, "the quoted selection renamed the same file");
        assert_eq!(names[0].name, "Quoted");
        assert!(names[0].path.ends_with(r"New folder\test3"), "{}", names[0].path);
        assert!(dir.0.join("policy").join("vault-personal-names-v1.json").is_file());
        assert_eq!(std::fs::read(&file).unwrap(), vec![0x5a; 1 << 20], "the container is never written");

        let restarted = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        restarted.load_at_startup();
        assert_eq!(restarted.display_names_for_caller(OWNER, |_| false), names);
    }

    #[test]
    fn a_container_this_account_cannot_write_keeps_no_name() {
        let dir = temp_dir("readonly");
        let file = dir.0.join("New folder").join("test3");
        std::fs::write(&file, vec![1; 4096]).unwrap();
        let writable = std::fs::metadata(&file).unwrap().permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(&file, read_only).unwrap();
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        let error = rename(&store, &file.to_string_lossy(), "Nope", &own_token()).unwrap_err();
        assert_eq!(error.kind, "vault_caller_access_denied");
        assert!(store.display_names_for_caller(OWNER, |_| false).is_empty());
        assert!(!dir.0.join("policy").join("vault-personal-names-v1.json").exists());
        std::fs::set_permissions(&file, writable).unwrap();
    }

    #[test]
    fn a_missing_container_is_reported_as_not_the_selected_vault() {
        let dir = temp_dir("missing");
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        let missing = dir.0.join("New folder").join("absent");
        let error = rename(&store, &missing.to_string_lossy(), "Ghost", &own_token()).unwrap_err();
        assert_eq!(error.kind, "vault_policy_identity_changed");
    }
}

#[test]
fn requests_need_an_absolute_path_a_valid_name_and_no_other_fields() {
    assert_eq!(request(PERSONAL_PATH, " Taxes ").name, "Taxes");
    for args in [
        json!({ "container_path": "notes.hc", "name": "Taxes" }),
        json!({ "container_path": PERSONAL_PATH, "name": "   " }),
        json!({ "container_path": PERSONAL_PATH, "name": "two\nlines" }),
        json!({ "container_path": PERSONAL_PATH, "name": "a".repeat(129) }),
        json!({ "container_path": PERSONAL_PATH, "name": 7 }),
        json!({ "container_path": PERSONAL_PATH }),
        json!({ "container_path": PERSONAL_PATH, "name": "Taxes", "owner_sid": OTHER }),
    ] {
        assert!(validate_request(&args).is_none(), "{args}");
    }
}

#[test]
fn handlers_refuse_without_an_authenticated_caller() {
    let (store, _) = store_with_personal_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let args = json!({ "container_path": PERSONAL_PATH, "name": "Taxes" });
    assert_eq!(handle(&store, &broker, args, None).unwrap_err().kind, PERSONAL_VAULT_SESSION_ABSENT);
    assert_eq!(handle_display_names(&store, None).unwrap_err().kind, PERSONAL_VAULT_SESSION_ABSENT);
    assert_eq!(personal_name(&store), None);
}
