// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws, VerifyError};

#[test]
fn record_reads_are_bounded_and_missing_is_explicit() {
    let path = std::env::temp_dir().join(format!("free-floor-test-{}", uuid::Uuid::new_v4()));
    assert!(read_record(&path).unwrap().is_none());
    std::fs::write(&path, b"record").unwrap();
    assert_eq!(read_record(&path).unwrap().as_deref(), Some(&b"record"[..]));
    std::fs::write(&path, vec![b'x'; LIMIT + 1]).unwrap();
    assert!(read_record(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn signed_record_rejects_replay_and_allows_retry_and_channel_promotion() {
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let trust = Trust {
        ed: ed.verifying_key().to_bytes(),
        pq: pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "ed",
        pq_id: "pq",
        epoch: 1,
    };
    let make = |version: &str, channel: &str, epoch: u64, hash: &str| {
        let url = format!(
            "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
        );
        let payload = serde_json::to_vec(
            &serde_json::json!({"format":"wincommander-free-release-v1", "version":version,
            "channel":channel, "platform":"windows-x86_64", "url":url, "size":2, "sha256":hash}),
        )
        .unwrap();
        let envelope = jws::sign::assemble(
            &payload,
            epoch,
            &trust.policy(),
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| {
                let mut sig = vec![0; 3309];
                pq.sign(message, &mut sig)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(sig)
            },
        )
        .unwrap();
        Record {
            version: version.into(),
            channel: channel.into(),
            envelope: String::from_utf8(envelope).unwrap(),
        }
    };
    let hash = "a".repeat(64);
    let old = make("3.7.0", "beta", 7, &hash);
    let stored = serde_json::to_vec(&old).unwrap();
    assert!(check(None, &old, &trust).is_ok());
    assert!(check(Some(&stored), &old, &trust).is_ok()); // failed installation can retry
    assert!(check(Some(&stored), &make("3.7.0", "stable", 7, &hash), &trust).is_ok());
    assert!(check(Some(&stored), &make("3.8.0", "stable", 8, &hash), &trust).is_ok());
    assert!(check(Some(&stored), &make("3.6.9", "stable", 8, &hash), &trust).is_err());
    assert!(check(Some(&stored), &make("3.8.0", "stable", 6, &hash), &trust).is_err());
    assert!(check(
        Some(&stored),
        &make("3.7.0", "stable", 8, &"b".repeat(64)),
        &trust
    )
    .is_err());
    assert!(check(Some(b"corrupt"), &old, &trust).is_err());
    let oversized = vec![b' '; LIMIT + 1];
    assert!(check(Some(&oversized), &old, &trust).is_err());
    let mut corrupt: Record = serde_json::from_slice(&stored).unwrap();
    corrupt.envelope = "invalid".into();
    assert!(check(None, &corrupt, &trust).is_err());
    assert!(check(Some(&serde_json::to_vec(&corrupt).unwrap()), &old, &trust).is_err());
}

// Child processes isolate the ProgramData override from the running application.
#[cfg(windows)]
#[test]
fn record_process_child() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let Ok(root) = std::env::var("WINCMD_FREE_FLOOR_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("fixture.json")).unwrap()).unwrap();
    let trust = Trust {
        ed: URL_SAFE_NO_PAD
            .decode(fixture["ed"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
        pq: URL_SAFE_NO_PAD
            .decode(fixture["pq"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
        ed_id: "ed",
        pq_id: "pq",
        epoch: 1,
    };
    let mode = std::env::var("WINCMD_FREE_FLOOR_TEST_MODE").unwrap();
    let old = mode == "rollback";
    let candidate = VerifiedUpdate {
        bytes: Vec::new(),
        envelope: fixture[if old { "old" } else { "current" }]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec(),
        version: if old { "3.6.9" } else { "3.7.0" }.into(),
        channel: crate::settings::UpdateChannel::Stable,
    };
    let lock_name = fixture["lock"].as_str().unwrap();
    let registry = anchor::Anchor::test_user(fixture["registry"].as_str().unwrap().into());
    std::fs::write(root.join(format!("{mode}.started")), b"ready").unwrap();
    let result = record_with_lock(&candidate, &trust, lock_name, &registry);
    if old || mode == "corrupt" {
        assert!(
            result.is_err(),
            "invalid replay or corrupt record must fail"
        );
        return;
    }
    let guard = result.expect("candidate must be recorded");
    if mode == "hold" {
        std::fs::write(root.join("held"), b"ready").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !root.join("release").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "release signal timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    // Simulate a failed installer launch: the process returns and the lock is released.
    drop(guard);
}

#[cfg(windows)]
#[test]
fn separate_processes_serialize_recording_and_retry_after_failed_launch() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let root = std::env::temp_dir().join(format!("wincmd-free-floor-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let trust = Trust {
        ed: ed.verifying_key().to_bytes(),
        pq: pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "ed",
        pq_id: "pq",
        epoch: 1,
    };
    let make = |version: &str| {
        let url = format!(
            "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
        );
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-free-release-v1", "version":version,
            "channel":"stable", "platform":"windows-x86_64", "url":url, "size":2, "sha256":"a".repeat(64)})).unwrap();
        String::from_utf8(
            jws::sign::assemble(
                &payload,
                7,
                &trust.policy(),
                |m| Ok(ed.sign(m).to_bytes().to_vec()),
                |m| {
                    let mut sig = vec![0; 3309];
                    pq.sign(m, &mut sig).unwrap();
                    Ok(sig)
                },
            )
            .unwrap(),
        )
        .unwrap()
    };
    let lock_name = format!("free-test-{}", uuid::Uuid::new_v4().simple());
    let registry_path = format!(
        r"SOFTWARE\WinCommanderPqTest-{}",
        uuid::Uuid::new_v4().simple()
    );
    let fixture = serde_json::json!({"ed":URL_SAFE_NO_PAD.encode(trust.ed), "pq":URL_SAFE_NO_PAD.encode(trust.pq),
        "current":make("3.7.0"), "old":make("3.6.9"), "lock":lock_name, "registry":registry_path});
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&fixture).unwrap(),
    )
    .unwrap();
    let test_module = module_path!().split_once("::").unwrap().1;
    let filter = format!("{test_module}::record_process_child");
    let spawn = |mode: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &filter, "--nocapture"])
            .env("WINCMD_FREE_FLOOR_TEST_ROOT", &root)
            .env("WINCMD_FREE_FLOOR_TEST_MODE", mode)
            .env("ProgramData", &root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let wait_file = |name: &str| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !root.join(name).exists() {
            assert!(Instant::now() < deadline, "{name} signal timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let finish = |mut child: std::process::Child| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("floor child timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child fixture did not run"
        );
    };
    let first = spawn("hold");
    wait_file("held");
    let mut second = spawn("retry");
    wait_file("retry.started");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        second.try_wait().unwrap().is_none(),
        "second process bypassed the lock"
    );
    std::fs::write(root.join("release"), b"release").unwrap();
    finish(first);
    finish(second);
    finish(spawn("rollback"));
    let machine = root.join(option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander"));
    let state = machine.join("machine-state");
    let record_path = state.join(NAME);
    let recorded: Record = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(recorded.version, "3.7.0");
    // A removed file does not erase the independently stored signed floor.
    std::fs::remove_file(&record_path).unwrap();
    finish(spawn("rollback"));
    finish(spawn("retry"));
    // A registry-first interrupted write still rejects an older file and can retry.
    let old = Record {
        version: "3.6.9".into(),
        channel: "stable".into(),
        envelope: fixture["old"].as_str().unwrap().into(),
    };
    std::fs::write(&record_path, serde_json::to_vec(&old).unwrap()).unwrap();
    finish(spawn("rollback"));
    finish(spawn("retry"));
    std::fs::write(&record_path, b"corrupt").unwrap();
    finish(spawn("corrupt"));
    std::fs::remove_file(&record_path).unwrap();
    finish(spawn("retry"));
    let anchor = anchor::Anchor::test_user(registry_path.clone());
    let saved = anchor.read().unwrap().unwrap();
    // Either surviving copy prevents rollback when the other is missing or stale.
    anchor::remove_test_key(&registry_path).unwrap();
    finish(spawn("rollback"));
    finish(spawn("retry"));
    anchor.write(&serde_json::to_vec(&old).unwrap()).unwrap();
    finish(spawn("rollback"));
    finish(spawn("retry"));
    anchor.write(b"corrupt").unwrap();
    finish(spawn("corrupt"));
    anchor.write(&saved).unwrap();
    anchor::remove_test_key(&registry_path).unwrap();
    std::fs::remove_file(record_path).unwrap();
    std::fs::remove_dir(state).unwrap();
    std::fs::remove_dir(machine).unwrap();
    for name in [
        "fixture.json",
        "hold.started",
        "retry.started",
        "rollback.started",
        "corrupt.started",
        "held",
        "release",
    ] {
        std::fs::remove_file(root.join(name)).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}
