// SPDX-License-Identifier: AGPL-3.0-or-later
// commander-shared — IPC types between WinCommander Free and Pro binaries
// ═══════════════════════════════════════════════════════════════════════
//
// Phase 7 of the tier-split rollout will use these types as the wire
// format for the named-pipe IPC channel between commander-free
// (`wincommander-free.exe`, the open-source primary) and commander-pro
// (`wincommander-pro.exe`, the paid sidecar).
//
// Phase 6 ships the type definitions only — phase 7 wires the actual
// pipe transport. Both crates already link to this one so the wire
// format is committed and visible without a flag day later.
//
// Wire envelope shape (length-prefixed JSON-RPC 2.0):
//
//   ┌──────────────┬──────────────────────────────────────────┐
//   │ u32 LE       │ JSON payload (UTF-8, no trailing newline) │
//   │ payload size │                                          │
//   └──────────────┴──────────────────────────────────────────┘
//
// All payloads are one of {Hello, Request, Response, Error, Notification, Bye}.

use serde::{Deserialize, Serialize};

/// Magic string in the Hello frame so a connecting peer can spot a
/// protocol-mismatch immediately. Bumped on incompatible wire-format
/// changes; minor tweaks to a request body don't touch this.
pub const PROTOCOL_VERSION: &str = "wincmd-ipc-v1";

/// The auto-erase PowerShell module — single source of truth for the
/// per-card auto-erase scheduler used by the Privacy Clean panel.
/// Embedded by both commander-free (loaded alongside privacy/cleanup
/// at runtime) and commander-pro (prepended to the inline command sent
/// to powershell.exe). Defines:
///   - `Set-AutoEraseSchedule -CategoryId <s> -IntervalMinutes <n> -RunAsSystem <bool>`
///   - `Remove-AutoEraseSchedule -CategoryId <s>`
///   - `Get-AutoEraseSchedules`
///   - `Get-AutoEraseSupportedCategories`
///   - `Invoke-AutoEraseMigration`
///
/// Adding a new schedulable privacy-clean category = one edit to this file.
pub const AUTO_ERASE_PS_MODULE: &str = include_str!("../scripts/auto-erase.ps1");

/// The app-capability access PowerShell module — single source of truth for
/// remotely toggling a Windows app-capability (camera / microphone / location)
/// to Allow or Deny. Embedded by commander-pro and invoked by the fleet command
/// executor (`handlers::dispatch` → "Set-AppCapabilityAccess"). Defines:
///   - `Set-AppCapabilityAccess -Capability <s> -Access <Allow|Deny>`
///   - `Get-AppCapabilityAccessStatus -Capability <s>`
///
/// Mirrors the four-layer enforcement Free already applies in telemetry.ps1.
/// AV-clean: no token here appears in tools/strings-grep-forbidden.txt, so it is
/// safe to embed in the Free binary (wincmd-shared links into both).
pub const CAPABILITY_ACCESS_PS_MODULE: &str = include_str!("../scripts/capability-access.ps1");

/// The unattended-session-guard PowerShell module — runs as SYSTEM on a
/// scheduled task (registered by commander-free's `attend_watch.rs`) and
/// dismounts local VeraCrypt vaults once no interactive session is attending.
/// Standalone (no module-bundle deps) because it executes in session 0,
/// independent of the GUI process. Parameters:
///   - `-IdleThresholdSeconds <n>` active-idle cutoff for "attending"
///   - `-SettleSeconds <n>` unattended dwell before dismount
///   - `-DismountVaults` / `-SignOffStale` action switches
pub const ATTEND_WATCH_PS_MODULE: &str = include_str!("../scripts/attend-watch.ps1");

