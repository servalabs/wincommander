//! Destructive-action authorization facade.
//!
//! Capabilities, canonical arguments, renderer dispatch policy, confirmation
//! requests, and trusted Rust-owned triggers are split by responsibility while
//! preserving the existing `crate::authz::*` API.

mod arguments;
mod capability;
mod dispatch;
mod request;
mod trusted;

pub use arguments::{
    canonical_path, decoy_delete_args, disk_delete_args, full_lockdown_args, kill_switch_args,
    lockdown_args, LockdownPlanSnapshot,
};
pub use capability::{consume, consume_required, DestructiveAction};
pub use dispatch::{
    action_for, authorize_backend_dispatch, backend_dispatch_policy, BackendDispatchPolicy,
};
pub use request::DestructiveRequest;
pub use trusted::native_confirm_action;
pub(crate) use trusted::{
    confirm_backend_dispatch, execute_trusted_lockdown, schedule_trusted_lockdown,
    toggle_trusted_lockdown,
};

#[tauri::command]
pub async fn request_destructive_confirmation(
    app: tauri::AppHandle,
    request: DestructiveRequest,
    pin: Option<String>,
) -> Result<String, String> {
    request::request_destructive_confirmation_impl(app, request, pin).await
}

#[cfg(test)]
use arguments::{free_space_erase_args, secure_erase_args};
#[cfg(test)]
use capability::mint;
#[cfg(test)]
use capability::{hash_args, Minted, STORE, TTL};
#[cfg(test)]
pub use dispatch::DESTRUCTIVE_COMMANDS;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::time::Instant;
#[cfg(test)]
use trusted::TrustedLockdownGate;

#[cfg(test)]
mod tests;
