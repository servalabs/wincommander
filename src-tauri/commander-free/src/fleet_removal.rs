// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reconcile a server-approved removal against the exact local agent session.
use serde_json::Value;

#[derive(Clone)]
pub(super) struct PendingRemoval {
    pub(super) fence: String,
    pub(super) settings_binding: Value,
    pub(super) service_token: Option<String>,
    pub(super) connection_token: Option<String>,
    pub(super) settings_disabled: bool,
}

pub(super) trait RemovalBackend {
    async fn stop(&self, fence: &str) -> Result<Value, String>;
    fn disable(&self, binding: &Value) -> Result<(), String>;
    fn save(&self, pending: Option<PendingRemoval>) -> Result<(), String>;
    async fn clear(&self, token: &str, connection_token: &str) -> ClearResult;
}

pub(super) enum ClearResult {
    Cleared,
    Rejected,
    Unconfirmed,
}

pub(super) async fn finish<B: RemovalBackend>(
    backend: &B,
    mut pending: PendingRemoval,
) -> Result<Value, String> {
    let stopped = backend.stop(&pending.fence).await?;
    if stopped.get("ok").and_then(Value::as_bool) != Some(true)
        || stopped.get("agentStopped").and_then(Value::as_bool) != Some(true)
        || stopped.get("sessionFence").and_then(Value::as_str) != Some(pending.fence.as_str())
    {
        return Err("Could not confirm the original Fleet agent stopped; retry removal.".into());
    }
    if !pending.settings_disabled {
        backend.disable(&pending.settings_binding)?;
        pending.settings_disabled = true;
        backend.save(Some(pending.clone()))?;
    }
    let result = match (&pending.service_token, &pending.connection_token) {
        (Some(token), Some(connection_token)) => backend.clear(token, connection_token).await,
        _ => ClearResult::Rejected,
    };
    let service_stopped = matches!(result, ClearResult::Cleared);
    let retryable = matches!(result, ClearResult::Unconfirmed);
    if !retryable {
        backend.save(None)?;
    }
    Ok(
        serde_json::json!({"ok":true,"serviceReachabilityStopped":service_stopped,"cleanupRetryable":retryable}),
    )
}

#[cfg(test)]
#[path = "fleet_removal_tests.rs"]
mod tests;