/// Top-level envelope. The first byte of the JSON tells us which variant
/// it is — serde renames the tag to `kind` for legibility on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    /// First frame after pipe connect — Free → Pro on spawn, then
    /// Pro → Free as the ack. The ack carries the Pro binary's
    /// SHA-256 hash so Free can verify it matches the value pinned
    /// at install time (defence against a swapped sidecar).
    Hello(Hello),

    /// Free invoking a paid feature on Pro.
    Request(Request),

    /// Pro returning a result. Matches by request_id.
    Response(Response),

    /// Pro returning a structured error (vs a Tauri-style result string).
    Error(ErrorReply),

    /// Pro → Free: unsolicited event emit. Used by paid background
    /// features (voice-lockdown, decoy-accessed, dead-man's-switch tick,
    /// honeypot hits, wifi-guard detections) that need to push state to
    /// the UI without a matching Request. Free's per-session reader
    /// task receives these and re-emits via `app.emit(event, payload)`
    /// so existing frontend listeners keep working unchanged after the
    /// paid Rust modules move out of commander-free into commander-pro.
    ///
    /// Integrity: Pro wraps each Notification in `Envelope::Signed`
    /// before write, exactly like Response/Error. The session token is
    /// not duplicated inside the Notification body because the signing
    /// layer already proves the sender knows the token (matches the
    /// Request/Response/ErrorReply convention).
    Notification(Notification),

    /// Either side announcing imminent disconnect — graceful close.
    Bye,

    /// Phase 9b — wrap any other variant with an HMAC tag keyed by the
    /// per-spawn session token. Senders: call `Envelope::sign(token)`
    /// after handshake to wrap. Receivers: call `verify_and_unwrap(token)`
    /// to validate the tag and recover the inner envelope. Replay /
    /// injection from outside the active session can't forge a valid tag.
    Signed(SignedEnvelope),
}

/// Wrapper variant carrying the HMAC tag + the inner envelope AS ITS EXACT
/// ON-WIRE BYTES (`RawValue`), not a re-parsed struct. This is the crux of the
/// integrity guarantee: the signer HMACs these exact bytes and they travel
/// verbatim, so the verifier checks the tag against byte-identical input rather
/// than a re-serialization. Re-serializing (the old `Box<Envelope>` design)
/// silently dropped any frame whose bytes the two independently-built binaries
/// couldn't reproduce identically — e.g. a settings-snapshot float that Free's
/// and Pro's differing `ryu` versions formatted differently — which is exactly
/// how large Flow-Sync-Rules / Flow-Ingest-Event frames hung Save for 120s.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct SignedEnvelope {
    /// HMAC-SHA256 hex tag over `inner.get().as_bytes()` — the verbatim
    /// serialized inner envelope, exactly as it appears on the wire.
    pub tag: String,
    /// The wrapped envelope (Request / Response / Error / Notification in
    /// practice; Hello / Bye are never signed) carried as its serialized JSON
    /// TEXT, not a re-parsed struct. Storing it as a string means the exact
    /// bytes that were signed travel on the wire and are verified verbatim —
    /// a JSON string round-trips byte-for-byte, so no re-encoding (and thus no
    /// float-formatter/serde-version skew between Free and Pro) can occur. A
    /// `RawValue` would be cleaner but serde can't deserialize one inside this
    /// internally-tagged (`tag = "kind"`) enum — it buffers, which RawValue
    /// rejects. `from_str(&inner)` reconstitutes the typed Envelope on demand.
    pub inner: String,
}

impl Envelope {
    /// Wrap `self` in a `Signed` variant with an HMAC tag computed from
    /// `session_token`. No-op if `self` is already `Signed` — guards
    /// against accidental double-signing during refactors.
    pub fn sign(self, session_token: &str) -> Envelope {
        if matches!(self, Envelope::Signed(_)) {
            return self;
        }
        // Serialize the inner envelope ONCE. The resulting bytes are both what
        // we HMAC and — carried verbatim as a RawValue — what travels on the
        // wire, so the verifier can check the tag against the identical bytes
        // without re-serializing (which would depend on its serde_json / ryu
        // matching ours byte-for-byte).
        let inner_json = serde_json::to_string(&self)
            .expect("Envelope serialises with serde_json infallibly for our types");
        let tag = sign_body(session_token, inner_json.as_bytes());
        Envelope::Signed(SignedEnvelope {
            tag,
            inner: inner_json,
        })
    }

    /// Verify the HMAC tag and return the inner envelope. Returns Err
    /// if `self` isn't a `Signed` variant (caller chose to skip signing
    /// for a pre-handshake frame and shouldn't be calling verify) or
    /// if the tag doesn't match.
    pub fn verify_and_unwrap(self, session_token: &str) -> Result<Envelope, &'static str> {
        match self {
            Envelope::Signed(SignedEnvelope { tag, inner }) => {
                // Verify against the EXACT bytes received on the wire (the inner
                // JSON text, round-tripped byte-for-byte as a string) — never a
                // re-serialization. Only parse the inner into a typed Envelope
                // AFTER the tag checks out.
                if verify_body(session_token, inner.as_bytes(), &tag) {
                    serde_json::from_str::<Envelope>(&inner)
                        .map_err(|_| "inner envelope deserialisation failed")
                } else {
                    Err("HMAC tag mismatch — frame may have been tampered with or replayed")
                }
            }
            _ => Err("envelope was not signed"),
        }
    }
}

