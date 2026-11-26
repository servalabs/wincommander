use super::*;
use crate::vault_access::FileFault;
use serde_json::json;
use std::cell::{Cell, RefCell};
use wincmd_shared::vault_access::{
    PersonalVaultRecord, VaultAccessEntry, VaultAccessPolicy, VaultContainerKind, VaultGrantInput,
    VaultMountPolicy, VaultPresentation,
};

const OWNER: &str = "S-1-5-21-owner";
const OTHER: &str = "S-1-5-21-other";
const PERSONAL_PATH: &str = r"D:\Vaults\notes.hc";
const POLICY_PATH: &str = r"D:\Vaults\shared.hc";

fn request(path: &str, new_name: &str) -> serde_json::Value {
    json!({ "container_path": path, "new_name": new_name, "password": "pw-canary" })
}

/// The production flow with every Windows step and the broker's probe replaced.
struct Fake {
    store: VaultAccessStore,
    caller: &'static str,
    access: Option<VaultAccess>,
    mounted: bool,
    header: Result<&'static str, &'static str>,
    existing: Vec<String>,
    access_denied: bool,
    renames: RefCell<Vec<(String, String)>>,
    probed: Cell<bool>,
}

impl Fake {
    fn new(store: VaultAccessStore) -> Self {
        Self {
            store,
            caller: OWNER,
            access: None,
            mounted: false,
            header: Ok("outer"),
            existing: Vec::new(),
            access_denied: false,
            renames: RefCell::new(Vec::new()),
            probed: Cell::new(false),
        }
    }

    fn identity(&self, path: &str) -> String {
        self.store.password_change_target(path, OWNER, 7).unwrap().container_identity
    }

    fn run(&self, path: &str, new_name: &str) -> Result<serde_json::Value, VerbError> {
        let broker = crate::vault_mount::test_broker_with_mount(self.mounted.then(|| self.identity(path)).as_deref());
        let request = validate_request(&request(path, new_name)).expect("valid request");
        let resolve = || {
            self.store
                .password_change_target(&request.container_path, self.caller, 7)
                .map_err(TargetLookupError::Vault)
        };
        let grant = |_: &str| (self.access.is_some(), self.access);
        let prove_access = |_: &PasswordChangeTarget| match self.access_denied {
            true => Err(RenameFault::File(FileFault::AccessDenied)),
            false => Ok(()),
        };
        let exists = |path: &str| Ok(self.existing.iter().any(|existing| existing.eq_ignore_ascii_case(path)));
        let rename = |from: &str, _: &str, to: &str| {
            self.renames.borrow_mut().push((from.to_owned(), to.to_owned()));
            Ok(())
        };
        let steps = RenameSteps { resolve: &resolve, grant: &grant, prove_access: &prove_access, exists: &exists, rename: &rename };
        let log = Refusals::new(1, "rename");
        rename_for_caller(&log, &self.store, &broker, &request, &steps, |_| {
            self.probed.set(true);
            self.header.map_err(|code| VerbError::new(code, "probe refused"))
        })
    }

    fn refused(&self, path: &str, new_name: &str) -> &'static str {
        self.run(path, new_name).expect_err("the rename must be refused").kind
    }
}

fn target(path: &str, identity: &str) -> PasswordChangeTarget {
    PasswordChangeTarget { container_path: path.into(), container_identity: identity.into(), entry_id: None, caller_owns_entry: false }
}

fn store_with_personal_vault() -> VaultAccessStore {
    let store = crate::vault_access::test_policy_store();
    let identity = store.password_change_target(PERSONAL_PATH, OWNER, 7).unwrap().container_identity;
    store.insert_personal_record_for_test(PersonalVaultRecord {
        container_path: PERSONAL_PATH.into(),
        container_identity: identity,
        owner_sid: OWNER.into(),
        scope: VaultPresentation::PerUser,
        created_by_session: 7,
        syncthing_opt_in: Some(false),
    });
    store
}

fn store_with_policy_vault() -> VaultAccessStore {
    let store = crate::vault_access::test_policy_store();
    let grants = ["Admin", "Partner"].map(|name| VaultGrantInput { principal_name: name.into(), access: VaultAccess::Write });
    let entry = VaultAccessEntry {
        id: "shared".into(),
        label: "SyncTest".into(),
        container_path: POLICY_PATH.into(),
        container_identity: None,
        container_kind: VaultContainerKind::Standard,
        primary_owner_sid: None,
        owner_account: "Admin".into(),
        access_pattern: None,
        syncthing_opt_in: None,
        cloud_backup: None,
        grants: grants.into(),
        mount: VaultMountPolicy { presentation: VaultPresentation::Machine, preferred_letter: Some("V".into()) },
    };
    let policy = VaultAccessPolicy { schema_version: 1, policy_id: "p".into(), version: 1, expected_previous_version: 0, entries: vec![entry] };
    store.apply(policy, 7).unwrap();
    store
}

