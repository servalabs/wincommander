// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn timed_out_callers_cannot_spawn_more_workers_for_a_stuck_probe() {
    let probe = Arc::new(Probe::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let (release, blocked) = std::sync::mpsc::channel();
    let first_calls = calls.clone();
    probe.refresh(Duration::ZERO, move || {
        first_calls.fetch_add(1, Ordering::SeqCst);
        blocked.recv().map_err(|e| e.to_string())?;
        Ok(42)
    });
    let timed_out = probe.wait(Duration::from_millis(30)).await;
    let mut callers = Vec::new();
    for _ in 0..24 {
        let probe = probe.clone();
        let calls = calls.clone();
        callers.push(std::thread::spawn(move || {
            probe.refresh(Duration::ZERO, move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(99)
            });
            probe.snapshot(Duration::from_secs(1)).status
        }));
    }
    let statuses: Vec<_> = callers
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    release.send(()).unwrap();
    let result = probe.wait(Duration::from_secs(2)).await;
    assert!(timed_out.is_err());
    assert!(statuses.iter().all(|status| *status == "loading"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn expired_and_failed_observations_are_never_labelled_live() {
    let probe = Probe::new();
    {
        let mut state = probe.state.lock().unwrap();
        state.value = Some(7);
        state.observed = Some(Instant::now() - Duration::from_secs(10));
    }
    let stale = probe.snapshot(Duration::from_secs(5));
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.value, Some(7));
    assert!(stale.age_ms.unwrap() >= 10_000);
    {
        let mut state = probe.state.lock().unwrap();
        state.observed = Some(Instant::now());
        state.failed = true;
    }
    assert_eq!(probe.snapshot(Duration::from_secs(5)).status, "stale");
    probe.state.lock().unwrap().value = None;
    assert_eq!(probe.snapshot(Duration::from_secs(5)).status, "unavailable");
}

#[tokio::test]
async fn a_stuck_disk_probe_does_not_hold_up_cpu_samples() {
    let disk = Probe::<u8>::new();
    let cpu = Probe::new();
    let (release, blocked) = std::sync::mpsc::channel();
    disk.refresh(Duration::ZERO, move || {
        blocked.recv().unwrap();
        Ok(1)
    });
    cpu.refresh(Duration::ZERO, || Ok(35));
    let cpu_result = cpu.wait(Duration::from_secs(2)).await;
    let disk_status = disk.snapshot(Duration::from_secs(5)).status;
    release.send(()).unwrap();
    disk.wait(Duration::from_secs(2)).await.unwrap();
    assert_eq!(cpu_result.unwrap(), 35);
    assert_eq!(disk_status, "loading");
}
