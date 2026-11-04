// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

#[tokio::test(flavor = "multi_thread")]
async fn stale_startup_hint_cannot_reenable_removed_enrollment() {
    let enabled = Arc::new(AtomicBool::new(true));
    let sent = Arc::new(AtomicUsize::new(0));
    // Setup observed enabled. Removal holds the same lease, disables the
    // enrollment and only then releases it to the background resume task.
    assert!(enabled.load(Ordering::SeqCst));
    let gate = Arc::new(std::sync::Mutex::new(()));
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let removal_gate = gate.clone();
    let removal = std::thread::spawn(move || {
        let _held = removal_gate.lock().unwrap();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.recv().unwrap();
    let (waiting, observed) = tokio::sync::oneshot::channel();
    let task_gate = gate.clone();
    let task_enabled = enabled.clone();
    let task_sent = sent.clone();
    let task = tokio::spawn(super::super::lifecycle_lease::run_with(
        move || {
            let _ = waiting.send(());
            let _guard = task_gate.lock().unwrap();
            Ok(())
        },
        async move {
            resume_locked(
                || {
                    Ok(task_enabled
                        .load(Ordering::SeqCst)
                        .then(|| serde_json::json!({"enrollmentIntent":"resume"})))
                },
                |_| async move {
                    task_sent.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
        },
    ));
    observed.await.unwrap();
    // The waiting background resume does not hold up unrelated startup work.
    let unrelated = tokio::spawn(async { "startup-ready" });
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), unrelated)
            .await
            .unwrap()
            .unwrap(),
        "startup-ready"
    );
    enabled.store(false, Ordering::SeqCst);
    release_tx.send(()).unwrap();
    removal.join().unwrap();
    task.await.unwrap().unwrap();
    assert_eq!(sent.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn enabled_resume_uses_current_values() {
    let seen = Arc::new(std::sync::Mutex::new(None));
    let out = seen.clone();
    resume_locked(
        || {
            Ok(Some(
                serde_json::json!({"deviceId":"current", "enrollmentIntent":"resume"}),
            ))
        },
        move |args| async move {
            *out.lock().unwrap() = Some(args);
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(
        seen.lock().unwrap().as_ref().unwrap()["deviceId"],
        "current"
    );
}

#[test]
fn resume_preserves_validated_mesh_failover() {
    let args = resume_args(
        "https://primary.example".into(),
        true,
        "key".into(),
        "device".into(),
        Some("https://backup.example/;https://backup.example"),
    );
    assert_eq!(args["enrollmentIntent"], "resume");
    assert_eq!(
        args["meshEndpoints"],
        serde_json::json!(["https://backup.example"])
    );
    let invalid = resume_args(
        "https://primary.example".into(),
        true,
        "key".into(),
        "device".into(),
        Some("http://backup.example"),
    );
    assert_eq!(invalid["meshEndpoints"], serde_json::json!([]));
}
