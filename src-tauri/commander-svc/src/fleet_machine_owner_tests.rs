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
    assert!(child("busy").status.success());
    drop(lease);
    assert!(child("free").status.success());
}

#[test]
fn real_system_store_holds_ownership_until_dropped() {
    let root = ProtectedDir::new();
    let path = root.0.join("fleet-machine");
    let store = super::super::open_system_store(path.clone()).unwrap();
    assert!(super::super::open_system_store(path.clone()).is_err());
    let busy = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(test_name("owner_lease_child"))
        .env("WINCMD_FLEET_MACHINE_OWNER_TEST", &path)
        .env("WINCMD_FLEET_MACHINE_OWNER_EXPECT", "busy")
        .output()
        .unwrap();
    assert!(busy.status.success());
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
