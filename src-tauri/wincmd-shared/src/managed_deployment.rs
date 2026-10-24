// SPDX-License-Identifier: AGPL-3.0-or-later
//! Neutral authority envelope for a service-owned managed deployment.

use crate::fleet::SignedCommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BIND_VERB: &str = "svc.managed_deployment.bind";
pub const APPLY_VERB: &str = "svc.managed_deployment.apply";
pub const STATUS_VERB: &str = "svc.managed_deployment.status";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct ManagedDeploymentContext {
    pub org_id: String,
    pub device_id: String,
    pub signing_key_b64: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
#[serde(deny_unknown_fields)]
pub struct ManagedDeploymentRequest {
    pub command: SignedCommand,
    pub bundle: Value,
}