#[test]
fn a_dismounted_vault_is_renamed_after_its_password_opens_it() {
    let fake = Fake::new(crate::vault_access::test_policy_store());
    let reply = fake.run(PERSONAL_PATH, "Taxes").unwrap();
    assert_eq!(reply, json!({ "status": "renamed", "path": r"D:\Vaults\Taxes.hc", "name": "Taxes.hc" }));
    assert_eq!(*fake.renames.borrow(), [(PERSONAL_PATH.to_owned(), r"D:\Vaults\Taxes.hc".to_owned())]);
    assert!(fake.probed.get());
}

#[test]
fn a_registered_personal_vault_record_moves_to_the_new_path() {
    let fake = Fake::new(store_with_personal_vault());
    fake.run(PERSONAL_PATH, "Taxes 2026").unwrap();
    use crate::vault_access::PersonalVaultRegistrationState::{IdentityChanged, Unregistered};
    assert_eq!(fake.store.personal_registration_state(PERSONAL_PATH, OWNER), Ok(Unregistered));
    // This stand-in filesystem derives identity from the path, so the moved record reads as changed.
    assert_eq!(fake.store.personal_registration_state(r"D:\Vaults\Taxes 2026.hc", OWNER), Ok(IdentityChanged));
}

#[test]
fn another_accounts_personal_vault_is_refused_before_its_password_is_tried() {
    let mut fake = Fake::new(store_with_personal_vault());
    fake.caller = OTHER;
    assert_eq!(fake.refused(PERSONAL_PATH, "Mine"), PERSONAL_VAULT_UNAUTHORIZED);
    assert!(!fake.probed.get() && fake.renames.borrow().is_empty());
}

#[test]
fn a_wrong_password_renames_nothing() {
    let mut fake = Fake::new(store_with_personal_vault());
    fake.header = Err("wrong_password");
    assert_eq!(fake.refused(PERSONAL_PATH, "Taxes"), "wrong_password");
    assert!(fake.renames.borrow().is_empty());
    assert!(fake.store.personal_for_owner(PERSONAL_PATH, OWNER).unwrap().is_some());
}

#[test]
fn a_noted_vault_needs_its_real_password_and_its_note_follows_the_file() {
    let fake = Fake::new(crate::vault_access::test_policy_store());
    let identity = fake.identity(PERSONAL_PATH);
    fake.store.note_hidden_part(&target(PERSONAL_PATH, &identity)).unwrap();
    assert_eq!(fake.refused(PERSONAL_PATH, "Taxes"), "real_password_required");
    assert!(fake.renames.borrow().is_empty());

    let mut fake = fake;
    fake.header = Ok("hidden");
    fake.run(PERSONAL_PATH, "Taxes").unwrap();
    assert_eq!(fake.store.has_hidden_part(&identity), Ok(true));
}

#[test]
fn a_hidden_answer_notes_the_vault() {
    let mut fake = Fake::new(crate::vault_access::test_policy_store());
    fake.header = Ok("hidden");
    let identity = fake.identity(PERSONAL_PATH);
    fake.run(PERSONAL_PATH, "Taxes").unwrap();
    assert_eq!(fake.store.has_hidden_part(&identity), Ok(true));
}

#[test]
fn a_mounted_vault_is_refused_before_its_password_is_tried() {
    let mut fake = Fake::new(store_with_personal_vault());
    fake.mounted = true;
    assert_eq!(fake.refused(PERSONAL_PATH, "Taxes"), "volume_mounted");
    assert!(!fake.probed.get() && fake.renames.borrow().is_empty());
}

#[test]
fn a_bad_taken_or_unchanged_name_is_refused_before_the_password_is_tried() {
    let mut fake = Fake::new(store_with_personal_vault());
    fake.existing = vec![r"D:\Vaults\TAKEN.hc".into()];
    for (name, code) in [
        ("a/b", "name_invalid"),
        ("CON", "name_invalid"),
        ("trailing.", "name_invalid"),
        ("Notes", "name_unchanged"),
        ("notes.HC", "name_unchanged"),
        ("taken", "name_exists"),
    ] {
        assert_eq!(fake.refused(PERSONAL_PATH, name), code, "{name}");
    }
    assert!(!fake.probed.get() && fake.renames.borrow().is_empty());
}

