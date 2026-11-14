// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::policy_store::{PolicyFs, WindowsPolicyFs};
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

struct ProtectedDir(PathBuf);
impl ProtectedDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("wincmd-fleet-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let status = Command::new("icacls.exe")
            .arg(&path)
            .arg("/setowner")
            .arg("*S-1-5-32-544")
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "could not assign protected test owner"
        );
        WindowsPolicyFs.ensure_dir_secure(&path).unwrap();
        Self(path)
    }
}
impl Drop for ProtectedDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn test_name(name: &str) -> String {
    format!("{}::{name}", module_path!().split_once("::").unwrap().1)
}

fn assert_child_test_passed(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "child test failed: {stdout}");
    assert!(
        stdout.contains("running 1 test") && stdout.contains("1 passed; 0 failed"),
        "child did not run exactly one test: {stdout}"
    );
}

#[test]
fn owner_lease_child() {
    let Some(path) = std::env::var_os("WINCMD_FLEET_MACHINE_OWNER_TEST") else {
        return;
    };
    let expected = std::env::var("WINCMD_FLEET_MACHINE_OWNER_EXPECT").unwrap();
    let result = acquire(Path::new(&path));
    match expected.as_str() {
        "busy" => assert!(result.is_err()),
        "free" => assert!(result.is_ok()),
        "hold" => {
            let _lease = result.unwrap();
            std::fs::write(Path::new(&path).join("child-ready"), b"ready").unwrap();
            std::thread::park_timeout(Duration::from_secs(30));
        }
        _ => panic!("invalid test fixture"),
    }
}

#[test]
fn machine_root_environment_child() {
    let Some(expected) = std::env::var_os("WINCMD_FLEET_MACHINE_ROOT_EXPECTED") else {
        return;
    };
    assert_eq!(
        super::super::machine_store_dir().unwrap(),
        PathBuf::from(expected)
    );
}

#[test]
fn machine_root_is_stable_across_process_environments() {
    let expected = super::super::machine_store_dir().unwrap();
    for suffix in ["first", "second"] {
        let fake = std::env::temp_dir().join(format!("untrusted-fleet-root-{suffix}"));
        let result = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(test_name("machine_root_environment_child"))
            .arg("--nocapture")
            .env("ProgramData", fake)
            .env("WINCMD_FLEET_MACHINE_ROOT_EXPECTED", &expected)
            .output()
            .unwrap();
        assert_child_test_passed(&result);
    }
}

#[test]
fn exclusive_owner_contends_across_processes_and_releases_on_drop() {
    let root = ProtectedDir::new();
    let lease = acquire(&root.0).unwrap();
    assert!(acquire(&root.0).is_err());
    assert!(std::fs::remove_file(root.0.join(LOCK_FILE)).is_err());
    assert!(std::fs::rename(root.0.join(LOCK_FILE), root.0.join("moved-lock")).is_err());
    let child = |expected: &str| {
        Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(test_name("owner_lease_child"))
            .env("WINCMD_FLEET_MACHINE_OWNER_TEST", &root.0)
            .env("WINCMD_FLEET_MACHINE_OWNER_EXPECT", expected)
            .output()
            .unwrap()
    };
    assert_child_test_passed(&child("busy"));
    drop(lease);
    assert_child_test_passed(&child("free"));
}

#[test]
fn protected_parent_cannot_be_replaced_before_child_creation() {
    let root = ProtectedDir::new();
    let child = root.0.join("not-created");
    let moved = root.0.with_extension("moved");
    let parent = protect_parent(&child).unwrap();
    let renamed = std::fs::rename(&root.0, &moved);
    if renamed.is_ok() {
        std::fs::rename(&moved, &root.0).unwrap();
    }
    assert!(renamed.is_err());
    assert!(!child.exists());
    drop(parent);
    std::fs::rename(&root.0, &moved).unwrap();
    std::fs::rename(&moved, &root.0).unwrap();
}

#[test]
fn rejects_untrusted_writer_on_the_store_parent() {
    let root = ProtectedDir::new();
    let result = Command::new("icacls.exe")
        .arg(&root.0)
        .arg("/grant:r")
        .arg("*S-1-1-0:(F)")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(protect_parent(&root.0.join("fleet-machine")).is_err());
    assert!(!root.0.join("fleet-machine").exists());
}

