// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn journal_cannot_supply_an_alternative_deletion_target() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let canary = directory.path().join("unrelated.txt");
    let journal = directory.path().join(JOURNAL_FILENAME);
    std::fs::write(&path, b"original").unwrap();
    std::fs::write(&canary, b"must remain").unwrap();
    let (_, source) = read(&path).unwrap();
    let settings = serde_json::json!({});
    prepare_journal(&mut Some(source), &journal, &settings, &settings).unwrap();
    let mut receipt: Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    receipt["path"] = serde_json::json!(canary);
    std::fs::write(&journal, serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert!(resume_journal(&journal, &path, &settings, &settings).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(std::fs::read(&canary).unwrap(), b"must remain");
}

#[test]
fn durable_journal_cleans_plaintext_after_restart_with_both_commits_verified() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let journal = directory.path().join(JOURNAL_FILENAME);
    std::fs::write(&path, b"original legacy secret").unwrap();
    let (_, source) = read(&path).unwrap();
    let machine = serde_json::json!({"policy": {"managed": true}, "appVersion": "old"});
    let user = serde_json::json!({"app": {"theme": "dark"}, "lastSeenAt": "old"});
    prepare_journal(&mut Some(source), &journal, &machine, &user).unwrap();
    let journal_text = std::fs::read_to_string(&journal).unwrap();
    assert!(!journal_text.contains("original legacy secret"));
    assert!(!journal_text.contains("path"));
    let mut loaded_machine = machine.clone();
    loaded_machine["appVersion"] = serde_json::json!("new");
    let mut loaded_user = user.clone();
    loaded_user["lastSeenAt"] = serde_json::json!("new");
    resume_journal(&journal, &path, &loaded_machine, &loaded_user).unwrap();
    assert!(!path.exists());
    assert!(!journal.exists());
}

#[test]
fn journal_preserves_plaintext_when_either_committed_partition_does_not_match() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let journal = directory.path().join(JOURNAL_FILENAME);
    std::fs::write(&path, b"original").unwrap();
    let (_, source) = read(&path).unwrap();
    let machine = serde_json::json!({"policy": {"managed": true}});
    let user = serde_json::json!({"app": {"theme": "dark"}});
    prepare_journal(&mut Some(source), &journal, &machine, &user).unwrap();
    resume_journal(&journal, &path, &serde_json::json!({}), &user).unwrap();
    assert!(path.exists());
    resume_journal(&journal, &path, &machine, &serde_json::json!({})).unwrap();
    assert!(path.exists());
    assert!(journal.exists());
}

#[test]
fn plaintext_is_removed_only_after_successful_encrypted_persistence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, b"{\"secret\":\"test\"}").unwrap();
    let (text, source) = read(&path).unwrap();
    let mut pending = Some(source);
    complete(&mut pending, false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(pending.is_some());
    complete(&mut pending, true).unwrap();
    assert!(!path.exists());
    assert!(pending.is_none());
}

#[test]
fn modified_source_is_preserved_after_successful_persistence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, b"old").unwrap();
    let (_, source) = read(&path).unwrap();
    std::fs::write(&path, b"new").unwrap();
    assert!(complete(&mut Some(source), true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
}

#[test]
fn reload_without_migration_cannot_delete_an_earlier_uncommitted_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, b"original legacy secret").unwrap();
    let (_, source) = read(&path).unwrap();
    let mut pending = Some(source);
    complete(&mut pending, false).unwrap();
    clear_pending(&mut pending);
    complete(&mut pending, true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"original legacy secret");
    assert!(pending.is_none());
}

#[test]
fn identical_replacement_is_preserved_by_file_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, b"original").unwrap();
    let (_, source) = read(&path).unwrap();
    std::fs::rename(&path, directory.path().join("original.json")).unwrap();
    std::fs::write(&path, b"original").unwrap();
    assert!(complete(&mut Some(source), true).is_err());
    assert!(path.exists());
}

#[test]
fn blocked_cleanup_preserves_source_until_a_later_success() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, b"original").unwrap();
    let (_, source) = read(&path).unwrap();
    let mut pending = Some(source);
    let busy = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    assert!(complete(&mut pending, true).is_err());
    assert!(pending.is_some());
    drop(busy);
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    complete(&mut pending, true).unwrap();
    assert!(!path.exists());
}
