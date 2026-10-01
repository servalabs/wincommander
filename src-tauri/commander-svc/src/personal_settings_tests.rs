// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::json;
use std::sync::{Arc, Barrier};

struct TestRoot(PathBuf);
impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "wincmd-personal-settings-{:016x}",
            rand::random::<u64>()
        )))
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn write(
    root: &Path,
    sid: &str,
    revision: u64,
    value: serde_json::Value,
) -> StoreResult<PersonalSettingsRecord> {
    serialized(
        root,
        sid,
        WRITE_PERSONAL_SETTINGS_VERB,
        json!({
            "expectedRevision": revision, "value": value, "legacyRecoveryRequired": false
        }),
    )
}
fn read(root: &Path, sid: &str) -> StoreResult<PersonalSettingsRecord> {
    serialized(root, sid, READ_PERSONAL_SETTINGS_VERB, json!({}))
}

fn serialized(
    root: &Path,
    sid: &str,
    verb: &'static str,
    args: serde_json::Value,
) -> StoreResult<PersonalSettingsRecord> {
    let fixture_account = format!("{}:{sid}", root.display());
    let root = root.to_owned();
    let owner = sid.to_owned();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scheduler().run(&fixture_account, move || execute(&root, verb, args, &owner)))
}

#[tokio::test]
async fn personal_settings_refuse_missing_peer_before_filesystem_access() {
    assert_eq!(
        handle(READ_PERSONAL_SETTINGS_VERB, json!({}), None).await,
        Err("personal_settings_unauthorized")
    );
    assert_eq!(
        handle(WRITE_PERSONAL_SETTINGS_VERB, json!({}), None).await,
        Err("personal_settings_unauthorized")
    );
}

