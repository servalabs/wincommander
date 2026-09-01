//! Small authenticated client for the local `commander-svc` named pipe.
//!
//! This is deliberately a transport adapter only. Callers supply a fixed
//! service verb and bounded JSON payload; authorization remains with the
//! SYSTEM service, which derives the peer identity from the pipe client.

use serde_json::Value;
#[cfg(any(windows, test))]
use std::sync::atomic::{AtomicU64, Ordering};
use zeroize::Zeroize;

const SVC_TRANSPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const VAULT_MOUNT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(130);
const VAULT_MUTATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
#[cfg(any(windows, test))]
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn request_timeout_for(feature_id: &str) -> std::time::Duration {
    match feature_id {
        // Applying a policy first dismounts active vaults and may resolve
        // domain principals. A five-second caller deadline falsely reported a
        // failure while the service continued the exclusive operation.
        "svc.vault.apply_policy" | "svc.vault.unmount" => VAULT_MUTATION_TIMEOUT,
        "svc.vault.mount" => VAULT_MOUNT_TIMEOUT,
        _ => SVC_TRANSPORT_TIMEOUT,
    }
}

#[cfg(any(windows, test))]
fn may_still_be_completing(feature_id: &str) -> bool {
    matches!(
        feature_id,
        "svc.vault.mount" | "svc.vault.apply_policy" | "svc.vault.unmount"
    )
}

