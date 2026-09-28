// SPDX-License-Identifier: AGPL-3.0-or-later
//! Schedule bounded private-volume work without retaining volume handles.
use super::privacy::{PrivateShard, SearchPlan};
use super::private_index::reconcile;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Job {
    generation: String,
    cancel: Arc<AtomicBool>,
    running: bool,
    last_run: Instant,
    complete: bool,
    count: u64,
    error: bool,
}

fn jobs() -> &'static Mutex<HashMap<String, Job>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

/// Invoked by status/search polling. Changed mount generations cancel queued work;
/// a worker rechecks the native identity under the dismount gate before writing.
pub(super) fn refresh(plan: &SearchPlan, device: &str, exclusions: &[String], force: bool) {
    let Ok(mut state) = jobs().lock() else {
        return;
    };
    for (key, job) in state.iter_mut() {
        if !plan.private.iter().any(|s| &s.volume.identity == key)
            || job.generation != plan.generation
        {
            job.cancel.store(true, Ordering::Release);
        }
    }
    state.retain(|key, job| job.running || plan.private.iter().any(|s| &s.volume.identity == key));
    for shard in &plan.private {
        if let Some(job) = state.get(&shard.volume.identity) {
            if job.running {
                continue;
            }
            let interval = if job.complete {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(1)
            };
            if !force && job.generation == plan.generation && job.last_run.elapsed() < interval {
                continue;
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let key = shard.volume.identity.clone();
        let count = state.get(&key).map_or(0, |job| job.count);
        state.insert(
            key.clone(),
            Job {
                generation: plan.generation.clone(),
                cancel: cancel.clone(),
                running: true,
                last_run: Instant::now(),
                complete: false,
                count,
                error: false,
            },
        );
        let shard = shard.clone();
        let device = device.to_string();
        let exclusions = exclusions.to_vec();
        std::thread::spawn(move || {
            let result = reconcile(&shard, &device, &exclusions, &cancel);
            if let Ok(mut state) = jobs().lock() {
                if let Some(job) = state.get_mut(&key) {
                    job.running = false;
                    job.last_run = Instant::now();
                    match result {
                        Ok((count, complete)) => {
                            job.count = count;
                            job.complete = complete;
                        }
                        Err(_) => {
                            job.error = true;
                            job.complete = true;
                        }
                    }
                }
            }
        });
    }
}

pub(super) fn state(shard: &PrivateShard) -> (&'static str, &'static str, u64) {
    let Ok(state) = jobs().lock() else {
        return ("unavailable", "Private index status is unavailable.", 0);
    };
    match state.get(&shard.volume.identity) {
        Some(job) if job.error => (
            "unavailable",
            "Private index could not be updated; no external index was used.",
            job.count,
        ),
        Some(job) if shard.volume.read_only => (
            "read_only",
            "Existing private index is read-only; updates require a writable mount.",
            job.count,
        ),
        Some(job) if job.running || !job.complete => (
            "indexing",
            "Updating the index inside this volume.",
            job.count,
        ),
        Some(job) => (
            "ready",
            "Search index is stored inside this volume.",
            job.count,
        ),
        None => (
            "indexing",
            "Waiting to build the index inside this volume.",
            0,
        ),
    }
}

pub(super) fn cancel_all() {
    if let Ok(mut state) = jobs().lock() {
        for job in state.values_mut() {
            job.cancel.store(true, Ordering::Release);
            job.generation.clear();
        }
    }
}
