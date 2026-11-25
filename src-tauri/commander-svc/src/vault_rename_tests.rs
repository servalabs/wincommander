use super::*;
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
        .find(|(path, _)| path == PERSONAL_PATH)
        .map(|(_, name)| name)
}

#[test]
fn the_owner_renames_a_registered_personal_vault() {
    let (store, _) = store_with_personal_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let reply = rename_for_caller(&store, &broker, OWNER, 7, &request(PERSONAL_PATH, "  Taxes "), &no_grant).unwrap();
    assert_eq!(reply, json!({ "status": "renamed", "name": "Taxes" }));
    assert_eq!(personal_name(&store).as_deref(), Some("Taxes"));
}

#[test]
fn another_account_cannot_rename_a_personal_vault() {
    let (store, _) = store_with_personal_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let error = rename_for_caller(&store, &broker, OTHER, 8, &request(PERSONAL_PATH, "Mine"), &no_grant).unwrap_err();
    assert_eq!(error.kind, PERSONAL_VAULT_UNAUTHORIZED);
    assert_eq!(personal_name(&store), None);
}

#[test]
fn a_write_grant_renames_a_policy_vault_and_a_read_grant_does_not() {
    let store = store_with_policy_vault();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    let rename = |access: Option<VaultAccess>, name: &str| {
        rename_for_caller(&store, &broker, OTHER, 8, &request(POLICY_PATH, name), &move |_| (access.is_some(), access))
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
    let error = rename_for_caller(&store, &broker, OWNER, 7, &request(PERSONAL_PATH, "Taxes"), &no_grant).unwrap_err();
    assert_eq!(error.kind, "volume_mounted");
    assert_eq!(personal_name(&store), None);

    let policy = store_with_policy_vault();
    let policy_identity = policy.password_change_target(POLICY_PATH, OWNER, 7).unwrap().container_identity;
    let broker = crate::vault_mount::test_broker_with_mount(Some(&policy_identity));
    let error = rename_for_caller(&policy, &broker, OTHER, 8, &request(POLICY_PATH, "Finance"), &|_| {
        (true, Some(VaultAccess::Write))
    })
    .unwrap_err();
    assert_eq!(error.kind, "volume_mounted");
    assert_eq!(policy.policy().unwrap().entries[0].label, "Shared");
}

#[test]
fn an_unregistered_container_is_refused_without_being_registered() {
    let store = crate::vault_access::test_policy_store();
    let broker = crate::vault_mount::test_broker_with_mount(None);
    for path in [PERSONAL_PATH, r"\Device\Harddisk1\Partition1"] {
        let error = rename_for_caller(&store, &broker, OWNER, 7, &request(path, "Taxes"), &no_grant).unwrap_err();
        assert_eq!(error.kind, VAULT_NOT_REGISTERED);
    }
    assert_eq!(store.rename_target(PERSONAL_PATH, OWNER, 7), Ok(None));
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