#[test]
fn personal_settings_eight_accounts_remain_separate_across_reloads() {
    let root = Arc::new(TestRoot::new());
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (1..=8)
        .map(|id| {
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let sid = format!("S-1-5-21-1-2-3-{id}");
                assert_eq!(read(&root.0, &sid).unwrap().revision, 0);
                write(&root.0, &sid, 0, json!({"theme": id})).unwrap();
                assert_eq!(
                    read(&root.0, &sid).unwrap().value,
                    Some(json!({"theme": id}))
                );
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    for id in 1..=8 {
        assert_eq!(
            read(&root.0, &format!("S-1-5-21-1-2-3-{id}"))
                .unwrap()
                .value,
            Some(json!({"theme": id}))
        );
    }
    assert!(read(&root.0, "S-1-5-21-1-2-3-9").unwrap().value.is_none());
}

#[test]
fn personal_settings_same_revision_has_exactly_one_winner() {
    let root = Arc::new(TestRoot::new());
    let workers: Vec<_> = (0..4)
        .map(|id| {
            let root = root.clone();
            std::thread::spawn(move || write(&root.0, "S-1-5-21-42", 0, json!({"theme": id})))
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err("personal_settings_conflict")))
            .count(),
        3
    );
    assert_eq!(read(&root.0, "S-1-5-21-42").unwrap().revision, 1);
}

#[test]
fn personal_settings_recovery_notice_is_sticky() {
    let root = TestRoot::new();
    execute(
        &root.0,
        WRITE_PERSONAL_SETTINGS_VERB,
        json!({
            "expectedRevision": 0, "value": {}, "legacyRecoveryRequired": true
        }),
        "S-1-5-21-42",
    )
    .unwrap();
    let result = execute(
        &root.0,
        WRITE_PERSONAL_SETTINGS_VERB,
        json!({
            "expectedRevision": 1, "value": {"theme": "dark"}, "legacyRecoveryRequired": false
        }),
        "S-1-5-21-42",
    )
    .unwrap();
    assert!(result.legacy_recovery_required);
}

#[test]
fn personal_settings_reject_identity_injection_invalid_and_oversized_values() {
    let root = TestRoot::new();
    assert_eq!(
        execute(
            &root.0,
            READ_PERSONAL_SETTINGS_VERB,
            json!({"sid": "other"}),
            "caller"
        ),
        Err(INVALID)
    );
    for value in [
        json!(null),
        json!([]),
        json!({"text": "x".repeat(MAX_PERSONAL_SETTINGS_BYTES)}),
    ] {
        assert_eq!(write(&root.0, "caller", 0, value), Err(INVALID));
    }
    assert!(!root.0.exists());
}

#[test]
fn personal_settings_copied_ciphertext_and_corruption_fail_without_overwrite() {
    let root = TestRoot::new();
    write(&root.0, "first", 0, json!({"secret": "not plaintext"})).unwrap();
    let original = std::fs::read(record_path(&root.0, "first")).unwrap();
    assert!(!original
        .windows(13)
        .any(|window| window == b"not plaintext"));
    let _guard = platform::secure_directory(&root.0).unwrap();
    atomic_write(&record_path(&root.0, "second"), &original).unwrap();
    assert_eq!(read(&root.0, "second"), Err(CORRUPT));
    atomic_write(&record_path(&root.0, "first"), b"broken").unwrap();
    assert_eq!(write(&root.0, "first", 1, json!({})), Err(CORRUPT));
    assert_eq!(
        std::fs::read(record_path(&root.0, "first")).unwrap(),
        b"broken"
    );
}

#[test]
fn personal_settings_revision_overflow_preserves_record() {
    let root = TestRoot::new();
    let _guard = platform::secure_directory(&root.0).unwrap();
    let payload = serde_json::to_vec(&StoredRecord {
        version: 1,
        owner_sid: "caller".into(),
        record: PersonalSettingsRecord {
            revision: u64::MAX,
            value: Some(json!({})),
            legacy_recovery_required: false,
        },
    })
    .unwrap();
    atomic_write(
        &record_path(&root.0, "caller"),
        &platform::dpapi(&payload, "caller", true).unwrap(),
    )
    .unwrap();
    assert_eq!(
        write(&root.0, "caller", u64::MAX, json!({})),
        Err("personal_settings_revision_exhausted")
    );
    assert_eq!(read(&root.0, "caller").unwrap().revision, u64::MAX);
}

#[test]
fn personal_settings_refuse_unprotected_preexisting_directory_and_hardlinks() {
    let unprotected = TestRoot::new();
    std::fs::create_dir(&unprotected.0).unwrap();
    assert_eq!(read(&unprotected.0, "caller"), Err(UNAVAILABLE));
    let root = TestRoot::new();
    write(&root.0, "first", 0, json!({})).unwrap();
    std::fs::hard_link(
        record_path(&root.0, "first"),
        record_path(&root.0, "second"),
    )
    .unwrap();
    assert_eq!(read(&root.0, "first"), Err(UNAVAILABLE));
    assert_eq!(read(&root.0, "second"), Err(UNAVAILABLE));
}

#[test]
fn personal_settings_refuse_directory_reparse_points() {
    let real = TestRoot::new();
    let link = TestRoot::new();
    let _guard = platform::secure_directory(&real.0).unwrap();
    std::os::windows::fs::symlink_dir(&real.0, &link.0).unwrap();
    assert_eq!(read(&link.0, "caller"), Err(UNAVAILABLE));
    // Remove only the link; the real directory's guard still prevents deletion.
    std::fs::remove_dir(&link.0).unwrap();
}

#[test]
fn personal_settings_refuse_file_reparse_points_and_oversized_disk_records() {
    let root = TestRoot::new();
    let _guard = platform::secure_directory(&root.0).unwrap();
    let source = record_path(&root.0, "source");
    atomic_write(&source, b"preserved").unwrap();
    let redirected = record_path(&root.0, "caller");
    std::os::windows::fs::symlink_file(&source, &redirected).unwrap();
    assert_eq!(read(&root.0, "caller"), Err(UNAVAILABLE));
    std::fs::remove_file(&redirected).unwrap();
    atomic_write(&redirected, &vec![0; MAX_RECORD_BYTES as usize + 1]).unwrap();
    assert_eq!(read(&root.0, "caller"), Err(CORRUPT));
    assert_eq!(std::fs::read(source).unwrap(), b"preserved");
}

#[test]
fn personal_settings_accept_expanded_encrypted_legacy_profile() {
    let root = TestRoot::new();
    let value = json!({"app": {"theme": "dark"},
        "_personalSecrets": format!("enc:v2:{}", "A".repeat(1_400_000))});
    let committed = write(&root.0, "caller", 0, value.clone()).unwrap();
    assert_eq!(committed.revision, 1);
    assert_eq!(read(&root.0, "caller").unwrap().value, Some(value));
}
