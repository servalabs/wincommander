use super::*;
use std::sync::{Arc, Barrier};
use std::thread;

fn test_plan(self_destruct: crate::settings::SelfDestructSettings) -> LockdownPlanSnapshot {
    LockdownPlanSnapshot {
        self_destruct,
        shred_mft_slack: false,
    }
}

#[test]
fn consume_rejects_wrong_action() {
    let tok = mint(DestructiveAction::SelfDestruct, "hash-A");
    assert!(consume(&tok, DestructiveAction::RemoveUsers, "hash-A").is_err());
}

#[test]
fn consume_rejects_wrong_args() {
    let tok = mint(DestructiveAction::DiskDelete, "path=/a");
    assert!(consume(&tok, DestructiveAction::DiskDelete, "path=/b").is_err());
}

#[test]
fn token_is_single_use() {
    let tok = mint(DestructiveAction::KillSwitch, "on");
    assert!(consume(&tok, DestructiveAction::KillSwitch, "on").is_ok());
    assert!(consume(&tok, DestructiveAction::KillSwitch, "on").is_err());
}

#[test]
fn unknown_token_refused() {
    assert!(consume("not-a-real-token", DestructiveAction::SelfDestruct, "x").is_err());
}

#[test]
fn missing_or_empty_token_is_refused() {
    assert!(consume_required(None, DestructiveAction::DiskDelete, "x").is_err());
    assert!(consume_required(Some(""), DestructiveAction::DiskDelete, "x").is_err());
}

#[test]
fn capability_is_bound_to_command_and_arguments() {
    let plan = test_plan(crate::settings::SelfDestructSettings::default());
    let args = lockdown_args(false, false, &plan);
    let tok = mint(DestructiveAction::SelfDestruct, &args);
    assert!(consume_required(
        Some(&tok),
        DestructiveAction::SelfDestruct,
        &full_lockdown_args(&plan),
    )
    .is_err());

    let args = disk_delete_args("C:\\one");
    let tok = mint(DestructiveAction::DiskDelete, &args);
    assert!(consume_required(
        Some(&tok),
        DestructiveAction::DiskDelete,
        &disk_delete_args("C:\\two"),
    )
    .is_err());
}

#[test]
fn lockdown_capability_is_bound_to_the_complete_configured_plan() {
    let mut confirmed = test_plan(crate::settings::SelfDestructSettings {
        enabled: Some(true),
        shred_folders: Some(vec!["C:\\confirmed".to_string()]),
        ..Default::default()
    });
    let args = full_lockdown_args(&confirmed);
    let token = mint(DestructiveAction::SelfDestruct, &args);
    confirmed.self_destruct.shred_folders = Some(vec!["C:\\substituted".to_string()]);
    assert!(consume_required(
        Some(&token),
        DestructiveAction::SelfDestruct,
        &full_lockdown_args(&confirmed),
    )
    .is_err());
}

#[test]
fn lockdown_plan_canonicalization_is_independent_of_map_insertion_order() {
    let mut first_steps = HashMap::new();
    first_steps.insert("configured_folders".to_string(), true);
    first_steps.insert("remove_users".to_string(), false);
    let mut second_steps = HashMap::new();
    second_steps.insert("remove_users".to_string(), false);
    second_steps.insert("configured_folders".to_string(), true);
    let first = test_plan(crate::settings::SelfDestructSettings {
        steps: Some(first_steps),
        ..Default::default()
    });
    let second = test_plan(crate::settings::SelfDestructSettings {
        steps: Some(second_steps),
        ..Default::default()
    });
    assert_eq!(full_lockdown_args(&first), full_lockdown_args(&second));
}

#[test]
fn lockdown_confirmation_discloses_mft_resident_slack_wipe() {
    let mut plan = test_plan(crate::settings::SelfDestructSettings::default());
    plan.shred_mft_slack = true;
    let detail = DestructiveRequest::FullLockdown.confirmation_detail(Some(&plan));

    assert!(detail.contains("MFT resident-slack wipe: yes"));
}

#[cfg(windows)]
#[test]
fn path_capability_detects_file_identity_replacement() {
    use std::io::Write;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.bin");
    let retained = directory.path().join("retained.bin");
    std::fs::File::create(&target)
        .unwrap()
        .write_all(b"first")
        .unwrap();
    let confirmed = disk_delete_args(&target.to_string_lossy());
    std::fs::rename(&target, &retained).unwrap();
    std::fs::File::create(&target)
        .unwrap()
        .write_all(b"replacement")
        .unwrap();
    let replaced = disk_delete_args(&target.to_string_lossy());
    assert_ne!(confirmed, replaced);
}

#[test]
fn required_capability_is_single_use() {
    let args = kill_switch_args(true);
    let tok = mint(DestructiveAction::KillSwitch, &args);
    assert!(consume_required(Some(&tok), DestructiveAction::KillSwitch, &args).is_ok());
    assert!(consume_required(Some(&tok), DestructiveAction::KillSwitch, &args).is_err());
}

