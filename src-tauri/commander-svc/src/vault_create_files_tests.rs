// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wc-create-files-test-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for entry in std::fs::read_dir(&self.0).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_dir() {
                std::fs::remove_dir(entry.path()).unwrap();
            } else {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}

#[test]
fn existing_target_is_never_overwritten() {
    let fixture = Fixture::new();
    let target = fixture.0.join("existing.bin");
    std::fs::write(&target, b"original user data").unwrap();
    assert!(Destination::create(&target).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"original user data");
}

#[test]
fn failure_deletes_only_the_reserved_new_file() {
    let fixture = Fixture::new();
    let target = fixture.0.join("new.bin");
    let neighbor = fixture.0.join("neighbor.bin");
    std::fs::write(&neighbor, b"preserve").unwrap();
    drop(Destination::create(&target).unwrap());
    assert!(!target.exists());
    assert_eq!(std::fs::read(&neighbor).unwrap(), b"preserve");
}

#[test]
fn held_destination_blocks_path_replacement_and_commit_preserves_it() {
    let fixture = Fixture::new();
    let target = fixture.0.join("new.bin");
    let mut reserved = Destination::create(&target).unwrap();
    assert!(std::fs::remove_file(&target).is_err());
    assert!(std::fs::rename(&target, fixture.0.join("replaced.bin")).is_err());
    reserved.commit();
    drop(reserved);
    assert!(target.is_file());
}

#[test]
fn missing_caller_cannot_create_any_destination() {
    let fixture = Fixture::new();
    let target = fixture.0.join("denied.bin");
    let result = crate::vault_access::with_caller_impersonation(std::ptr::null_mut(), || {
        Destination::create(&target)
    });
    assert!(result.is_err());
    assert!(!target.exists());
}

#[test]
fn directory_source_is_not_accepted_as_a_file() {
    let fixture = Fixture::new();
    assert!(read_file(&fixture.0).is_err());
}

#[test]
fn held_destination_allows_existing_owner_acl_and_identity_completion() {
    use crate::vault_access::{
        AclApplier, PrincipalResolver, ResolvedGrant, VaultAclPlan, WindowsAclApplier,
        WindowsPrincipalResolver,
    };
    use wincmd_shared::vault_access::VaultAccess;
    let fixture = Fixture::new();
    let target = fixture.0.join("new.bin");
    let mut reserved = Destination::create(&target).unwrap();
    let sid = WindowsPrincipalResolver
        .resolve_sid(&std::env::var("USERNAME").unwrap())
        .unwrap();
    let plan = VaultAclPlan {
        parent: fixture.0.clone(),
        container: target.clone(),
        grants: vec![ResolvedGrant {
            sid,
            access: VaultAccess::Write,
        }],
        authorization_grants: Vec::new(),
        managed_groups: Vec::new(),
    };
    let original = WindowsAclApplier.snapshot(&plan).unwrap();
    WindowsAclApplier.apply_and_verify(&plan).unwrap();
    assert!(std::fs::File::open(&target).is_ok());
    WindowsAclApplier.restore(&original).unwrap();
    reserved.commit();
}
