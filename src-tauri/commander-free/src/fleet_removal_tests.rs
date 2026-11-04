// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::json;
use std::sync::Mutex;

#[derive(Default)]
struct State {
    fence: String,
    fail_stop: bool,
    clear: bool,
    reject: bool,
    stops: usize,
    disables: usize,
    clears: usize,
    saved: Option<PendingRemoval>,
}
struct Backend(Mutex<State>);
impl RemovalBackend for Backend {
    async fn stop(&self, fence: &str) -> Result<Value, String> {
        let mut state = self.0.lock().unwrap();
        state.stops += 1;
        if state.fail_stop || fence != state.fence {
            return Err("stop unconfirmed or stale".into());
        }
        Ok(json!({"ok":true,"agentStopped":true,"sessionFence":fence}))
    }
    fn disable(&self, _: &Value) -> Result<(), String> {
        self.0.lock().unwrap().disables += 1;
        Ok(())
    }
    fn save(&self, pending: Option<PendingRemoval>) -> Result<(), String> {
        self.0.lock().unwrap().saved = pending;
        Ok(())
    }
    async fn clear(&self, _: &str, _: &str) -> ClearResult {
        let mut state = self.0.lock().unwrap();
        state.clears += 1;
        if state.clear {
            ClearResult::Cleared
        } else if state.reject {
            ClearResult::Rejected
        } else {
            ClearResult::Unconfirmed
        }
    }
}
fn pending(token: Option<&str>) -> PendingRemoval {
    PendingRemoval {
        fence: "one".into(),
        settings_binding: json!({}),
        service_token: token.map(str::to_owned),
        connection_token: token.map(|_| "intent".to_owned()),
        settings_disabled: false,
    }
}

#[tokio::test]
async fn failure_retry_and_reconnection_never_disable_a_newer_session() {
    let backend = Backend(Mutex::new(State {
        fence: "one".into(),
        fail_stop: true,
        ..State::default()
    }));
    assert!(finish(&backend, pending(Some("token"))).await.is_err());
    assert_eq!(backend.0.lock().unwrap().disables, 0);
    {
        let mut state = backend.0.lock().unwrap();
        state.fail_stop = false;
        state.fence = "two".into();
    }
    assert!(finish(&backend, pending(Some("token"))).await.is_err());
    assert_eq!(backend.0.lock().unwrap().disables, 0);
    backend.0.lock().unwrap().fence = "one".into();
    let first = finish(&backend, pending(Some("token"))).await.unwrap();
    assert_eq!(first["cleanupRetryable"], true);
    let saved = backend.0.lock().unwrap().saved.clone().unwrap();
    backend.0.lock().unwrap().clear = true;
    let second = finish(&backend, saved).await.unwrap();
    assert_eq!(second["serviceReachabilityStopped"], true);
    assert_eq!(backend.0.lock().unwrap().disables, 1);
}

#[tokio::test]
async fn unavailable_service_does_not_prevent_local_removal() {
    let backend = Backend(Mutex::new(State {
        fence: "one".into(),
        ..State::default()
    }));
    let result = finish(&backend, pending(None)).await.unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["serviceReachabilityStopped"], false);
    assert_eq!(result["cleanupRetryable"], false);
    assert_eq!(backend.0.lock().unwrap().disables, 1);
}

#[tokio::test]
async fn stale_service_token_is_not_retried_or_refreshed() {
    let backend = Backend(Mutex::new(State {
        fence: "one".into(),
        reject: true,
        ..State::default()
    }));
    let result = finish(&backend, pending(Some("old-token"))).await.unwrap();
    assert_eq!(result["serviceReachabilityStopped"], false);
    assert_eq!(result["cleanupRetryable"], false);
    assert!(backend.0.lock().unwrap().saved.is_none());
}

#[tokio::test]
async fn unsafe_coordination_object_prevents_all_removal_effects() {
    let backend = std::sync::Arc::new(Backend(Mutex::new(State {
        fence: "one".into(),
        ..State::default()
    })));
    let task_backend = backend.clone();
    let result = super::super::lifecycle_lease::run_with(
        || Err::<(), _>("Fleet connection coordination is unavailable or unsafe".into()),
        async move { finish(&*task_backend, pending(Some("old-token"))).await },
    )
    .await;
    assert!(result.is_err());
    let state = backend.0.lock().unwrap();
    assert_eq!((state.stops, state.disables, state.clears), (0, 0, 0));
}
