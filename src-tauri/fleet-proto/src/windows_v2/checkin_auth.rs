// SPDX-License-Identifier: AGPL-3.0-or-later
//! Exact-byte authentication for the Windows v2 check-in transport.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// Adapters must pass every occurrence to the verifier and reject any header
/// value that cannot be represented as ASCII, rather than selecting one.
pub const WINDOWS_V2_CHECKIN_AUTH_HEADER: &str = "x-fleet-windows-v2-auth";
const DOMAIN: &[u8] = b"fleet.windows.checkin-auth.v2\nPOST\n/v1/agents/checkin\n";
const METHOD: &str = "POST";
const PATH: &str = "/v1/agents/checkin";
const SECRET_BYTES: usize = 32;
const MAC_BYTES: usize = 32;
type HmacSha256 = Hmac<Sha256>;

fn mac(secret: &[u8], method: &str, path: &str, body: &[u8]) -> Option<HmacSha256> {
    if secret.len() != SECRET_BYTES || method != METHOD || path != PATH {
        return None;
    }
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(DOMAIN);
    mac.update(body);
    Some(mac)
}

/// Create one ASCII header value using the enrolled device's check-in secret.
/// No JSON parsing or canonicalization occurs; `body` is the exact wire body.
/// Freshness, enrollment binding and replay checks remain transport duties.
pub fn sign_windows_v2_checkin_auth(
    secret: &[u8],
    method: &str,
    path: &str,
    body: &[u8],
) -> Option<String> {
    let signature = mac(secret, method, path, body)?.finalize().into_bytes();
    Some(format!("v2.{}", B64.encode(signature)))
}

/// Verify exactly one canonical header value using constant-time MAC checking.
/// Comma-folded, duplicate, non-ASCII and malformed values are rejected. This
/// authenticates the exact bytes only; it does not interpret their contents.
pub fn verify_windows_v2_checkin_auth(
    secret: &[u8],
    method: &str,
    path: &str,
    body: &[u8],
    headers: &[&str],
) -> bool {
    let [header] = headers else { return false };
    let Some(encoded) = header.strip_prefix("v2.") else {
        return false;
    };
    // A 32-byte MAC has one canonical padded Base64 length; reject oversized
    // values before allocating a decode buffer.
    if encoded.len() != 44 || !header.is_ascii() {
        return false;
    }
    let Ok(signature) = B64.decode(encoded) else {
        return false;
    };
    if signature.len() != MAC_BYTES || B64.encode(&signature) != encoded {
        return false;
    }
    let Some(mac) = mac(secret, method, path, body) else {
        return false;
    };
    mac.verify_slice(&signature).is_ok()
}

#[cfg(test)]
#[path = "tests_checkin_auth.rs"]
mod tests;
