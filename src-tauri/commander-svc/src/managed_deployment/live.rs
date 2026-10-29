// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed machine store and authenticated private helper boundary.

use super::{authority::Decision, registry::Store};
use serde_json::Value;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use wincmd_shared::{
    managed_deployment::{ManagedDeploymentContext, ManagedDeploymentRequest},
    vault_access::VaultPresentation,
};

static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) async fn status(args: Value) -> Result<Value, &'static str> {
    let context: ManagedDeploymentContext =
        serde_json::from_value(args).map_err(|_| "management_context_invalid")?;
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    let state = Store::existing()?
        .map(|store| store.load())
        .transpose()?
        .unwrap_or_default();
    Ok(
        serde_json::json!({"bound":state.context.is_some(),"matches":state.context.as_ref()==Some(&context)}),
    )
}

pub(crate) async fn bind(args: Value) -> Result<Value, &'static str> {
    let context: ManagedDeploymentContext =
        serde_json::from_value(args).map_err(|_| "management_context_invalid")?;
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    let store = Store::open()?;
    let mut state = store.load()?;
    state.bind(context)?;
    store.save(&state)?;
    Ok(serde_json::json!({"bound":true}))
}

pub(crate) async fn apply(args: Value, request_id: u64) -> Result<Value, &'static str> {
    if wincmd_shared::fleet::canonical_json_bytes(&args).len() > 65_536 {
        return Err("deployment_bundle_invalid");
    }
    let request: ManagedDeploymentRequest =
        serde_json::from_value(args).map_err(|_| "deployment_authority_invalid")?;
    // Retain the claim across the helper call; another pipe must not race a second install.
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "deployment_clock_invalid")?
        .as_secs() as i64;
    let (mut state, setup_id) = {
        let store = Store::open()?;
        let mut state = store.load()?;
        let setup_id = match state.authorize(&request, now)? {
            Decision::Observed(result) => return Ok(result),
            Decision::Apply { setup_id } => setup_id,
        };
        store.save(&state)?;
        (state, setup_id)
    };
    // The existing Windows broker owns thread-local native handles. Keep its
    // future on one blocking worker, as the other service broker callers do.
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
            crate::pro_broker::VaultCall {
                request_id,
                target_session_id: 0,
                caller_sid: "S-1-5-18",
                caller_token: None,
                caller_authentication_id: None,
                presentation: VaultPresentation::Machine,
                feature_id: "managed.deployment.apply",
                args: request.bundle,
            },
        ))
    })
    .await
    .map_err(|_| "installation_outcome_unknown")?
    .map_err(|_| "installation_outcome_unknown")?;
    let result = state.complete(&setup_id, result)?;
    Store::open()?.save(&state)?;
    Ok(result)
}
