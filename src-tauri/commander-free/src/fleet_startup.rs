// SPDX-License-Identifier: AGPL-3.0-or-later
//! Background resume reads the current configuration while holding the Fleet lease.
use std::future::Future;

async fn resume_locked<R, D, F>(read: R, dispatch: D) -> Result<(), String>
where
    R: FnOnce() -> Result<Option<serde_json::Value>, String>,
    D: FnOnce(serde_json::Value) -> F,
    F: Future<Output = Result<(), String>>,
{
    if let Some(args) = read()? {
        dispatch(args).await?;
    }
    Ok(())
}

fn resume_args(
    primary: String,
    dispatch: bool,
    pin: String,
    device: String,
    mesh: Option<&str>,
) -> serde_json::Value {
    let origins = super::enrollment_origins(&primary, mesh);
    serde_json::json!({
        "serverUrl": primary,
        "dispatch": dispatch,
        "signingKeyPub": pin,
        "deviceId": device,
        "enrollmentIntent": "resume",
        "meshEndpoints": origins.into_iter().skip(1).collect::<Vec<_>>(),
    })
}

pub(crate) async fn resume_from_settings() -> Result<(), String> {
    super::lifecycle_lease::run(async move {
        resume_locked(
            || {
                crate::settings::invalidate_cache();
                let current = crate::settings::read_settings()?;
                if !current.app.fleet.enabled || current.app.fleet.server_url.is_empty() {
                    return Ok(None);
                }
                let mesh = std::env::var("FLEET_MESH_ENDPOINTS").ok();
                Ok(Some(resume_args(
                    current.app.fleet.server_url,
                    current.app.fleet.dispatch,
                    current.app.fleet.signing_key_pub,
                    current.device_id,
                    mesh.as_deref(),
                )))
            },
            |args| async move {
                crate::sidecar::dispatch_paid_command("fleet_agent_configure", args).await?;
                Ok(())
            },
        )
        .await
    })
    .await
}

#[cfg(test)]
#[path = "fleet_startup_tests.rs"]
mod tests;
