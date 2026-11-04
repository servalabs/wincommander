// SPDX-License-Identifier: AGPL-3.0-or-later
//! Enrollment-bound Windows wire records. These pure contracts do not enable
//! transport, persistence, command execution, or legacy-protocol fallback.
//!
//! Windows v2 check-in authenticates its original body bytes using a separate
//! method/path-bound HMAC header. This authenticates bytes; it does not itself
//! validate JSON, freshness, nonce replay, enrollment state, or telemetry.
//!
//! Windows peers must require the v2 block by authenticated device kind and
//! reject mixed v1 reports. Decode original complete wire bytes with
//! [`decode_windows_v2`] before extracting this block. Arbitrary JSON is exact
//! text; counters wider than u16 are canonical decimal strings on the wire.
//! Signed outcomes use a closed summary: exact optional ACK-result hash or the
//! existing readiness self-test states/timestamp. They never copy ACK details.
//! Preserve the original signed summary and timestamp spelling in storage;
//! validate parsed fields separately. This contract is not an absolute privacy
//! guarantee against a malicious device encoding data in otherwise valid fields.
//!
//! Consume one detached proof only for its live nonce, original request/response
//! buffers and current enrollment/lifecycle. Verify it before acting on any
//! response field, including jobs or policy outside this block. The required
//! command epoch floor is independent of policy revision and applies to each
//! command in the response; equal epochs are permitted. Required delivery
//! issuance/expiry timestamps are full signed-i64 decimal strings bound only
//! by the response proof, not the offline action signature. Consumers must
//! validate the delivery interval and enforce a monotonic freshness deadline
//! from request start before dispatch; decoding or action-signature validation
//! alone does not authorize delivery. Action signature
//! verification alone does not bind the server-assigned delivery ID.
//!
//! Accepted/duplicate ACK receipts settle only the matching report ID and hash.
//! A head alone never discards pending outcomes. Retain unconfirmed evidence;
//! definitive rejection may be quarantined locally, never treated as success.
//! Servers release proofs only after committing the exact claimed receipts and
//! dispatch state; storage/signing failure must not consume one-shot delivery.
mod checkin_auth;
mod command;
mod decimal;
mod decode;
mod envelopes;
mod lossless;
mod outcome;
mod receipt;
mod validation;

pub use checkin_auth::*;
pub use command::*;
pub use decode::*;
pub use envelopes::*;
pub use lossless::*;
pub use outcome::*;
pub use receipt::*;
use validation::*;

use crate::{DeviceId, OrgId};
use serde::{Deserialize, Serialize};

pub const WINDOWS_WIRE_VERSION: u16 = 2;

/// Select this context from verified enrollment, never from an incoming record.
/// The authority is established by the configured enrollment transport and any
/// configured signing-key pin; this type does not require an additional pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct FleetEnrollmentBinding {
    pub org_id: OrgId,
    pub device_id: DeviceId,
    pub enrollment_generation: String,
    pub authority_key_b64: String,
}

impl FleetEnrollmentBinding {
    pub fn is_valid(&self) -> bool {
        !self.org_id.0.is_empty()
            && !self.device_id.0.is_empty()
            && self.device_id.0.len() <= 128
            && canonical_generation(&self.enrollment_generation)
            && strong_canonical_key(&self.authority_key_b64)
    }
}

#[cfg(test)]
mod tests;
