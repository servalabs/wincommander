use super::*;

fn target(entry_id: Option<&str>) -> PasswordChangeTarget {
    PasswordChangeTarget {
        container_path: r"D:\Vault\New folder\test3".into(),
        container_identity: "v:9:i:3".into(),
        entry_id: entry_id.map(str::to_owned),
        caller_owns_entry: false,
    }
}

#[test]
fn a_file_the_caller_cannot_reach_is_never_reported_as_a_validation_failure() {
    for (fault, code) in [
        (FileFault::NotFound, "vault_file_not_found"),
        (FileFault::AccessDenied, "vault_access_denied"),
        (FileFault::InUse, "vault_file_in_use"),
    ] {
        assert_eq!(target_refusal(TargetLookupError::File(fault)).1.kind, code);
    }
    for (error, code) in [
        (VaultError::Validation, "vault_validation_failed"),
        (VaultError::Forbidden, PERSONAL_VAULT_UNAUTHORIZED),
        (VaultError::ContainerIdentity, "vault_policy_identity_changed"),
        (VaultError::AclReadback, "vault_policy_unavailable"),
    ] {
        assert_eq!(target_refusal(TargetLookupError::Vault(error)).1.kind, code);
    }
}

#[test]
fn a_vault_permissions_vault_is_changed_only_by_its_owner_or_a_writer() {
    use wincmd_shared::vault_access::VaultAccess;
    assert!(policy_refusal(&target(None), |_| unreachable!("a personal Vault has no grant")).is_ok());
    let managed = target(Some("shared"));
    assert!(policy_refusal(&managed, |id| (id == "shared", Some(VaultAccess::Write))).is_ok());
    assert_eq!(policy_refusal(&managed, |_| (true, Some(VaultAccess::Read))).unwrap_err().1.kind, "vault_policy_managed");
    // An administrator outside the share has no grant and is refused like anyone else.
    assert_eq!(policy_refusal(&managed, |_| (false, None)).unwrap_err().1.kind, PERSONAL_VAULT_UNAUTHORIZED);
    let owned = PasswordChangeTarget { caller_owns_entry: true, ..target(Some("shared")) };
    assert!(policy_refusal(&owned, |_| unreachable!("the owner needs no grant")).is_ok());
}

#[test]
fn a_mounted_container_is_refused_whichever_header_was_mounted() {
    let selected = target(None);
    // The registry records the container, not the header: an outer and a hidden mount look the same here.
    let mounted = crate::vault_mount::test_broker_with_mount(Some(&selected.container_identity));
    let reason = mount_refusal(&mounted, &selected).unwrap_err();
    assert_eq!(failure_code(reason), "volume_mounted");
    let other = crate::vault_mount::test_broker_with_mount(Some("v:9:i:4"));
    assert_eq!(mount_refusal(&other, &selected), Ok(()));
    let idle = crate::vault_mount::test_broker_with_mount(None);
    assert_eq!(mount_refusal(&idle, &selected), Ok(()));
}
