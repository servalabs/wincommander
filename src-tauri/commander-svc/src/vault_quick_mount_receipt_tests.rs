// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn quick_mount_explorer_receipt_requires_exact_owner_logon_and_live_native_instance() {
    let mut mount = ActiveMount {
        drive_letter: "V:".into(), internal_drive: 12, presentation: VaultPresentation::Machine,
        session_id: 7, authentication_id: Some((42, 0)), caller_sid: "S-1-5-21-101".into(),
        policy_id: "personal".into(), policy_version: 0, personal: true,
        syncthing_managed: false, syncthing_resume_unresolved: false,
        container_identity: "v:1:i:2".into(), access: wincmd_shared::vault_access::VaultAccess::Write,
        mounted_at: 1, cleanup_required: false, driver_slot_absent: false,
        engine_mount_identity: Some("native:original".into()),
        canonical_container_path: Some(r"D:\Disposable\vault.hc".into()),
    };
    let slots = HashMap::from([(12, "native:original".into())]);
    let receipt = |mount: &ActiveMount, session, sid, logon, slots: &HashMap<u8, String>|
        quick_mount_receipt_for_caller("personal", mount, session, sid, logon, slots);
    let original = receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &slots).unwrap();
    for (session, sid, logon) in [
        (8, "S-1-5-21-101", Some((42, 0))),
        (7, "S-1-5-21-102", Some((42, 0))),
        (7, "S-1-5-21-101", Some((43, 0))),
        (7, "S-1-5-21-101", None),
    ] {
        assert!(receipt(&mount, session, sid, logon, &slots).is_none());
    }
    assert!(receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &HashMap::new()).is_none());
    let reused = HashMap::from([(12, "native:replacement".into())]);
    assert!(receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &reused).is_none());
    mount.engine_mount_identity = Some("native:replacement".into());
    assert_ne!(receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &reused).unwrap(), original);
    mount.cleanup_required = true;
    assert!(receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &reused).is_none());
    mount.cleanup_required = false;
    mount.driver_slot_absent = true;
    assert!(receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &reused).is_none());
    mount.driver_slot_absent = false;
    mount.authentication_id = None;
    assert!(receipt(&mount, 7, "S-1-5-21-101", Some((42, 0)), &reused).is_none());
}
