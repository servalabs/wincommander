// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disabled service-owned grant primitives. No recovery pipe verb is registered.
#![allow(dead_code)]

use super::{peer_has_active_interactive_session, service_peer_privileges, AuthenticatedPipePeer};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex, time::{Duration, Instant}};

#[path = "fleet_recovery_native_confirmation.rs"]
mod native_confirmation;
#[path = "fleet_recovery_signed_context.rs"]
mod signed_context;

const MAX_GRANTS: usize = 64;
const GRANT_LIFETIME: Duration = Duration::from_secs(120);

#[derive(Clone, PartialEq, Eq)]
struct Peer {
    sid: String,
    session: u32,
    logon: (u32, i32),
}

impl Peer {
    fn capture(peer: &AuthenticatedPipePeer) -> Result<Self, &'static str> {
        if peer.caller_sid().is_empty() || peer.session_id() == 0
            || !peer_has_active_interactive_session(peer)
            || !service_peer_privileges::token_is_local_administrator(peer.token()).unwrap_or(false) {
            return Err("local interactive administrator required");
        }
        Ok(Self { sid: peer.caller_sid().to_owned(), session: peer.session_id(), logon: peer.authentication_id() })
    }
}

#[derive(Clone, PartialEq, Eq)]
struct BoundRequest {
    recovery_id: String,
    context_digest: [u8; 32],
    replacement_fingerprint: [u8; 32],
}

impl BoundRequest {
    fn from_verified(context: &signed_context::VerifiedContext, public_key: &signed_context::VerifiedLocalPublicKey) -> Self {
        Self { recovery_id: context.recovery_id().into(), context_digest: Sha256::digest(context.bytes()).into(), replacement_fingerprint: Sha256::digest(public_key.bytes()).into() }
    }
}

struct Grant { peer: Peer, request: BoundRequest, deadline: Instant }

enum NativeConfirmation {
    Verified { peer: Peer, request: BoundRequest },
}

#[derive(Default)]
struct GrantStore { grants: Mutex<HashMap<[u8; 32], Grant>> }

impl GrantStore {
    // A native helper exists but is disabled. No IPC may assert confirmation.
    fn issue(&self, peer: &AuthenticatedPipePeer, _context: &[u8], _public_key: &[u8; 32]) -> Result<[u8; 32], &'static str> {
        Peer::capture(peer)?;
        Err("protected native recovery confirmation is unavailable")
    }

    fn consume(&self, peer: &AuthenticatedPipePeer, _token: &[u8; 32], _context: &[u8], _public_key: &[u8; 32]) -> Result<(), &'static str> {
        Peer::capture(peer)?;
        Err("protected native recovery confirmation is unavailable")
    }

    // Trust is locked before the grant store; clock reads occur under that lock.
    fn mint_after_dialog_revalidated(&self, store: &crate::fleet_machine_policy::FleetMachinePolicy,
        initial: &signed_context::VerifiedContext, key: &signed_context::VerifiedLocalPublicKey,
        confirmation: NativeConfirmation, peer: Peer) -> Result<[u8; 32], &'static str> {
        self.mint_with_clock(store, initial, key, confirmation, peer, service_clock)
    }

    fn mint_with_clock(&self, store: &crate::fleet_machine_policy::FleetMachinePolicy,
        initial: &signed_context::VerifiedContext, key: &signed_context::VerifiedLocalPublicKey,
        confirmation: NativeConfirmation, peer: Peer,
        clock: impl FnOnce() -> Result<(i64, Instant), &'static str>) -> Result<[u8; 32], &'static str> {
        store.with_recovery_trust(|trust| {
            let mut grants = self.grants.lock().map_err(|_| "recovery authorization unavailable")?;
            let (wall, now) = clock()?;
            let fresh = signed_context::verify_with_trust(initial.bytes(), trust, wall)?;
            Self::mint_locked(&mut grants, confirmation, peer, BoundRequest::from_verified(&fresh, key), now)
        })
    }

    fn consume_after_revalidation(&self, store: &crate::fleet_machine_policy::FleetMachinePolicy,
        initial: &signed_context::VerifiedContext, key: &signed_context::VerifiedLocalPublicKey,
        peer: &Peer, token: &[u8; 32]) -> bool {
        self.consume_with_clock(store, initial, key, peer, token, service_clock)
    }

    fn consume_with_clock(&self, store: &crate::fleet_machine_policy::FleetMachinePolicy,
        initial: &signed_context::VerifiedContext, key: &signed_context::VerifiedLocalPublicKey,
        peer: &Peer, token: &[u8; 32],
        clock: impl FnOnce() -> Result<(i64, Instant), &'static str>) -> bool {
        store.with_recovery_trust(|trust| {
            let mut grants = self.grants.lock().map_err(|_| "recovery authorization unavailable")?;
            let (wall, now) = clock()?;
            let fresh = signed_context::verify_with_trust(initial.bytes(), trust, wall)?;
            Ok(Self::consume_locked(&mut grants, peer, token, &BoundRequest::from_verified(&fresh, key), now))
        }).unwrap_or(false)
    }

    // These primitives require a service-authenticated confirmation issuer
    // before they can be connected to either disabled method above.
    fn mint_after_verified_confirmation(&self, confirmation: NativeConfirmation, peer: Peer, request: BoundRequest, now: Instant) -> Result<[u8; 32], &'static str> {
        let mut grants = self.grants.lock().map_err(|_| "recovery authorization unavailable")?;
        Self::mint_locked(&mut grants, confirmation, peer, request, now)
    }

    fn mint_locked(grants: &mut HashMap<[u8; 32], Grant>, confirmation: NativeConfirmation, peer: Peer, request: BoundRequest, now: Instant) -> Result<[u8; 32], &'static str> {
        match confirmation {
            NativeConfirmation::Verified { peer: confirmed_peer, request: confirmed_request }
                if confirmed_peer == peer && confirmed_request == request => (),
            _ => return Err("recovery confirmation does not match request"),
        }
        grants.retain(|_, grant| grant.deadline > now);
        if grants.len() >= MAX_GRANTS { return Err("recovery authorization unavailable"); }
        let deadline = now.checked_add(GRANT_LIFETIME).ok_or("recovery authorization unavailable")?;
        let mut token = [0; 32];
        OsRng.try_fill_bytes(&mut token).map_err(|_| "recovery authorization unavailable")?;
        let digest: [u8; 32] = Sha256::digest(token).into();
        if grants.contains_key(&digest) { return Err("recovery authorization unavailable"); }
        grants.insert(digest, Grant { peer, request, deadline });
        Ok(token)
    }

    fn consume_verified(&self, peer: &Peer, token: &[u8; 32], request: &BoundRequest, now: Instant) -> bool {
        let Ok(mut grants) = self.grants.lock() else { return false; };
        Self::consume_locked(&mut grants, peer, token, request, now)
    }

    fn consume_locked(grants: &mut HashMap<[u8; 32], Grant>, peer: &Peer, token: &[u8; 32], request: &BoundRequest, now: Instant) -> bool {
        let digest: [u8; 32] = Sha256::digest(token).into();
        if !grants.get(&digest).is_some_and(|grant| grant.deadline > now && &grant.peer == peer && &grant.request == request) {
            return false;
        }
        grants.remove(&digest).is_some()
    }
}

fn service_clock() -> Result<(i64, Instant), &'static str> {
    let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "recovery authorization unavailable")?;
    Ok((i64::try_from(elapsed.as_secs()).map_err(|_| "recovery authorization unavailable")?, Instant::now()))
}