#[test]
fn a_caller_who_cannot_write_the_file_or_folder_is_refused() {
    let mut fake = Fake::new(crate::vault_access::test_policy_store());
    fake.access_denied = true;
    assert_eq!(fake.refused(PERSONAL_PATH, "Taxes"), "vault_access_denied");
    assert!(!fake.probed.get());
}

#[test]
fn a_policy_vault_is_renamed_only_by_a_writer_and_its_entry_moves() {
    let mut fake = Fake::new(store_with_policy_vault());
    fake.caller = OTHER;
    assert_eq!(fake.refused(POLICY_PATH, "Read"), PERSONAL_VAULT_UNAUTHORIZED);
    fake.access = Some(VaultAccess::Read);
    assert_eq!(fake.refused(POLICY_PATH, "Read"), "vault_policy_managed");
    assert!(!fake.probed.get());
    assert_eq!(fake.store.policy().unwrap().version, 1);

    fake.access = Some(VaultAccess::Write);
    fake.run(POLICY_PATH, "SyncTest").unwrap();
    let policy = fake.store.policy().unwrap();
    assert_eq!(policy.entries[0].container_path, r"D:\Vaults\SyncTest.hc");
    assert_eq!(policy.entries[0].label, "SyncTest", "the label is not the file name");
    assert_eq!(policy.version, 2);
}

#[test]
fn a_record_that_cannot_be_saved_renames_the_file_back() {
    let files = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::from([(
        std::path::PathBuf::from("/policy/vault-hidden-parts-v1.json"),
        b"not json".to_vec(),
    )])));
    let store = crate::vault_access::test_policy_store_over(files);
    store.load_at_startup();
    let mut fake = Fake::new(store);
    fake.header = Ok("hidden");
    assert_eq!(fake.refused(PERSONAL_PATH, "Taxes"), "vault_rename_not_saved");
    let renamed = r"D:\Vaults\Taxes.hc".to_owned();
    assert_eq!(*fake.renames.borrow(), [(PERSONAL_PATH.to_owned(), renamed.clone()), (renamed, PERSONAL_PATH.to_owned())]);
}

#[test]
fn requests_need_a_file_path_a_name_a_credential_and_no_other_fields() {
    let parsed = validate_request(&request(&format!("  \"{PERSONAL_PATH}\" "), " Taxes ")).unwrap();
    assert_eq!((parsed.container_path.as_str(), parsed.new_name.as_str()), (PERSONAL_PATH, " Taxes "));
    assert!(!format!("{parsed:?}").contains("canary"));
    let keyfiles_only = json!({ "container_path": PERSONAL_PATH, "new_name": "x", "password": "", "keyfiles": [r"D:\k.key"], "pim": "600" });
    assert_eq!(validate_request(&keyfiles_only).unwrap().keyfiles, [r"D:\k.key"]);
    for (key, value) in [
        ("container_path", json!("notes.hc")),
        ("container_path", json!(r"\Device\Harddisk1\Partition1")),
        ("new_name", json!(7)),
        ("password", json!("")),
        ("pim", json!("0")),
        ("keyfiles", json!(["relative.key"])),
        ("name", json!("Taxes")),
    ] {
        let mut args = request(PERSONAL_PATH, "Taxes");
        args[key] = value;
        assert!(validate_request(&args).is_none(), "{key} must be rejected");
    }
    let mut missing = request(PERSONAL_PATH, "Taxes");
    missing.as_object_mut().unwrap().remove("password");
    assert!(validate_request(&missing).is_none());
}

#[test]
fn the_password_leaves_the_request_only_by_being_moved_into_the_probe() {
    let mut args = json!({ "container_path": PERSONAL_PATH, "new_name": "x", "password": "pw-canary", "pim": 600 });
    let probe = probe_args(&mut args);
    assert_eq!(probe.0["Password"], "pw-canary");
    assert_eq!(probe.0["Pim"], 600);
    assert!(!args.to_string().contains("canary"));
}

#[test]
fn a_verbatim_path_reads_as_an_ordinary_one() {
    assert_eq!(display_path(r"\\?\D:\Vault\a.hc"), r"D:\Vault\a.hc");
    assert_eq!(display_path(r"\\?\UNC\server\share\a.hc"), r"\\server\share\a.hc");
    assert_eq!(display_path(r"D:\Vault\a.hc"), r"D:\Vault\a.hc");
}

#[test]
fn handlers_refuse_without_an_authenticated_caller() {
    let store = crate::vault_access::test_policy_store();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    assert_eq!(handle(1, &store, &broker, request(PERSONAL_PATH, "Taxes"), None).unwrap_err().kind, PERSONAL_VAULT_SESSION_ABSENT);
    assert_eq!(handle_display_names(&store, None).unwrap_err().kind, PERSONAL_VAULT_SESSION_ABSENT);
}

