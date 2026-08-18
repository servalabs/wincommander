// SPDX-License-Identifier: AGPL-3.0-or-later
//! `commander-svc` IPC client — `svc.clipboard.get_policy` (`ReadOnly`) and
//! `svc.clipboard.report_event` (`SessionHelper`), over the EXISTING
//! `\\.\pipe\wincmd-svc` framing (plan §1.4: "do NOT invent a third IPC
//! mechanism").
//!
//! Protocol, exactly as `wincmd_shared::svc`'s own module doc describes
//! it: dial [`wincmd_shared::svc::SVC_PIPE_NAME`], send `Envelope::Hello`
//! carrying `SVC_PROTOCOL_VERSION` and a fresh random session token, read
//! the Hello ack, then send `Envelope::Request` wrapped in
//! `Envelope::Signed` (HMAC-keyed by that same session token via
//! `wincmd_shared::Envelope::sign`).
//!
//! # A real, documented gap this client is written to tolerate
//!
//! As of this crate's writing, `commander-svc/src/pipe.rs::handle_connection`
//! does not yet unwrap `Envelope::Signed` frames (they fall through its
//! wildcard `_ => {}` arm and are silently ignored — that file is not in
//! this crate's file ownership; see this task's handoff note for the
//! concrete gap). So a stock build of `commander-svc` today will never
//! reply to the `Signed(Request)` this client sends per the documented
//! protocol above. To stay correct against BOTH today's stub server and
//! a future properly-signing one, without hanging:
//!
//! - every read is wrapped in `CALL_TIMEOUT`, so a non-responding peer
//!   degrades to [`SvcError::Unavailable`] rather than hanging forever;
//! - a reply is accepted whether it arrives as a plain `Envelope::Response`/
//!   `Envelope::Error` (today's stub behaviour) or wrapped in
//!   `Envelope::Signed` (the documented target behaviour) — see
//!   `unwrap_reply`.
//!
//! Every call retries with backoff and every failure path returns a typed
//! [`SvcError`] — this module never panics on the service being absent or
//! refusing (plan: "Handle the service being absent or refusing: retry
//! with backoff, degrade to a documented safe state, and never
//! crash-loop.").
//!
//! # Cfg layout
//!
//! Only [`SvcClient`]/[`SvcError`] (the public surface) and
//! [`SVC_PIPE_NAME`]-driven dispatch are compiled unconditionally, so the
//! rest of this crate can depend on this module cross-platform. Every
//! Win32-pipe-specific helper (`call_once`'s real body, `unwrap_reply`,
//! `random_session_token`) is `#[cfg(windows)]`-scoped, per this repo's
//! "Windows-only enforcement, non-Windows no-op stub" convention
//! (AGENTS.md) — this avoids `unused_imports`/`dead_code` warnings under
//! `-D warnings` on a non-Windows compile rather than merely hiding them.

use std::time::Duration;

use serde_json::Value;
use wincmd_shared::svc::SVC_PIPE_NAME;

use crate::policy::ClipboardPolicyResponse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvcError {
    /// Couldn't connect (pipe missing, no free server instance) or the
    /// peer never replied within the call timeout.
    Unavailable,
    /// The peer's Hello ack didn't carry the expected protocol version.
    ProtocolMismatch,
    /// The service explicitly refused the verb (`Envelope::Error`). Carries
    /// only the `error_kind` tag — never the free-text `message`, so a
    /// caller that logs `SvcError` can't accidentally leak a path/detail
    /// the service embedded in a human-readable string.
    Forbidden(String),
    /// The reply didn't parse into what was expected (malformed JSON,
    /// wrong envelope variant, or `verify_and_unwrap` failed).
    Malformed,
}

pub struct SvcClient {
    max_attempts: u32,
    backoff: Duration,
}

impl Default for SvcClient {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: Duration::from_millis(500),
        }
    }
}

impl SvcClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// `svc.clipboard.get_policy` — `ReadOnly` (plan §7 / GROUNDING §7).
    pub async fn get_policy(&self) -> Result<ClipboardPolicyResponse, SvcError> {
        let value = self
            .call(
                SVC_PIPE_NAME,
                "svc.clipboard.get_policy",
                serde_json::json!({}),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| SvcError::Malformed)
    }

    /// `svc.clipboard.report_event` — `SessionHelper` (plan §7 / GROUNDING
    /// §7). `report`'s fields are exactly the content-free set — see
    /// `report::build_report`.
    pub async fn report_event(
        &self,
        report: &wincmd_shared::fleet::ClipboardEventReport,
    ) -> Result<(), SvcError> {
        let args = serde_json::to_value(report).map_err(|_| SvcError::Malformed)?;
        self.call(SVC_PIPE_NAME, "svc.clipboard.report_event", args)
            .await?;
        Ok(())
    }

    async fn call(&self, pipe_name: &str, verb: &str, args: Value) -> Result<Value, SvcError> {
        let attempts = self.max_attempts.max(1);
        let mut last_err = SvcError::Unavailable;
        for i in 0..attempts {
            match call_once(pipe_name, verb, args.clone()).await {
                Ok(value) => return Ok(value),
                Err(err) => last_err = err,
            }
            if i + 1 < attempts {
                tokio::time::sleep(self.backoff * (i + 1)).await;
            }
        }
        Err(last_err)
    }
}

#[cfg(windows)]
mod win32 {
    //! The actual named-pipe round trip. Kept in its own inner module (as
    //! opposed to scattering `#[cfg(windows)]` across the outer module) so
    //! every Windows-only import lives beside the code that uses it —
    //! nothing here is reachable, and therefore nothing here is
    //! flagged unused, on a non-Windows compile.

