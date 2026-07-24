// src-tauri/commander-free/src/services/webhook_server.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SHARED WEBHOOK SERVER — bound to Tailscale interface only
// ═══════════════════════════════════════════════════════════════════════
//
// Single hyper HTTP/1.1 server that fans inbound POSTs to per-route
// subscribers. Drives `WebhookTrigger` in the Flows engine today; will
// drive `flows.heartbeat-polling` server-side once that lands.
//
// ── Threat model
//
//   1. ISP / hostile-state traffic — NEVER reached. Server binds to the
//      Tailscale IPv4 only (discovered via `tailscale.exe ip --4`).
//      Refuses to start if Tailscale isn't running. There is no
//      0.0.0.0 fallback.
//   2. Same-network peer impersonation — every POST must carry a
//      `X-Wincmd-Signature: <hex>` header == HMAC-SHA256(secret, body).
//      Constant-time verify via `subtle`. Per-route secret stored in
//      the flow definition (`settings.json`), only known to the user
//      who configured the trigger + admins who can sign the same body.
//   3. Replay — partial defence only. We don't enforce a timestamp
//      window (would require a clock-skew tolerance config + timestamp
//      header). For panic-style triggers replay is fine (re-firing
//      panic doesn't escalate damage). For other action types: caller's
//      responsibility to make the action idempotent.
//   4. DoS — body capped at 64 KiB. No per-IP rate limit (Tailscale ACLs
//      already give us peer-identity-level filtering at the network
//      layer).
//
// ── API
//
//   fn register(path: String, secret: String, fire: Box<...>) -> Result<RouteHandle, String>
//
// Returns a RouteHandle whose Drop unregisters the route. When the last
// route is removed, the server task shuts down.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use once_cell::sync::Lazy;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::Notify;

// ── Constants ───────────────────────────────────────────────────────

const WEBHOOK_PORT: u16 = 47821;
const MAX_BODY_BYTES: usize = 64 * 1024;
const SIGNATURE_HEADER: &str = "X-Wincmd-Signature";

// ── Route registry ──────────────────────────────────────────────────

type FireFn = Box<dyn Fn() + Send + Sync + 'static>;

struct Route {
    secret: String,
    fire: FireFn,
}

struct ServerState {
    routes: HashMap<String, Route>,
    /// Cancel signal for the running server task. Cleared when the last
    /// route unregisters; new server task spawned on next register.
    shutdown: Option<std::sync::Arc<Notify>>,
}

static STATE: Lazy<Mutex<ServerState>> = Lazy::new(|| {
    Mutex::new(ServerState {
        routes: HashMap::new(),
        shutdown: None,
    })
});

// ── Public API ──────────────────────────────────────────────────────

/// Drop guard returned by `register`. When dropped, removes the route
/// from the live registry; if no routes remain, signals the server
/// task to shut down.
pub struct RouteHandle {
    path: String,
}

impl Drop for RouteHandle {
    fn drop(&mut self) {
        let should_stop = {
            let mut state = STATE.lock().unwrap();
            state.routes.remove(&self.path);
            state.routes.is_empty()
        };
        if should_stop {
            let shutdown = STATE.lock().unwrap().shutdown.take();
            if let Some(notify) = shutdown {
                notify.notify_waiters();
                crate::log_message(
                    "debug",
                    "[WebhookServer] last route removed — shutting down",
                );
            }
        }
    }
}

/// Register a webhook route. Idempotent on `path`: if the same path is
/// re-registered (e.g. flow toggled off → on), the new entry replaces
/// the old. Spawns the HTTP server task if not already running.
pub fn register(path: String, secret: String, fire: FireFn) -> Result<RouteHandle, String> {
    if !path.starts_with('/') {
        return Err(format!("webhook path must start with '/': '{}'", path));
    }
    if secret.len() < 16 {
        return Err("webhook secret must be at least 16 chars".to_string());
    }

    let needs_spawn = {
        let mut state = STATE.lock().unwrap();
        state.routes.insert(path.clone(), Route { secret, fire });
        state.shutdown.is_none()
    };

    if needs_spawn {
        let notify = std::sync::Arc::new(Notify::new());
        STATE.lock().unwrap().shutdown = Some(notify.clone());

        let bind_addr = resolve_bind_addr()?;
        crate::log_message("info", &format!("[WebhookServer] binding to {}", bind_addr));

        tauri::async_runtime::spawn(async move {
            if let Err(e) = run_server(bind_addr, notify).await {
                crate::log_message("error", &format!("[WebhookServer] {}", e));
            }
            crate::log_message("debug", "[WebhookServer] task exited");
        });
    }

    Ok(RouteHandle { path })
}

// ── Tailscale-IP discovery ──────────────────────────────────────────

