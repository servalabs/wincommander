// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only model of a service-owned generation/effect fence. No IPC route or
//! durable enforcement is wired; a client-held token alone cannot fence an effect.

use crate::policy_store::PolicyFs;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[cfg(windows)]
#[path = "fleet_effect_lock.rs"]
mod windows_lock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Generation([u8; 16]);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Lease([u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Effect {
    Idle,
    Running(Lease),
    Unknown(Lease),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fence {
    generation: Generation,
    effect: Effect,
}

/// Test-only durable wrapper. Missing, malformed, inaccessible, or uncertain
/// storage fails closed; callers may not invent an idle record on failure.
struct DurableFence {
    fs: Box<dyn PolicyFs>,
    coordination: Arc<dyn Coordination>,
    path: PathBuf,
    state: Mutex<Option<Fence>>,
}

/// Production requires an ACL-verified OS-level lock implementation. None is
/// supplied by this test-only model, so it cannot be opened by runtime code.
trait Coordination: Send + Sync {
    fn validates_directory(&self, directory: &std::path::Path) -> bool;
    fn acquire(&self) -> Result<Box<dyn CoordinationGuard + '_>, &'static str>;
}
trait CoordinationGuard {}

fn decode(bytes: &[u8]) -> Result<Fence, &'static str> {
    if bytes.len() > 4096 {
        return Err("fence_unavailable");
    }
    let state: Fence = serde_json::from_slice(bytes).map_err(|_| "fence_unavailable")?;
    if state.generation.0 == [0; 16]
        || matches!(state.effect, Effect::Running(lease) | Effect::Unknown(lease) if lease.0 == [0;16])
    {
        return Err("fence_unavailable");
    }
    Ok(state)
}

impl DurableFence {
    fn open_existing(
        fs: Box<dyn PolicyFs>,
        coordination: Arc<dyn Coordination>,
        dir: PathBuf,
    ) -> Result<Self, &'static str> {
        if !coordination.validates_directory(&dir) {
            return Err("fence_unavailable");
        }
        // Coordination must verify already-provisioned directories; never
        // create or repair security while opening the fence.
        let _guard = coordination.acquire()?;
        let path = dir.join("effect-fence.json");
        let bytes = fs.read(&path).map_err(|_| "fence_unavailable")?;
        let previous = decode(&bytes)?;
        let recovered = previous.after_restart();
        if recovered != previous {
            let bytes = serde_json::to_vec(&recovered).map_err(|_| "fence_unavailable")?;
            fs.atomic_write(&path, &bytes)
                .map_err(|_| "fence_unavailable")?;
        }
        drop(_guard);
        Ok(Self {
            fs,
            coordination,
            path,
            state: Mutex::new(Some(recovered)),
        })
    }

    fn update(&self, apply: impl FnOnce(&mut Fence) -> Result<(), ()>) -> Result<(), &'static str> {
        let _guard = self.coordination.acquire()?;
        let mut current = self.state.lock().map_err(|_| "fence_unavailable")?;
        if current.is_none() {
            return Err("fence_unavailable");
        }
        // Never act on a cached snapshot: another service instance may have
        // committed a generation or effect while this instance was idle.
        let mut next = match self
            .fs
            .read(&self.path)
            .map_err(|_| "fence_unavailable")
            .and_then(|bytes| decode(&bytes))
        {
            Ok(value) => value,
            Err(error) => {
                *current = None;
                return Err(error);
            }
        };
        apply(&mut next).map_err(|_| "fence_conflict")?;
        let bytes = serde_json::to_vec(&next).map_err(|_| "fence_unavailable")?;
        if self.fs.atomic_write(&self.path, &bytes).is_err() {
            // A failed write may have reached disk; no decisions until reopen.
            *current = None;
            return Err("fence_unavailable");
        }
        *current = Some(next);
        Ok(())
    }
}

impl Fence {
    fn begin(&mut self, generation: Generation, lease: Lease) -> Result<(), ()> {
        if self.generation != generation || self.effect != Effect::Idle || lease.0 == [0; 16] {
            return Err(());
        }
        self.effect = Effect::Running(lease);
        Ok(())
    }

    /// A timeout is uncertain: it is not cancellation or proof of failure.
    fn timeout(&mut self, lease: Lease) -> Result<(), ()> {
        if self.effect != Effect::Running(lease) {
            return Err(());
        }
        self.effect = Effect::Unknown(lease);
        Ok(())
    }

    /// The service may release only after it has established that execution
    /// ended and the result is independently verified. A bare ack is not enough.
    fn finish_verified(&mut self, lease: Lease) -> Result<(), ()> {
        if !matches!(self.effect, Effect::Running(v) | Effect::Unknown(v) if v == lease) {
            return Err(());
        }
        self.effect = Effect::Idle;
        Ok(())
    }

    fn replace_generation(&mut self, old: Generation, new: Generation) -> Result<(), ()> {
        if self.generation != old || self.effect != Effect::Idle || new == old || new.0 == [0; 16] {
            return Err(());
        }
        self.generation = new;
        Ok(())
    }

    /// A service restart cannot assume an in-flight effect stopped.
    fn after_restart(mut self) -> Self {
        if let Effect::Running(lease) = self.effect {
            self.effect = Effect::Unknown(lease);
        }
        self
    }
}