/// Initial handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct Hello {
    pub protocol_version: String,
    /// Per-spawn random session token. Free generates it before
    /// spawning Pro; Pro echoes it in its Hello ack so Free knows
    /// the right process answered. Phase 9 hardening adds signing.
    pub session_token: String,
    /// SHA-256 hex of the Pro binary, only present in the Pro→Free
    /// ack. None in the Free→Pro initial frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_hash: Option<String>,
    /// Free version string — useful for compatibility tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_version: Option<String>,
    /// Pro version string in the ack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pro_version: Option<String>,
}

/// Free → Pro: invoke a paid feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct Request {
    /// Sequential id within a session — Pro's Response/Error must match.
    pub request_id: u64,
    /// Feature key, e.g. "vault.create-volume", "lockdown",
    /// "Disable-WindowsDefender". Matches the keys in
    /// commander-free's get_command_tier() that resolve to "paid".
    pub feature_id: String,
    /// Arbitrary JSON args — Pro deserializes per-feature.
    pub args: serde_json::Value,
}

/// Pro → Free: success result. Body is the feature's return value as
/// arbitrary JSON (matches the existing `serde_json::Value` shape that
/// run_backend_script returned).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct Response {
    pub request_id: u64,
    pub result: serde_json::Value,
}

/// Pro → Free: unsolicited UI event. The reader task on Free side
/// translates this into a Tauri `app.emit(event, payload)` so the
/// existing frontend listeners (`decoy-accessed`, `voice-lockdown-triggered`,
/// `honeypot-hit`, `dead-man-switch-tick`, etc.) see no change in
/// behaviour after the paid Rust modules move to commander-pro.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct Notification {
    /// Tauri event name — what `app.emit()` will dispatch under. Must
    /// match what frontend hooks subscribe to via
    /// `listen("<event>", ...)`. Keep the existing event names stable
    /// across the move so the frontend doesn't need to be touched.
    pub event: String,
    /// Arbitrary JSON payload — frontend's listener deserializes it.
    pub payload: serde_json::Value,
}

/// Pro → Free: structured error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct ErrorReply {
    pub request_id: u64,
    /// Short kind tag — frontend can branch on this.
    /// Examples: "missing_entitlement", "feature_unknown",
    /// "feature_failed", "panic".
    ///
    /// Wire-renamed to `error_kind` because the parent `Envelope` enum
    /// uses `#[serde(tag = "kind")]` for variant discrimination — leaving
    /// this field as `kind` produced JSON with two `kind` fields and
    /// blew up on deserialise with "duplicate field `kind`". The Rust
    /// field name stays as `kind` for ergonomic call-site access.
    #[serde(rename = "error_kind")]
    pub kind: String,
    /// Human-readable message. Safe to surface in a toast.
    pub message: String,
}

/// Optional helper for the Free side: build the Hello it sends to Pro
/// at spawn time.
pub fn hello_from_free(session_token: impl Into<String>) -> Hello {
    Hello {
        protocol_version: PROTOCOL_VERSION.to_string(),
        session_token: session_token.into(),
        binary_hash: None,
        free_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        pro_version: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Framing — length-prefixed JSON over an async byte stream
// ═══════════════════════════════════════════════════════════════════════
//
// Wire layout per frame:
//   ┌──────────────────────┬──────────────────────────────────┐
//   │ u32 LE — payload len │ JSON UTF-8 bytes (no terminator) │
//   └──────────────────────┴──────────────────────────────────┘
//
// A few intentional choices:
//   - u32 LE keeps each frame ≤ 4 GiB, plenty for any IPC payload.
//   - No magic / version prefix: the Hello envelope carries
//     `protocol_version` and is the first frame both sides exchange,
//     so a mismatch surfaces in the application layer rather than
//     conflating with framing errors.
//   - MAX_PAYLOAD_BYTES bounds frames to 16 MiB so a malicious /
//     wedged peer can't allocate unbounded memory on us.
//
// These helpers are generic over any AsyncRead/AsyncWrite — both the
// Tokio Windows named-pipe halves and the in-memory test pair from
// `tokio::io::duplex` satisfy them.

use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Hard cap on a single inbound frame's payload (16 MiB). Larger frames
/// are refused with InvalidData. Tunable per-deployment if a future
/// feature needs to exchange bigger blobs (file transfer over IPC?),
/// though most flows stay under a few KiB.
pub const MAX_PAYLOAD_BYTES: u32 = 16 * 1024 * 1024;

/// Read one length-prefixed JSON frame and decode it as an Envelope.
/// Returns Err on EOF mid-frame, malformed length, oversized frame, or
/// invalid JSON. Pairs with [`write_envelope`].
pub async fn read_envelope<R>(reader: &mut R) -> io::Result<Envelope>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let len = reader.read_u32_le().await?;
    if len > MAX_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "IPC frame payload {} bytes exceeds cap of {} bytes",
                len, MAX_PAYLOAD_BYTES
            ),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice::<Envelope>(&buf).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("IPC frame JSON decode failed: {}", e),
        )
    })
}

