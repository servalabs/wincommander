// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn seed() {
    let mut settings = create_default_settings();
    settings.snapshot_revision = Some(Uuid::new_v4());
    *SETTINGS_CACHE.lock().unwrap() = Some(settings);
    DECOY_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn committed_snapshot_remains_readable_while_patch_persistence_is_blocked() {
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seed();
    let before = serde_json::to_value(cached_settings().unwrap()).unwrap();
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        patch_settings_with(
            serde_json::json!({"app": {"theme": "light"}}),
            true,
            |_| {
                started.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
            |_, _| {
                assert!(SETTINGS_TRANSACTION_GATE.try_lock().is_ok());
                assert!(read_settings().is_ok());
            },
        )
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let (read, observed) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        read.send(read_settings().map(|s| serde_json::to_value(s).unwrap()))
            .unwrap();
    });
    let snapshot = observed.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    reader.join().unwrap();
    assert_eq!(snapshot.unwrap().unwrap(), before);
    assert_eq!(
        serde_json::to_value(cached_settings().unwrap()).unwrap()["app"]["theme"],
        "light"
    );
}

#[test]
fn full_write_and_patch_share_a_transaction_without_blocking_snapshot_reads() {
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seed();
    let mut full = cached_settings().unwrap();
    full.app.theme = "light".into();
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let full_writer = std::thread::spawn(move || {
        write_settings_with(&full, |_| {
            started.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let patcher = std::thread::spawn(|| {
        patch_settings_with(
            serde_json::json!({"app": {"lastPanel": "privacy"}}),
            false,
            |candidate| {
                assert_eq!(candidate.app.theme, "light");
                Ok(())
            },
            |_, _| {},
        )
    });
    let available = SETTINGS_CACHE.try_lock().is_ok();
    release.send(()).unwrap();
    full_writer.join().unwrap().unwrap();
    patcher.join().unwrap().unwrap();
    assert!(available);
    let committed = serde_json::to_value(cached_settings().unwrap()).unwrap();
    assert_eq!(committed["app"]["theme"], "light");
    assert_eq!(committed["app"]["lastPanel"], "privacy");
}

#[test]
fn queued_mutation_rechecks_decoy_after_previous_writer_finishes() {
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seed();
    let gate = SETTINGS_TRANSACTION_GATE.lock().unwrap();
    let writer = std::thread::spawn(|| {
        patch_settings_with(
            serde_json::json!({"app": {"theme": "light"}}),
            false,
            |_| panic!("decoy mutation reached persistence"),
            |_, _| {},
        )
    });
    DECOY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    drop(gate);
    let result = writer.join().unwrap();
    DECOY_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(result.unwrap_err().contains("read-only in decoy mode"));
}

#[test]
fn invalidation_waits_for_pending_commit_then_removes_its_snapshot() {
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seed();
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        patch_settings_with(
            serde_json::json!({"app": {"theme": "light"}}),
            false,
            |_| {
                started.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
            |_, _| {},
        )
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let invalidator = std::thread::spawn(invalidate_cache);
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    invalidator.join().unwrap();
    assert!(cached_settings().is_none());
}

#[test]
fn stale_full_write_cannot_overwrite_a_committed_patch_or_reloaded_snapshot() {
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seed();
    let mut stale = read_settings().unwrap();
    stale.app.theme = "light".into();
    patch_settings_with(
        serde_json::json!({"app": {"lastPanel": "privacy"}}),
        false,
        |_| Ok(()),
        |_, _| {},
    )
    .unwrap();
    assert!(write_settings_with(&stale, |_| panic!("stale snapshot persisted")).is_err());
    let before = cached_settings().unwrap();
    assert_eq!(before.app.last_panel, "privacy");
    invalidate_cache();
    assert!(write_settings_with(&before, |_| panic!("invalidated snapshot persisted")).is_err());
    seed();
    assert!(write_settings_with(&before, |_| panic!("pre-reload snapshot persisted")).is_err());
    let untrusted: AppSettings =
        serde_json::from_value(serde_json::to_value(cached_settings().unwrap()).unwrap()).unwrap();
    assert!(untrusted.snapshot_revision.is_none());
    assert!(write_settings_with(&untrusted, |_| panic!("unversioned snapshot persisted")).is_err());
}

#[test]
fn failed_full_write_invalidates_cache_without_publishing_candidate() {
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seed();
    let mut candidate = read_settings().unwrap();
    candidate.app.theme = "light".into();
    assert!(write_settings_with(&candidate, |_| Err("unknown result".into())).is_err());
    assert!(cached_settings().is_none());
}
