//! Small authenticated client for the local `commander-svc` named pipe.
//!
//! This is deliberately a transport adapter only. Callers supply a fixed
//! service verb and bounded JSON payload; authorization remains with the
//! SYSTEM service, which derives the peer identity from the pipe client.

use serde_json::Value;

const SVC_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const VAULT_MOUNT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(130);

/// Call a service verb through the production pipe.
pub async fn call(feature_id: &str, args: Value) -> Result<Value, String> {
    call_via_with_timeout(wincmd_shared::svc::SVC_PIPE_NAME, feature_id, args, SVC_REQUEST_TIMEOUT).await
}

/// Vault mounting may take up to two minutes in the engine.  Keep its UI pipe
/// open long enough to receive the final bounded result; all ordinary service
/// RPCs retain the short five-second deadline.
pub async fn call_vault_mount(args: Value) -> Result<Value, String> {
    call_via_with_timeout(wincmd_shared::svc::SVC_PIPE_NAME, "svc.vault.mount", args, VAULT_MOUNT_TIMEOUT).await
}

/// Same as [`call`], with an injectable pipe solely for focused protocol tests.
///
/// A fresh connection token authenticates post-handshake frames. It is not an
/// authorization credential: the service must still authenticate the Windows
/// named-pipe peer and apply its own capability gate.
#[cfg(windows)]
pub async fn call_via(pipe_name: &str, feature_id: &str, args: Value) -> Result<Value, String> {
    call_via_with_timeout(pipe_name, feature_id, args, SVC_REQUEST_TIMEOUT).await
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
        request_timeout,
        wincmd_shared::write_envelope(&mut client, &hello),
    )
    .await
    .map_err(|_| "service Hello write timed out".to_string())?
    .map_err(|e| format!("service Hello write failed: {e}"))?;
    let ack = timeout(
        request_timeout,
        wincmd_shared::read_envelope(&mut client),
    )
    .await
    .map_err(|_| "service Hello acknowledgement timed out".to_string())?
    .map_err(|e| format!("service Hello acknowledgement failed: {e}"))?;
    if !matches!(ack, wincmd_shared::Envelope::Hello(_)) {
        return Err("service returned an invalid Hello acknowledgement".to_string());
    }

    let request = wincmd_shared::Envelope::Request(wincmd_shared::Request {
        request_id: 1,
        feature_id: feature_id.to_string(),
        args,
    })
    .sign(&session_token);
    timeout(
        request_timeout,
        wincmd_shared::write_envelope(&mut client, &request),
    )
    .await
    .map_err(|_| "service request write timed out".to_string())?
    .map_err(|e| format!("service request write failed: {e}"))?;
    let reply = timeout(
        request_timeout,
        wincmd_shared::read_envelope(&mut client),
    )
    .await
    .map_err(|_| "service reply timed out".to_string())?
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
        wincmd_shared::Envelope::Response(response) if response.request_id == 1 => {
            Ok(response.result)
        }
        wincmd_shared::Envelope::Response(_) => {
            Err("service reply did not match the request".to_string())
        }
        wincmd_shared::Envelope::Error(error) => {
            Err(format!("service rejected request: {}", error.kind))
        }
        _ => Err("service returned an unexpected reply".to_string()),
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