/// Encode an Envelope as JSON and write it as one length-prefixed frame.
/// Pairs with [`read_envelope`]. Caller is responsible for `flush()` if
/// they need a write barrier; the helper only writes.
pub async fn write_envelope<W>(writer: &mut W, env: &Envelope) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(env).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("IPC envelope JSON encode failed: {}", e),
        )
    })?;
    let len = u32::try_from(body.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC envelope larger than u32::MAX (impossible in practice)",
        )
    })?;
    if len > MAX_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "IPC envelope {} bytes exceeds cap of {} bytes",
                len, MAX_PAYLOAD_BYTES
            ),
        ));
    }
    writer.write_u32_le(len).await?;
    writer.write_all(&body).await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 9 — HMAC integrity for request bodies
// ═══════════════════════════════════════════════════════════════════════
//
// Free and Pro both have the per-spawn session token (Free generates
// it; Pro echoes it back in the Hello ack). Phase 9 uses it as an
// HMAC-SHA256 key over every Request / Response body so a patcher
// outside the session can't inject frames or replay captured ones.
//
// Wire layer doesn't change yet — `sign_body` / `verify_body` are
// pure helpers. Phase 9b wraps them around the existing Envelope by
// introducing a `Signed { tag, env }` variant or a separate envelope
// frame; the choice is captured in the rollout plan.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Compute HMAC-SHA256(session_token, body) and return the 32-byte tag
/// as a lowercase-hex string. body is typically the JSON-encoded
/// payload of a Request / Response — exactly the bytes that travel on
/// the wire. Caller is responsible for passing a stable serialization.
pub fn sign_body(session_token: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(session_token.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(body);
    let tag = mac.finalize().into_bytes();
    tag.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Verify that `expected_tag_hex` matches HMAC-SHA256(session_token, body).
/// Constant-time hex compare so a patcher can't binary-search the tag
/// byte-by-byte through timing.
pub fn verify_body(session_token: &str, body: &[u8], expected_tag_hex: &str) -> bool {
    let actual = sign_body(session_token, body);
    if actual.len() != expected_tag_hex.len() {
        return false;
    }
    actual.as_bytes().ct_eq(expected_tag_hex.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrips() {
        let req = Envelope::Request(Request {
            request_id: 42,
            feature_id: "Disable-WindowsDefender".to_string(),
            args: serde_json::json!({}),
        });
        let s = serde_json::to_string(&req).unwrap();
        let back: Envelope = serde_json::from_str(&s).unwrap();
        match back {
            Envelope::Request(r) => assert_eq!(r.request_id, 42),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn notification_roundtrips() {
        let n = Envelope::Notification(Notification {
            event: "voice-lockdown-triggered".to_string(),
            payload: serde_json::json!({"reason": "wake-word", "ts": 1234567890u64}),
        });
        let s = serde_json::to_string(&n).unwrap();
        // serde_tag = "kind" + rename_all = "snake_case" → "notification".
        assert!(s.contains("\"kind\":\"notification\""));
        assert!(s.contains("\"event\":\"voice-lockdown-triggered\""));
        let back: Envelope = serde_json::from_str(&s).unwrap();
        match back {
            Envelope::Notification(n) => {
                assert_eq!(n.event, "voice-lockdown-triggered");
                assert_eq!(n.payload["reason"], "wake-word");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn notification_signs_and_unwraps() {
        // Pro wraps every notification in Signed before write; the Free
        // reader task verifies and unwraps it back to Notification.
        let original = Envelope::Notification(Notification {
            event: "decoy-accessed".to_string(),
            payload: serde_json::json!({"path": "C:\\Users\\admin\\decoy.docx"}),
        });
        let signed = original.clone().sign("session-tk-1");
        assert!(matches!(signed, Envelope::Signed(_)));
        let unwrapped = signed.verify_and_unwrap("session-tk-1").unwrap();
        match unwrapped {
            Envelope::Notification(n) => assert_eq!(n.event, "decoy-accessed"),
            _ => panic!("expected Notification after unwrap"),
        }
    }

    #[test]
    fn hello_serialises_with_kind_tag() {
        let h = Envelope::Hello(hello_from_free("token-abc"));
        let s = serde_json::to_string(&h).unwrap();
        assert!(s.contains("\"kind\":\"hello\""));
        assert!(s.contains("\"session_token\":\"token-abc\""));
    }

    /// Round-trip a Hello + Request + Response over an in-memory duplex
    /// pair. Proves the framing helpers handle multiple back-to-back
    /// frames correctly and that read/write are exact inverses.
    #[tokio::test]
    async fn framing_roundtrips_three_envelopes() {
        let (mut a, mut b) = tokio::io::duplex(8192);

        let writer = async {
            write_envelope(&mut a, &Envelope::Hello(hello_from_free("tk-1")))
                .await
                .unwrap();
            write_envelope(
                &mut a,
                &Envelope::Request(Request {
                    request_id: 7,
                    feature_id: "vault.create-volume".to_string(),
                    args: serde_json::json!({"size_mb": 512}),
                }),
            )
            .await
            .unwrap();
            write_envelope(
                &mut a,
                &Envelope::Response(Response {
                    request_id: 7,
                    result: serde_json::json!({"ok": true}),
                }),
            )
            .await
            .unwrap();
            a.shutdown().await.unwrap();
        };

        let reader = async {
            let h = read_envelope(&mut b).await.unwrap();
            match h {
                Envelope::Hello(h) => assert_eq!(h.session_token, "tk-1"),
                _ => panic!("expected Hello"),
            }
            let r = read_envelope(&mut b).await.unwrap();
            match r {
                Envelope::Request(r) => assert_eq!(r.feature_id, "vault.create-volume"),
                _ => panic!("expected Request"),
            }
            let s = read_envelope(&mut b).await.unwrap();
            match s {
                Envelope::Response(s) => assert_eq!(s.request_id, 7),
                _ => panic!("expected Response"),
            }
        };

        tokio::join!(writer, reader);
    }

    // ── Phase 9b: signed envelopes ───────────────────────────────────

    #[test]
    fn signed_envelope_roundtrips() {
        let original = Envelope::Request(Request {
            request_id: 9,
            feature_id: "Disable-WindowsDefender".to_string(),
            args: serde_json::json!({}),
        });
        let signed = original.clone().sign("token-abc");
        // Wire form should be a Signed variant.
        assert!(matches!(signed, Envelope::Signed(_)));
        let unwrapped = signed.verify_and_unwrap("token-abc").unwrap();
        match unwrapped {
            Envelope::Request(r) => assert_eq!(r.request_id, 9),
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn signed_envelope_rejects_wrong_token() {
        let signed = Envelope::Bye.sign("real-token");
        assert!(matches!(
            signed.verify_and_unwrap("forged-token"),
            Err("HMAC tag mismatch — frame may have been tampered with or replayed")
        ));
    }

    #[test]
    fn signed_envelope_survives_wire_roundtrip_with_floats() {
        // The real Free→Pro path: sign, serialize to the wire, deserialize on
        // the peer, then verify. Floats in the payload are what exposed the old
        // re-serialization bug — a peer whose float formatter (ryu) differed by
        // a single digit produced a non-matching tag and silently dropped the
        // frame. Byte-exact verification must survive this regardless.
        let original = Envelope::Request(Request {
            request_id: 11,
            feature_id: "Flow-Sync-Rules".to_string(),
            args: serde_json::json!({
                "confidence": 0.1,
                "threshold": 1e-7,
                "ratio": 3.3333333333333335,
                "nested": { "opacity": 230.0, "list": [0.5, 0.25, 0.125] },
            }),
        });
        let signed = original.sign("session-tk");
        let wire = serde_json::to_string(&signed).unwrap();
        let received: Envelope = serde_json::from_str(&wire).unwrap();
        let unwrapped = received.verify_and_unwrap("session-tk").unwrap();
        match unwrapped {
            Envelope::Request(r) => {
                assert_eq!(r.request_id, 11);
                assert_eq!(r.args["confidence"], serde_json::json!(0.1));
            }
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn verify_uses_raw_bytes_not_reserialization() {
        // The inner carries a float written as `1E2`, which any re-serialization
        // would rewrite to `100.0`. Signing over — and verifying against — the
        // exact raw bytes must succeed; if verify re-encoded the parsed inner
        // (the old bug) the tag would not match. This is the byte-exact guarantee
        // that immunises the channel against Free/Pro float-formatter skew.
        let raw = "{\"kind\":\"request\",\"request_id\":1,\"feature_id\":\"x\",\"args\":1E2}";
        let tag = sign_body("tk", raw.as_bytes());
        let signed = Envelope::Signed(SignedEnvelope {
            tag,
            inner: raw.to_string(),
        });
        assert!(signed.verify_and_unwrap("tk").is_ok());
    }

    #[test]
    fn verify_unwrap_refuses_unsigned() {
        let unsigned = Envelope::Bye;
        assert!(matches!(
            unsigned.verify_and_unwrap("token"),
            Err("envelope was not signed")
        ));
    }

    #[test]
    fn sign_is_idempotent() {
        // Calling sign twice produces the same Signed envelope —
        // guards against accidental double-signing during refactors.
        let req = Envelope::Request(Request {
            request_id: 1,
            feature_id: "x".to_string(),
            args: serde_json::Value::Null,
        });
        let once = req.clone().sign("tk");
        let twice = once.clone().sign("tk");
        // Both serialise identically.
        let a = serde_json::to_string(&once).unwrap();
        let b = serde_json::to_string(&twice).unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn framing_rejects_oversize_length_prefix() {
        // Synthetically write a giant length prefix without the body.
        // read_envelope must refuse before allocating.
        let (mut a, mut b) = tokio::io::duplex(1024);
        a.write_u32_le(MAX_PAYLOAD_BYTES + 1).await.unwrap();
        a.shutdown().await.unwrap();
        let err = read_envelope(&mut b).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    // ── Phase 9 HMAC helpers ─────────────────────────────────────────

    #[test]
    fn hmac_signs_and_verifies() {
        let token = "session-token-abc";
        let body = br#"{"feature_id":"vault.create-volume"}"#;
        let tag = sign_body(token, body);
        assert_eq!(tag.len(), 64); // sha256 -> 32 bytes -> 64 hex chars
        assert!(verify_body(token, body, &tag));
    }

    #[test]
    fn hmac_rejects_wrong_token() {
        let body = br#"{"x":1}"#;
        let tag = sign_body("token-A", body);
        assert!(!verify_body("token-B", body, &tag));
    }

    #[test]
    fn hmac_rejects_tampered_body() {
        let token = "tk";
        let tag = sign_body(token, b"original");
        assert!(!verify_body(token, b"tampered", &tag));
    }

    #[test]
    fn hmac_rejects_truncated_tag() {
        let token = "tk";
        let body = b"hello";
        let tag = sign_body(token, body);
        assert!(!verify_body(token, body, &tag[..tag.len() - 1]));
    }
}

pub mod command_strings;

/// Fleet wire types — shared with `commander-pro/fleet-server` and the desktop
/// Fleet admin panel (via ts-rs). Extracted into the standalone `fleet-proto`
/// crate (SSOT — also consumed by TuxCommander and secureOS); re-exported here
/// unchanged so every existing `wincmd_shared::fleet::*` call site keeps
/// compiling without modification.
pub use fleet_proto as fleet;

/// UI ↔ SYSTEM-service RPC namespace (`svc.*`). Defines the pipe name,
/// protocol version, capability-class enum, and `classify_verb` — the shared
/// contract that both the desktop UI and `commander-svc` import. See `svc.rs`.
pub mod svc;

/// F6 USB wipe-authorization handshake token (Phase 1, Piece 1).
/// Ed25519-signed, device-bound, nonce-bearing, TTL-limited token that gates
/// the USB wipe environment — no valid token means the USB does nothing.
/// Pure crypto + format; no I/O, no destructive actions. See `wipe_auth.rs`.
pub mod wipe_auth;

/// F6 Phase-1 Piece 2 — reboot-to-USB arming predicate (pure, no I/O, no destructive calls).
pub mod reboot_usb_predicate;

/// F6 Phase-1 Piece 2 — wipe token + pubkey write helper (filesystem write only; no reboot/erase).
pub mod wipe_token_write;