/// Real files on this PC, renamed in this test's own token; only the broker's probe is replaced.
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
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("wc-rename-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(dir.join("New folder")).unwrap();
        TempDir(dir)
    }

    struct ProcessToken(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for ProcessToken {
        fn drop(&mut self) {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }

    /// This test process's own token, standing in for the pipe caller's.
    fn own_token() -> ProcessToken {
        use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = std::ptr::null_mut();
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE, &mut token) };
        assert_ne!(opened, 0);
        ProcessToken(token)
    }

    fn rename(store: &VaultAccessStore, path: &str, new_name: &str, header: &'static str) -> Result<serde_json::Value, VerbError> {
        let token = own_token();
        let token = token.0;
        let broker = crate::vault_mount::test_broker_with_mount(None);
        let request = validate_request(&super::request(path, new_name)).expect("valid request");
        let resolve = || store.password_change_target_as_caller(&request.container_path, OWNER, 7, token);
        let grant = |_: &str| (false, None);
        let prove_access = |target: &PasswordChangeTarget| crate::vault_access::prove_rename_access_as_caller(target, token);
        let exists = |path: &str| crate::vault_access::target_exists_as_caller(path, token);
        let rename = |from: &str, identity: &str, to: &str| crate::vault_access::rename_container_as_caller(from, identity, to, token);
        let steps = RenameSteps { resolve: &resolve, grant: &grant, prove_access: &prove_access, exists: &exists, rename: &rename };
        let log = Refusals::new(1, "rename");
        rename_for_caller(&log, store, &broker, &request, &steps, |_| Ok(header))
    }

    #[test]
    fn an_extensionless_container_in_a_folder_with_a_space_is_renamed_on_disk() {
        let dir = temp_dir("plain");
        let file = dir.0.join("New folder").join("test3");
        std::fs::write(&file, vec![0x5a; 1 << 20]).unwrap();
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        let reply = rename(&store, &format!("\"{}\"", file.display()), "Test three", "hidden").unwrap();
        let renamed = dir.0.join("New folder").join("Test three");
        assert_eq!(reply, json!({ "status": "renamed", "path": renamed.to_str().unwrap(), "name": "Test three" }));
        assert!(!file.exists());
        assert_eq!(std::fs::read(&renamed).unwrap(), vec![0x5a; 1 << 20], "the container is never written");
        let restarted = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        restarted.load_at_startup();
        let identity = restarted.password_change_target(renamed.to_str().unwrap(), OWNER, 7).unwrap().container_identity;
        assert_eq!(restarted.has_hidden_part(&identity), Ok(true), "the note follows the file");
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let dir = temp_dir("exists");
        let file = dir.0.join("New folder").join("notes.hc");
        let taken = dir.0.join("New folder").join("Taxes.hc");
        std::fs::write(&file, b"vault").unwrap();
        std::fs::write(&taken, b"other").unwrap();
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        assert_eq!(rename(&store, file.to_str().unwrap(), "Taxes", "outer").unwrap_err().kind, "name_exists");
        assert_eq!((std::fs::read(&file).unwrap(), std::fs::read(&taken).unwrap()), (b"vault".to_vec(), b"other".to_vec()));
    }

    #[test]
    fn a_read_only_container_is_refused_and_a_missing_one_is_not_found() {
        let dir = temp_dir("readonly");
        let file = dir.0.join("New folder").join("test3");
        std::fs::write(&file, vec![1; 4096]).unwrap();
        let writable = std::fs::metadata(&file).unwrap().permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(&file, read_only).unwrap();
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        assert_eq!(rename(&store, file.to_str().unwrap(), "Nope", "outer").unwrap_err().kind, "vault_access_denied");
        assert!(file.exists());
        std::fs::set_permissions(&file, writable).unwrap();
        let missing = dir.0.join("New folder").join("absent");
        assert_eq!(rename(&store, missing.to_str().unwrap(), "Ghost", "outer").unwrap_err().kind, "vault_file_not_found");
    }

    #[test]
    fn a_program_writing_the_file_keeps_it_in_use() {
        let dir = temp_dir("in-use");
        let file = dir.0.join("New folder").join("notes.hc");
        std::fs::write(&file, b"vault").unwrap();
        let store = crate::vault_access::test_windows_fs_store(dir.0.join("policy"));
        let writer = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
        assert_eq!(rename(&store, file.to_str().unwrap(), "Taxes", "outer").unwrap_err().kind, "vault_file_in_use");
        drop(writer);
        assert!(file.exists());
    }
}
