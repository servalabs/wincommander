// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned trust and crash-safe execution journal for managed deployments.

mod authority;
#[cfg(test)]
mod tests;

#[cfg(windows)]
mod live;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
pub(crate) use live::{apply, bind, status};
