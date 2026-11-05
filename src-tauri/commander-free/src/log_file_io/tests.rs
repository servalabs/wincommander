// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::mpsc;

fn sealed(date: &str, _body: &str) -> Result<String, String> {
    Ok(format!("L2:{date}:authenticated-ciphertext"))
}

#[cfg(windows)]
#[test]
fn held_ancestor_cannot_be_replaced_during_append() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("logs");
    let moved = directory.path().join("moved");
    std::fs::create_dir(&parent).unwrap();
    let held = hold_ordinary_ancestors(&parent.join("wincommander.log")).unwrap();
    assert!(std::fs::rename(&parent, &moved).is_err());
    drop(held);
    std::fs::rename(&parent, &moved).unwrap();
}

#[cfg(windows)]
#[test]
fn clear_refuses_hard_links_and_directory_links() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.txt");
    let path = directory.path().join("wincommander.log");
    std::fs::write(&target, b"target remains unchanged").unwrap();
    std::fs::hard_link(&target, &path).unwrap();
    assert!(clear(&path).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"target remains unchanged");
    let actual_dir = directory.path().join("actual");
    let redirected = directory.path().join("redirected");
    std::fs::create_dir(&actual_dir).unwrap();
    let actual_log = actual_dir.join("wincommander.log");
    std::fs::write(&actual_log, b"other target unchanged").unwrap();
    std::os::windows::fs::symlink_dir(&actual_dir, &redirected)
        .expect("test requires symlink privilege");
    assert!(clear(&redirected.join("wincommander.log")).is_err());
    assert_eq!(
        std::fs::read(actual_log).unwrap(),
        b"other target unchanged"
    );
}

#[cfg(windows)]
#[test]
fn append_rejects_hard_links_without_touching_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.txt");
    let path = directory.path().join("wincommander.log");
    std::fs::write(&target, b"target remains unchanged").unwrap();
    std::fs::hard_link(&target, &path).unwrap();
    assert!(append_encrypted(&path, || sealed("2026-11-05", "private")).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"target remains unchanged");
}

#[cfg(windows)]
#[test]
fn append_rejects_directory_links_without_touching_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    let redirected = directory.path().join("redirected");
    std::fs::create_dir(&target).unwrap();
    let target_file = target.join("wincommander.log");
    std::fs::write(&target_file, b"target remains unchanged").unwrap();
    std::os::windows::fs::symlink_dir(&target, &redirected)
        .expect("test requires symlink privilege");
    assert!(
        append_encrypted(&redirected.join("wincommander.log"), || sealed(
            "2026-11-05",
            "private"
        ))
        .is_err()
    );
    assert_eq!(
        std::fs::read(target_file).unwrap(),
        b"target remains unchanged"
    );
}

#[cfg(windows)]
#[test]
fn append_rejects_symbolic_link_without_touching_its_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.txt");
    let path = directory.path().join("wincommander.log");
    std::fs::write(&target, b"target remains unchanged").unwrap();
    std::os::windows::fs::symlink_file(&target, &path).expect("test requires symlink privilege");
    assert!(append_encrypted(&path, || sealed("2026-11-05", "private")).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"target remains unchanged");
}

#[test]
fn unavailable_key_or_encryption_never_appends_plaintext() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wincommander.log");
    for error in ["key unavailable", "encryption failed"] {
        assert!(append_encrypted(&path, || Err(error.into())).is_err());
        assert!(!path.exists());
    }
    std::fs::write(&path, b"existing ciphertext\n").unwrap();
    assert!(append_encrypted(&path, || Err("key unavailable".into())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing ciphertext\n");
    assert!(append_encrypted(&path, || Ok("[ENCRYPT_FAIL] private text".into())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing ciphertext\n");
}

#[test]
fn migration_is_all_or_nothing_and_preserves_unicode_input_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wincommander.log");
    let original = "[2026-11-05 01:02:03] [WARN] sensitive\n[😀😀😀😀] detail\n";
    std::fs::write(&path, original).unwrap();
    let calls = std::cell::Cell::new(0);
    assert!(migrate_with(&path, |date, body| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Err("encryption unavailable".into())
        } else {
            sealed(date, body)
        }
    })
    .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    migrate_with(&path, sealed).unwrap();
    let result = std::fs::read_to_string(&path).unwrap();
    assert_eq!(result.lines().count(), 2);
    assert!(result.lines().all(encrypted_log));
    assert!(!result.contains("sensitive") && !result.contains("detail"));
}

#[test]
fn concurrent_append_waits_until_migration_is_committed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wincommander.log");
    std::fs::write(&path, b"legacy text\n").unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_path = path.clone();
    let migration = std::thread::spawn(move || {
        migrate_with(&worker_path, |date, body| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            sealed(date, body)
        })
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        let result = append_encrypted(&writer_path, || sealed("2026-11-05", "new"));
        done_tx.send(()).unwrap();
        result
    });
    assert!(done_rx.recv_timeout(Duration::from_millis(150)).is_err());
    release_tx.send(()).unwrap();
    migration.join().unwrap().unwrap();
    done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    writer.join().unwrap().unwrap();
    let result = std::fs::read_to_string(&path).unwrap();
    assert_eq!(result.lines().count(), 2);
    assert!(result.lines().all(encrypted_log));
}

#[test]
fn slow_migration_preserves_original_and_releases_waiting_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wincommander.log");
    let original = "legacy private text\n";
    std::fs::write(&path, original).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let worker_path = path.clone();
    let migration = std::thread::spawn(move || {
        migrate_with(&worker_path, |date, body| {
            started_tx.send(()).unwrap();
            std::thread::sleep(MAX_MIGRATION_TIME + Duration::from_millis(100));
            sealed(date, body)
        })
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let start = Instant::now();
    let result = append_encrypted(&path, || sealed("2026-11-05", "new"));
    assert!(migration.join().unwrap().is_err());
    result.unwrap();
    assert!(start.elapsed() < Duration::from_secs(5));
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.starts_with(original));
    assert!(content.ends_with("L2:2026-11-05:authenticated-ciphertext\n"));
}

#[test]
fn uncoordinated_append_cannot_be_lost_by_migration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wincommander.log");
    std::fs::write(&path, b"legacy text\n").unwrap();
    let result = migrate_with(&path, |date, body| {
        let mut writer = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|_| "external writer blocked")?;
        writer.write_all(b"external append\n").unwrap();
        sealed(date, body)
    });
    assert!(result.is_err());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with("legacy text\n"));
    #[cfg(not(windows))]
    assert!(content.ends_with("external append\n"));
    #[cfg(windows)]
    assert_eq!(content, "legacy text\n");
}
