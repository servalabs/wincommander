// SPDX-License-Identifier: AGPL-3.0-or-later
//! In-process native-selection grants. Issuers must derive owners from a trusted
//! window/connection and accept paths only from native pickers; the renderer
//! must never supply an owner or a path. Sidecar authorization is separate.
//! Values must retain the selected object and carry its checked identity; this
//! store alone cannot authenticate a peer or authorize a filesystem operation.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use wincmd_shared::file_package::PackageToken;

const LIFETIME: Duration = Duration::from_secs(600);
const MAX_GLOBAL: usize = 20_020;
const MAX_PER_OWNER: usize = 10_010;
pub(crate) const MAX_BATCH: usize = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Owner([u8; 32]);
impl Owner {
    /// Created by native code for one window/connection lifetime, not deserialized.
    pub(crate) fn random() -> Result<Self, &'static str> {
        let mut value = [0; 32];
        getrandom::fill(&mut value).map_err(|_| "selection unavailable")?;
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    SourceFile,
    SourceDirectory,
    Carrier,
    Output,
    Destination,
}

struct Grant<T> {
    owner: Owner,
    purpose: Purpose,
    value: T,
    expires: Instant,
}
pub(crate) struct GrantStore<T> {
    entries: HashMap<[u8; 32], Grant<T>>,
    counts: HashMap<Owner, usize>,
    max_global: usize,
    max_owner: usize,
}
impl<T> Default for GrantStore<T> {
    fn default() -> Self {
        Self::with_limits(MAX_GLOBAL, MAX_PER_OWNER)
    }
}
impl<T> GrantStore<T> {
    fn with_limits(max_global: usize, max_owner: usize) -> Self {
        Self {
            entries: HashMap::new(),
            counts: HashMap::new(),
            max_global,
            max_owner,
        }
    }

    fn remove(&mut self, token: &[u8; 32]) {
        if let Some(grant) = self.entries.remove(token) {
            if let Some(count) = self.counts.get_mut(&grant.owner) {
                *count -= 1;
                if *count == 0 {
                    self.counts.remove(&grant.owner);
                }
            }
        }
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, grant| now < grant.expires);
        self.counts.clear();
        for grant in self.entries.values() {
            *self.counts.entry(grant.owner).or_default() += 1;
        }
    }

    pub(crate) fn expire(&mut self, now: Instant) {
        self.prune(now);
    }

    /// All tokens are generated before committing any paths, so an error never
    /// leaves a partially issued batch. Call only with native picker results.
    pub(crate) fn issue(
        &mut self,
        owner: Owner,
        purpose: Purpose,
        paths: Vec<T>,
        now: Instant,
    ) -> Result<Vec<PackageToken>, &'static str> {
        self.issue_with(owner, purpose, paths, now, |bytes| {
            getrandom::fill(bytes).map_err(|_| ())
        })
    }

    fn issue_with(
        &mut self,
        owner: Owner,
        purpose: Purpose,
        paths: Vec<T>,
        now: Instant,
        mut random: impl FnMut(&mut [u8; 32]) -> Result<(), ()>,
    ) -> Result<Vec<PackageToken>, &'static str> {
        if paths.is_empty() || paths.len() > MAX_BATCH {
            return Err("invalid selection count");
        }
        let count = paths.len();
        let full = |store: &Self| {
            store.entries.len().saturating_add(count) > store.max_global
                || store
                    .counts
                    .get(&owner)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(count)
                    > store.max_owner
        };
        if full(self) {
            self.prune(now);
        }
        if full(self) {
            return Err("too many selections");
        }
        let expires = now.checked_add(LIFETIME).ok_or("selection unavailable")?;
        let mut generated = Vec::with_capacity(count);
        let mut batch = HashSet::with_capacity(count);
        for _ in 0..count {
            let mut value = [0u8; 32];
            random(&mut value).map_err(|_| "selection unavailable")?;
            if self.entries.contains_key(&value) || !batch.insert(value) {
                return Err("selection unavailable");
            }
            generated.push(value);
        }
        let public: Vec<_> = generated
            .iter()
            .map(|value| PackageToken::new(hex::encode(value)).expect("canonical token"))
            .collect();
        for (token, path) in generated.into_iter().zip(paths) {
            self.entries.insert(
                token,
                Grant {
                    owner,
                    purpose,
                    value: path,
                    expires,
                },
            );
        }
        *self.counts.entry(owner).or_default() += count;
        Ok(public)
    }

    /// Must be called only after authenticating the requesting owner. The path
    /// stays in native code and must be independently checked by the sidecar.
    pub(crate) fn resolve(
        &mut self,
        owner: Owner,
        purpose: Purpose,
        token: &PackageToken,
        now: Instant,
    ) -> Option<&T> {
        let key: [u8; 32] = hex::decode(token.as_str()).ok()?.try_into().ok()?;
        let expired = self
            .entries
            .get(&key)
            .is_some_and(|grant| now >= grant.expires);
        if expired {
            self.remove(&key);
            return None;
        }
        let grant = self.entries.get(&key)?;
        (grant.owner == owner && grant.purpose == purpose).then_some(&grant.value)
    }

    pub(crate) fn revoke_owner(&mut self, owner: Owner) {
        self.entries.retain(|_, grant| grant.owner != owner);
        self.counts.remove(&owner);
    }

    pub(crate) fn revoke(&mut self, owner: Owner, token: &PackageToken) -> bool {
        let Some(key) = hex::decode(token.as_str())
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
        else {
            return false;
        };
        if !self
            .entries
            .get(&key)
            .is_some_and(|grant| grant.owner == owner)
        {
            return false;
        }
        self.remove(&key);
        true
    }
}

#[cfg(test)]
#[path = "file_package_grants_tests.rs"]
mod tests;
