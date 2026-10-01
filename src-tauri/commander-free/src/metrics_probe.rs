// SPDX-License-Identifier: AGPL-3.0-or-later
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) struct Probe<T> {
    state: Arc<Mutex<State<T>>>,
}

struct State<T> {
    value: Option<T>,
    observed: Option<Instant>,
    attempted: Option<Instant>,
    running: bool,
    failed: bool,
}

pub(super) struct Snapshot<T> {
    pub value: Option<T>,
    pub status: &'static str,
    pub age_ms: Option<u64>,
}

impl<T: Clone + Send + 'static> Probe<T> {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                value: None,
                observed: None,
                attempted: None,
                running: false,
                failed: false,
            })),
        }
    }

    pub fn refresh(
        &self,
        ttl: Duration,
        collect: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.running || state.attempted.is_some_and(|at| at.elapsed() < ttl) {
                return;
            }
            state.running = true;
            state.attempted = Some(Instant::now());
        }
        let shared = self.state.clone();
        let spawned = std::thread::Builder::new()
            .name("metrics-probe".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(collect));
                let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(Ok(value)) => {
                        state.value = Some(value);
                        state.observed = Some(Instant::now());
                        state.failed = false;
                    }
                    _ => state.failed = true,
                }
                state.running = false;
            });
        if spawned.is_err() {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.running = false;
            state.failed = true;
        }
    }

    pub fn snapshot(&self, ttl: Duration) -> Snapshot<T> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let age = state.observed.map(|at| at.elapsed());
        let status = if state.value.is_none() {
            if state.failed {
                "unavailable"
            } else {
                "loading"
            }
        } else if state.failed || age.is_some_and(|age| age >= ttl) {
            "stale"
        } else {
            "live"
        };
        Snapshot {
            value: state.value.clone(),
            status,
            age_ms: age.map(|age| age.as_millis().min(u64::MAX as u128) as u64),
        }
    }

    pub async fn wait(&self, budget: Duration) -> Result<T, String> {
        let deadline = Instant::now() + budget;
        loop {
            {
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if !state.running {
                    return if state.failed {
                        Err("Hardware probe unavailable".into())
                    } else {
                        state
                            .value
                            .clone()
                            .ok_or_else(|| "Hardware probe unavailable".into())
                    };
                }
            }
            if Instant::now() >= deadline {
                return Err("Hardware probe timed out".into());
            }
            // The native worker retains its slot until it actually exits.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[cfg(test)]
#[path = "metrics_probe_tests.rs"]
mod tests;
