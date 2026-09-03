use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const TTL: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DestructiveAction {
    SelfDestruct,
    RemoveUsers,
    CryptoErase,
    FleetReenroll,
    DiskDelete,
    DecoyDelete,
    KillSwitch,
}

pub(super) struct Minted {
    pub(super) action: DestructiveAction,
    pub(super) args_hash: [u8; 32],
    pub(super) at: Instant,
}

pub(super) static STORE: Lazy<Mutex<HashMap<String, Minted>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) fn hash_args(args_canonical: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(args_canonical.as_bytes());
    h.finalize().into()
}

/// Mint a capability for (action, args). Returns the opaque token id.
pub fn mint(action: DestructiveAction, args_canonical: &str) -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = raw.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let mut store = STORE.lock().expect("authz store poisoned");
    // Opportunistic GC of expired tokens.
    store.retain(|_, m| m.at.elapsed() < TTL);
    store.insert(
        token.clone(),
        Minted {
            action,
            args_hash: hash_args(args_canonical),
            at: Instant::now(),
        },
    );
    token
}

/// Consume a capability. Fails closed on any mismatch, expiry, or reuse.
pub fn consume(token: &str, action: DestructiveAction, args_canonical: &str) -> Result<(), String> {
    let mut store = STORE
        .lock()
        .map_err(|_| "confirmation unavailable (authorization store failure)".to_string())?;
    let Some(m) = store.remove(token) else {
        return Err("confirmation required (no valid capability)".into());
    };
    if m.at.elapsed() >= TTL {
        return Err("confirmation expired — please re-confirm".into());
    }
    if m.action != action {
        return Err("confirmation does not match this action".into());
    }
    if m.args_hash != hash_args(args_canonical) {
        return Err("confirmation does not match this request".into());
    }
    Ok(())
}

/// Require and consume a caller-supplied capability at the mutation boundary.
/// `Option` is deliberate: a forged Tauri invoke that omits the argument must
/// reach an explicit fail-closed decision rather than relying on deserialization.
pub fn consume_required(
    token: Option<&str>,
    action: DestructiveAction,
    args_canonical: &str,
) -> Result<(), String> {
    let token = token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "confirmation required (capability missing)".to_string())?;
    consume(token, action, args_canonical)
}