    use std::time::Duration;

    use serde_json::Value;
    use wincmd_shared::svc::{hello_from_ui, SVC_PROTOCOL_VERSION};
    use wincmd_shared::{read_envelope, write_envelope, Envelope, Request};

    use super::SvcError;

    /// Bound on a single read/write within one call attempt.
    const CALL_TIMEOUT: Duration = Duration::from_secs(5);

    pub(super) async fn call_once(
        pipe_name: &str,
        verb: &str,
        args: Value,
    ) -> Result<Value, SvcError> {
        use tokio::net::windows::named_pipe::ClientOptions;

        let mut client = ClientOptions::new()
            .open(pipe_name)
            .map_err(|_| SvcError::Unavailable)?;

        let token = random_session_token();
        let hello = Envelope::Hello(hello_from_ui(token.clone()));
        timeout_io(write_envelope(&mut client, &hello)).await?;

        let ack = timeout_io(read_envelope(&mut client)).await?;
        match ack {
            Envelope::Hello(h) if h.protocol_version == SVC_PROTOCOL_VERSION => {}
            _ => return Err(SvcError::ProtocolMismatch),
        }

        let request = Envelope::Request(Request {
            request_id: 1,
            feature_id: verb.to_string(),
            args,
        });
        let signed = request.sign(&token);
        timeout_io(write_envelope(&mut client, &signed)).await?;

        let reply = timeout_io(read_envelope(&mut client)).await?;
        // Best-effort close — a failure here doesn't change whether we
        // already have a usable reply.
        let _ = write_envelope(&mut client, &Envelope::Bye).await;

        unwrap_reply(reply, &token)
    }

    async fn timeout_io<T>(
        fut: impl std::future::Future<Output = std::io::Result<T>>,
    ) -> Result<T, SvcError> {
        match tokio::time::timeout(CALL_TIMEOUT, fut).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err(SvcError::Unavailable),
            Err(_) => Err(SvcError::Unavailable),
        }
    }

    /// Accept a reply whether it's plain (today's `commander-svc` stub) or
    /// `Envelope::Signed` (the documented target protocol) — see module doc.
    fn unwrap_reply(reply: Envelope, token: &str) -> Result<Value, SvcError> {
        let reply = match reply {
            Envelope::Signed(_) => reply
                .verify_and_unwrap(token)
                .map_err(|_| SvcError::Malformed)?,
            other => other,
        };
        match reply {
            Envelope::Response(r) => Ok(r.result),
            Envelope::Error(e) => Err(SvcError::Forbidden(e.kind)),
            _ => Err(SvcError::Malformed),
        }
    }

    fn random_session_token() -> String {
        use rand::rngs::OsRng;
        use rand::RngCore;
        let mut buf = [0u8; 32];
        OsRng.fill_bytes(&mut buf);
        buf.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn unwrap_reply_returns_response_result() {
            let reply = Envelope::Response(wincmd_shared::Response {
                request_id: 1,
                result: serde_json::json!({"ok": true}),
            });
            let value = unwrap_reply(reply, "unused-token").expect("plain Response accepted");
            assert_eq!(value, serde_json::json!({"ok": true}));
        }

        #[test]
        fn unwrap_reply_maps_error_to_forbidden_with_kind_only() {
            let reply = Envelope::Error(wincmd_shared::ErrorReply {
                request_id: 1,
                kind: "forbidden".to_string(),
                message: "session-helper verb requires SYSTEM/Admin caller (path/detail that must never leak)"
                    .to_string(),
            });
            let err = unwrap_reply(reply, "unused-token").unwrap_err();
            assert_eq!(err, SvcError::Forbidden("forbidden".to_string()));
        }

        #[test]
        fn unwrap_reply_verifies_signed_replies() {
            let inner = Envelope::Response(wincmd_shared::Response {
                request_id: 1,
                result: serde_json::json!({"policy_version": 3}),
            });
            let signed = inner.sign("shared-token");
            let value =
                unwrap_reply(signed, "shared-token").expect("correctly signed reply accepted");
            assert_eq!(value, serde_json::json!({"policy_version": 3}));
        }

        #[test]
        fn unwrap_reply_rejects_signed_reply_with_wrong_token() {
            let inner = Envelope::Response(wincmd_shared::Response {
                request_id: 1,
                result: serde_json::json!({}),
            });
            let signed = inner.sign("real-token");
            let err = unwrap_reply(signed, "wrong-token").unwrap_err();
            assert_eq!(err, SvcError::Malformed);
        }

        /// Plan's test list: "Pipe unavailable degrades without panic."
        /// Runs the REAL Windows connect path against a pipe name that
        /// certainly does not exist, and asserts a typed error rather
        /// than a panic or a hang.
        #[tokio::test]
        async fn pipe_unavailable_returns_unavailable_without_panic() {
            let result = call_once(
                r"\\.\pipe\clipboard-guard-helper-test-nonexistent-pipe-9f3c",
                "svc.clipboard.get_policy",
                serde_json::json!({}),
            )
            .await;
            assert_eq!(result, Err(SvcError::Unavailable));
        }
    }
}

#[cfg(windows)]
use win32::call_once;

#[cfg(not(windows))]
async fn call_once(_pipe_name: &str, _verb: &str, _args: Value) -> Result<Value, SvcError> {
    // Windows-only enforcement, per AGENTS.md: the service and its pipe
    // are Windows-only; this stub exists solely so the workspace compiles
    // on non-Windows CI/dev.
    Err(SvcError::Unavailable)
}