#[test]
fn real_system_store_holds_ownership_until_dropped() {
    let root = ProtectedDir::new();
    let path = root.0.join("fleet-machine");
    let moved_parent = root.0.with_extension("moved");
    std::fs::rename(&root.0, &moved_parent).unwrap();
    std::fs::rename(&moved_parent, &root.0).unwrap();
    let store = super::super::open_system_store(path.clone()).unwrap();
    assert!(super::super::open_system_store(path.clone()).is_err());
    assert!(std::fs::rename(&path, root.0.join("moved-store")).is_err());
    let renamed = std::fs::rename(&root.0, &moved_parent);
    if renamed.is_ok() {
        std::fs::rename(&moved_parent, &root.0).unwrap();
    }
    assert!(
        renamed.is_err(),
        "the store parent must stay fixed while the service owns it"
    );
    let busy = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(test_name("owner_lease_child"))
        .env("WINCMD_FLEET_MACHINE_OWNER_TEST", &path)
        .env("WINCMD_FLEET_MACHINE_OWNER_EXPECT", "busy")
        .output()
        .unwrap();
    assert_child_test_passed(&busy);
    drop(store);
    assert!(super::super::open_system_store(path).is_ok());
}

#[test]
fn killed_owner_releases_the_lock_for_another_process() {
    let root = ProtectedDir::new();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(test_name("owner_lease_child"))
        .env("WINCMD_FLEET_MACHINE_OWNER_TEST", &root.0)
        .env("WINCMD_FLEET_MACHINE_OWNER_EXPECT", "hold")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.0.join("child-ready").exists()
        && child.try_wait().unwrap().is_none()
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let ready = root.0.join("child-ready").exists();
    let busy = acquire(&root.0).is_err();
    let killed = child.kill();
    let exited = child.wait().is_ok();
    assert!(ready && busy && killed.is_ok() && exited);
    assert!(acquire(&root.0).is_ok());
}

#[test]
fn rejects_nonempty_and_hardlinked_lock_without_changing_bytes() {
    let root = ProtectedDir::new();
    let lock = root.0.join(LOCK_FILE);
    std::fs::write(&lock, b"retained fixture").unwrap();
    assert!(acquire(&root.0).is_err());
    assert_eq!(std::fs::read(&lock).unwrap(), b"retained fixture");
    std::fs::remove_file(&lock).unwrap();
    std::fs::write(&lock, b"").unwrap();
    protect_file(&lock);
    assert!(acquire(&root.0).is_ok());
    let target = root.0.join("other-file");
    std::fs::hard_link(&lock, &target).unwrap();
    assert!(acquire(&root.0).is_err());
    assert_eq!(std::fs::metadata(&target).unwrap().len(), 0);
}
fn protect_file(lock: &Path) {
    let owner = Command::new("icacls.exe")
        .arg(lock)
        .arg("/setowner")
        .arg("*S-1-5-32-544")
        .output()
        .unwrap();
    assert!(owner.status.success());
    let grant = Command::new("icacls.exe")
        .arg(lock)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg("*S-1-5-18:(F)")
        .arg("*S-1-5-32-544:(F)")
        .output()
        .unwrap();
    assert!(grant.status.success());
}

// Existing lock ACLs are never repaired or adopted by the owner lease.
#[test]
fn rejects_untrusted_writer_on_a_preexisting_lock() {
    let root = ProtectedDir::new();
    let lock = root.0.join(LOCK_FILE);
    std::fs::write(&lock, b"").unwrap();
    protect_file(&lock);
    assert!(acquire(&root.0).is_ok());
    let grant = Command::new("icacls.exe")
        .arg(&lock)
        .arg("/grant:r")
        .arg("*S-1-1-0:(F)")
        .output()
        .unwrap();
    assert!(grant.status.success());
    assert!(acquire(&root.0).is_err());
    assert_eq!(std::fs::metadata(&lock).unwrap().len(), 0);
}

#[test]
fn rejects_a_junction_at_the_lock_path() {
    let root = ProtectedDir::new();
    let target = root.0.join("junction-target");
    let lock = root.0.join(LOCK_FILE);
    std::fs::create_dir(&target).unwrap();
    let result = Command::new("cmd.exe")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(&lock)
        .arg(&target)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(acquire(&root.0).is_err());
    assert!(target.is_dir());
    std::fs::remove_dir(&lock).unwrap();
}