#[test]
fn expired_capability_is_refused_and_removed() {
    let token = "expired-test-token".to_string();
    STORE.lock().unwrap().insert(
        token.clone(),
        Minted {
            action: DestructiveAction::DiskDelete,
            args_hash: hash_args("target"),
            at: Instant::now() - TTL,
        },
    );
    assert!(consume(&token, DestructiveAction::DiskDelete, "target").is_err());
    assert!(!STORE.lock().unwrap().contains_key(&token));
}

#[test]
fn concurrent_replay_has_exactly_one_winner() {
    let token = mint(DestructiveAction::DiskDelete, "target");
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let token = token.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                consume(&token, DestructiveAction::DiskDelete, "target").is_ok()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|won| *won)
            .count(),
        1
    );
}

#[test]
fn trusted_countdown_gate_is_single_fire_and_cancel_safe() {
    let gate = TrustedLockdownGate::new();
    let first = gate.arm().unwrap();
    assert!(gate.arm().is_none());
    assert!(gate.cancel());
    assert!(!gate.take_if_current(first));
    let second = gate.arm().unwrap();
    assert!(!gate.take_if_current(first));
    assert!(gate.take_if_current(second));
    assert!(!gate.take_if_current(second));
}

#[test]
fn typed_request_owns_action_arguments_and_safe_details() {
    let request = DestructiveRequest::DiskDeleteItem {
        path: "C:\\target\nspoof".to_string(),
    };
    assert_eq!(request.action(), DestructiveAction::DiskDelete);
    assert_eq!(
        request.canonical_args(None),
        disk_delete_args("C:\\target\nspoof")
    );
    let detail = request.confirmation_detail(None);
    assert!(detail.contains("C:\\target�spoof"));
    assert!(!detail.contains("target\nspoof"));
}

#[test]
fn backend_dispatch_keeps_read_only_commands_available() {
    let mut params = HashMap::new();
    assert_eq!(
        backend_dispatch_policy("Get-DnsCacheEntries"),
        BackendDispatchPolicy::Ordinary
    );
    assert!(authorize_backend_dispatch("Get-DnsCacheEntries", &mut params).is_ok());
}

#[test]
fn backend_dispatch_refuses_lockdown_only_feature_ids() {
    for command in [
        "run_destruct_step",
        "Remove-LocalUsers",
        "Destroy-VeraCryptHeader",
        "Clear-BitLockerKeyProtectors",
        "Invoke-7Wipe",
        "Clear-MFTResidentSlack",
        "not-a-registered-command",
    ] {
        let mut params = HashMap::new();
        assert!(
            authorize_backend_dispatch(command, &mut params).is_err(),
            "allowed {command}"
        );
    }
    for command in [
        "Invoke-CrashDumpErase",
        "Invoke-PreviousWindowsInstallErase",
    ] {
        assert_eq!(
            backend_dispatch_policy(command),
            BackendDispatchPolicy::NativeConfirmation,
            "did not require native confirmation for {command}"
        );
    }
}

#[test]
fn backend_secure_erase_requires_matching_single_use_capability() {
    let path = "C:\\Users\\test\\target.bin";
    let mut missing = HashMap::from([("Path".to_string(), path.to_string())]);
    assert!(authorize_backend_dispatch("Invoke-7Erase", &mut missing).is_err());

    let token = mint(DestructiveAction::DiskDelete, &secure_erase_args(path));
    let mut authorized = HashMap::from([
        ("Path".to_string(), path.to_string()),
        ("CapabilityToken".to_string(), token.clone()),
    ]);
    assert!(authorize_backend_dispatch("Invoke-7Erase", &mut authorized).is_ok());
    assert!(!authorized.contains_key("CapabilityToken"));

    let mut replay = HashMap::from([
        ("Path".to_string(), path.to_string()),
        ("CapabilityToken".to_string(), token),
    ]);
    assert!(authorize_backend_dispatch("Invoke-7Erase", &mut replay).is_err());

    for (key, value) in [
        ("Name", "DifferentRegistryValue"),
        ("Passes", "99"),
        ("Type", "RegistryProperty"),
    ] {
        let token = mint(DestructiveAction::DiskDelete, &secure_erase_args(path));
        let mut changed = HashMap::from([
            ("Path".to_string(), path.to_string()),
            ("CapabilityToken".to_string(), token),
            (key.to_string(), value.to_string()),
        ]);
        assert!(
            authorize_backend_dispatch("Invoke-7Erase", &mut changed).is_err(),
            "allowed attacker-controlled {key}"
        );
    }
}

#[test]
fn backend_free_space_capability_is_bound_to_drive_and_media() {
    let token = mint(
        DestructiveAction::CryptoErase,
        &free_space_erase_args("D", "SSD"),
    );
    let mut changed_target = HashMap::from([
        ("DriveLetter".to_string(), "E".to_string()),
        ("MediaType".to_string(), "SSD".to_string()),
        ("CapabilityToken".to_string(), token),
    ]);
    assert!(
        authorize_backend_dispatch("Invoke-UnallocatedSpaceErase", &mut changed_target).is_err()
    );
}

#[test]
fn registry_covers_known_catastrophic_commands() {
    for name in [
        "lockdown",
        "full_lockdown",
        "run_destruct_step",
        "fleet_connect",
        "erase_encrypted_container",
    ] {
        assert!(
            action_for(name).is_some(),
            "missing registry entry for {name}"
        );
    }
}