#[cfg(any(windows, test))]
fn next_request_id() -> u64 {
    // A pipe currently carries one request at a time, yet an ID unique across
    // connections keeps a delayed response from ever looking like a retry if
    // the client gains connection reuse later.
    NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

/// Call a service verb through the production pipe.
pub async fn call(feature_id: &str, args: Value) -> Result<Value, String> {
    call_via_with_timeout(
        wincmd_shared::svc::SVC_PIPE_NAME,
        feature_id,
        args,
        request_timeout_for(feature_id),
    )
    .await
}

/// Vault mounting may take up to two minutes in the engine.  Keep its UI pipe
/// open long enough to receive the final bounded result; read-only service
/// RPCs retain the short five-second deadline.
pub async fn call_vault_mount(args: Value) -> Result<Value, String> {
    call_via_with_timeout(
        wincmd_shared::svc::SVC_PIPE_NAME,
        "svc.vault.mount",
        args,
        request_timeout_for("svc.vault.mount"),
    )
    .await
}

/// Same as [`call`], with an injectable pipe solely for focused protocol tests.
///
/// A fresh connection token authenticates post-handshake frames. It is not an
/// authorization credential: the service must still authenticate the Windows
/// named-pipe peer and apply its own capability gate.
#[cfg(windows)]
pub async fn call_via(pipe_name: &str, feature_id: &str, args: Value) -> Result<Value, String> {
    call_via_with_timeout(pipe_name, feature_id, args, request_timeout_for(feature_id)).await
}

#[cfg(windows)]
async fn call_via_with_timeout(
    pipe_name: &str,
    feature_id: &str,
    args: Value,
    request_timeout: std::time::Duration,
) -> Result<Value, String> {
    use tokio::net::windows::named_pipe::{ClientOptions, PipeMode};
    use tokio::time::timeout;
    use uuid::Uuid;

    let mut client = ClientOptions::new()
        .pipe_mode(PipeMode::Byte)
        .open(pipe_name)
        .map_err(|e| format!("service connect failed: {e}"))?;
    let session_token = Uuid::new_v4().to_string();
    let hello = wincmd_shared::Envelope::Hello(wincmd_shared::svc::hello_from_ui(&session_token));

    timeout(
        SVC_TRANSPORT_TIMEOUT,
        wincmd_shared::write_envelope(&mut client, &hello),
    )
    .await
    .map_err(|_| "service Hello write timed out".to_string())?
    .map_err(|e| format!("service Hello write failed: {e}"))?;
    let ack = timeout(
        SVC_TRANSPORT_TIMEOUT,
        wincmd_shared::read_envelope(&mut client),
    )
    .await
    .map_err(|_| "service Hello acknowledgement timed out".to_string())?
    .map_err(|e| format!("service Hello acknowledgement failed: {e}"))?;
    if !matches!(ack, wincmd_shared::Envelope::Hello(_)) {
        return Err("service returned an invalid Hello acknowledgement".to_string());
    }

    let request_id = next_request_id();
    let mut request = wincmd_shared::Envelope::Request(wincmd_shared::Request {
        request_id,
        feature_id: feature_id.to_string(),
        args,
    })
    .sign(&session_token);
    let write_result = timeout(
        SVC_TRANSPORT_TIMEOUT,
        wincmd_shared::write_envelope(&mut client, &request),
    )
    .await;
    // `SignedEnvelope::inner` is a second serialized copy of the password.
    // Clear it immediately after the pipe write, including timeout/error.
    zeroize_envelope(&mut request);
    write_result
        .map_err(|_| "service request write timed out".to_string())?
        .map_err(|e| format!("service request write failed: {e}"))?;
    let reply = timeout(request_timeout, wincmd_shared::read_envelope(&mut client))
        .await
        .map_err(|_| {
            if may_still_be_completing(feature_id) {
                "service operation did not confirm before its deadline; refresh vault state before retrying"
                    .to_string()
            } else {
                "service reply timed out".to_string()
            }
        })?
        .map_err(|e| format!("service reply read failed: {e}"))?;
    let reply = if matches!(reply, wincmd_shared::Envelope::Signed(_)) {
        reply
            .verify_and_unwrap(&session_token)
            .map_err(|e| format!("service reply signature check failed: {e}"))?
    } else {
        reply
    };
    let _ = wincmd_shared::write_envelope(&mut client, &wincmd_shared::Envelope::Bye).await;

    match reply {
        wincmd_shared::Envelope::Response(response) if response.request_id == request_id => {
            Ok(response.result)
        }
        wincmd_shared::Envelope::Response(_) => {
            Err("service reply did not match the request".to_string())
        }
        wincmd_shared::Envelope::Error(error) => {
            // `error.message` already respects the service's privacy boundary
            // (admin-typed principal names only — never a SID, path, or ACL
            // detail; see `vault_error_message`'s doc comment), so it is safe
            // to surface here. `error.kind` stays in the string too so
            // substring checks like `describeReconcileFailure`'s `"forbidden"`
            // match keep working unchanged.
            Err(format!(
                "service rejected request: {} ({})",
                error.kind, error.message
            ))
        }
        _ => Err("service returned an unexpected reply".to_string()),
    }
}

fn zeroize_envelope(envelope: &mut wincmd_shared::Envelope) {
    match envelope {
        wincmd_shared::Envelope::Request(request) => zeroize_json(&mut request.args),
        wincmd_shared::Envelope::Signed(signed) => {
            signed.inner.zeroize();
            signed.tag.zeroize();
        }
        _ => {}
    }
}

fn zeroize_json(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => {
            for value in values {
                zeroize_json(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                zeroize_json(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_signed_mount_payload_after_pipe_write() {
        let mut envelope = wincmd_shared::Envelope::Request(wincmd_shared::Request {
            request_id: 1,
            feature_id: "svc.vault.mount".into(),
            args: serde_json::json!({"entry_id":"shared","password":"canary-secret"}),
        })
        .sign("session-token");

        zeroize_envelope(&mut envelope);
        let wincmd_shared::Envelope::Signed(signed) = envelope else {
            panic!("request should remain a signed envelope");
        };
        assert!(signed.inner.is_empty());
        assert!(signed.tag.is_empty());
    }

    #[test]
    fn long_deadlines_are_reserved_for_vault_mutations() {
        assert_eq!(request_timeout_for("svc.vault.mount"), VAULT_MOUNT_TIMEOUT);
        assert_eq!(
            request_timeout_for("svc.vault.apply_policy"),
            VAULT_MUTATION_TIMEOUT
        );
        assert_eq!(
            request_timeout_for("svc.vault.unmount"),
            VAULT_MUTATION_TIMEOUT
        );
        assert_eq!(
            request_timeout_for("svc.vault.get_policy"),
            SVC_TRANSPORT_TIMEOUT
        );
    }

    #[test]
    fn service_calls_do_not_reuse_request_correlation_ids() {
        assert_ne!(next_request_id(), next_request_id());
    }

    #[test]
    fn only_mutation_timeouts_are_described_as_unknown_outcomes() {
        assert!(may_still_be_completing("svc.vault.apply_policy"));
        assert!(may_still_be_completing("svc.vault.unmount"));
        assert!(may_still_be_completing("svc.vault.mount"));
        assert!(!may_still_be_completing("svc.vault.get_status"));
    }
}

/// The enforcement service is intentionally Windows-only. A non-Windows build
/// must not pretend it queried or applied a vault policy.
#[cfg(not(windows))]
pub async fn call_via(_pipe_name: &str, _feature_id: &str, _args: Value) -> Result<Value, String> {
    Err("Vault access service is available only on Windows".to_string())
}

#[cfg(not(windows))]
async fn call_via_with_timeout(
    _pipe_name: &str,
    _feature_id: &str,
    _args: Value,
    _request_timeout: std::time::Duration,
) -> Result<Value, String> {
    Err("Vault access service is available only on Windows".to_string())
}