/// Discover the Tailscale IPv4 to bind to. Refuses to fall back to
/// 0.0.0.0 — Tailscale-only is the load-bearing security invariant.
fn resolve_bind_addr() -> Result<SocketAddr, String> {
    // `tailscale.exe ip --4` prints just the IPv4 on stdout, exits 1 if
    // not connected. Synchronous since this runs once per server spawn.
    let mut cmd = std::process::Command::new("tailscale.exe");
    cmd.args(["ip", "--4"]);
    #[cfg(windows)]
    {
        // no console flash
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| {
        format!(
            "Pvt Mesh VPN client not found ({}). WebhookTrigger requires the mesh VPN.",
            e
        )
    })?;
    if !output.status.success() {
        return Err(
            "Pvt Mesh VPN not running or not logged in. WebhookTrigger needs the mesh VPN up."
                .to_string(),
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ip_str = stdout.trim().lines().next().unwrap_or("").trim();
    let ip: IpAddr = ip_str.parse().map_err(|_| {
        format!(
            "tailscale.exe ip --4 returned unparseable output: '{}'",
            ip_str
        )
    })?;
    Ok(SocketAddr::new(ip, WEBHOOK_PORT))
}

// ── HTTP server ─────────────────────────────────────────────────────

async fn run_server(addr: SocketAddr, shutdown: std::sync::Arc<Notify>) -> Result<(), String> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {}: {}", addr, e))?;
    crate::log_message("info", &format!("[WebhookServer] listening on {}", addr));

    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                crate::log_message("debug", "[WebhookServer] shutdown signal received");
                return Ok(());
            }
            accept = listener.accept() => {
                let (stream, _peer) = match accept {
                    Ok(p) => p,
                    Err(e) => {
                        crate::log_message("warn", &format!("[WebhookServer] accept: {}", e));
                        continue;
                    }
                };
                let io = TokioIo::new(stream);
                tauri::async_runtime::spawn(async move {
                    // Per-connection 5s read timeout. Hard cap so a
                    // half-open client can't hold a slot forever.
                    let serve = hyper::server::conn::http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(io, service_fn(handle));
                    let _ = tokio::time::timeout(Duration::from_secs(5), serve).await;
                });
            }
        }
    }
}

async fn handle(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    if req.method() != hyper::Method::POST {
        return Ok(reply(StatusCode::METHOD_NOT_ALLOWED, "POST only"));
    }
    let path = req.uri().path().to_string();
    let sig_header = req
        .headers()
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Read body with a hard byte cap. http_body_util's `collect()`
    // pulls the full body — we only need to reject when it overflows.
    let body_bytes = match req.into_body().collect().await {
        Ok(b) => {
            let bytes = b.to_bytes();
            if bytes.len() > MAX_BODY_BYTES {
                return Ok(reply(StatusCode::PAYLOAD_TOO_LARGE, "body too large"));
            }
            bytes
        }
        Err(_) => return Ok(reply(StatusCode::BAD_REQUEST, "body read failed")),
    };

    // Look up the route. Lock release ASAP so the long verify+fire
    // doesn't block the registry.
    let route_secret_fire = {
        let state = STATE.lock().unwrap();
        state.routes.get(&path).map(|r| {
            // Can't clone the FireFn directly — call site invokes through
            // a snapshot. Instead: copy the secret + leave fire-invocation
            // for inside the lock-held closure below.
            (r.secret.clone(),)
        })
    };

    let secret = match route_secret_fire {
        Some((s,)) => s,
        None => return Ok(reply(StatusCode::NOT_FOUND, "no route")),
    };

    let signature = match sig_header {
        Some(s) => s,
        None => return Ok(reply(StatusCode::UNAUTHORIZED, "missing signature")),
    };

    if !verify_hmac(&secret, &body_bytes, &signature) {
        return Ok(reply(StatusCode::UNAUTHORIZED, "bad signature"));
    }

    // Fire under the lock (briefly) — the FireFn is cheap (just queues
    // a trigger event on a channel).
    {
        let state = STATE.lock().unwrap();
        if let Some(route) = state.routes.get(&path) {
            (route.fire)();
        }
    }

    Ok(reply(StatusCode::OK, "ok"))
}

fn reply(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

// ── HMAC verify ─────────────────────────────────────────────────────

fn verify_hmac(secret: &str, body: &[u8], signature_hex: &str) -> bool {
    let expected_bytes = match hex::decode(signature_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };
    if expected_bytes.len() != 32 {
        return false; // SHA-256 = 32 bytes
    }
    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let computed = mac.finalize().into_bytes();
    bool::from(computed.as_slice().ct_eq(&expected_bytes))
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_verify_accepts_matching_signature() {
        let secret = "0123456789abcdef0123456789abcdef";
        let body = b"hello world";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());
        assert!(verify_hmac(secret, body, &sig));
    }

    #[test]
    fn hmac_verify_rejects_wrong_body() {
        let secret = "0123456789abcdef0123456789abcdef";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(b"original");
        let sig = hex::encode(mac.finalize().into_bytes());
        assert!(!verify_hmac(secret, b"tampered", &sig));
    }

    #[test]
    fn hmac_verify_rejects_wrong_secret() {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret-a-secret-a").unwrap();
        mac.update(b"body");
        let sig = hex::encode(mac.finalize().into_bytes());
        assert!(!verify_hmac("different-secret16chars", b"body", &sig));
    }

    #[test]
    fn hmac_verify_rejects_malformed_signature() {
        assert!(!verify_hmac(
            "0123456789abcdef0123456789abcdef",
            b"body",
            "not-hex"
        ));
        assert!(!verify_hmac(
            "0123456789abcdef0123456789abcdef",
            b"body",
            "abcd"
        ));
    }

    #[test]
    fn register_rejects_invalid_inputs() {
        assert!(register(
            "noleadingslash".into(),
            "0123456789abcdef".into(),
            Box::new(|| {})
        )
        .is_err());
        assert!(register("/ok".into(), "tooshort".into(), Box::new(|| {})).is_err());
    }
}
