use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use rand::{rngs::SysRng, TryRng};
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
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    pub(super) folder: Option<crate::disk_tree_delete::PreparedTree>,
}

pub(super) static STORE: Lazy<Mutex<HashMap<String, Minted>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) fn hash_args(args_canonical: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(args_canonical.as_bytes());
    h.finalize().into()
}

/// Mint a capability for (action, args). Returns the opaque token id.
pub fn mint(action: DestructiveAction, args_canonical: &str) -> Result<String, String> {
    mint_with(action, args_canonical, |raw| SysRng.try_fill_bytes(raw).map_err(|_| "confirmation randomness unavailable".to_string()))
}

fn mint_with(action: DestructiveAction, args_canonical: &str, fill: impl FnOnce(&mut [u8]) -> Result<(), String>) -> Result<String, String> {
    let mut raw = [0u8; 32];
    fill(&mut raw)?;
    let token = raw.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let mut store = STORE.lock().map_err(|_| "confirmation unavailable (authorization store failure)")?;
    // Opportunistic GC of expired tokens.
    store.retain(|_, m| m.at.elapsed() < TTL);
    store.insert(
        token.clone(),
        Minted {
            action,
            args_hash: hash_args(args_canonical),
            at: Instant::now(),
            #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
            folder: None,
        },
    );
    Ok(token)
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
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    if m.folder.is_some() {
        return Err("confirmation does not match this request".into());
    }
    if m.action != action {
        return Err("confirmation does not match this action".into());
    }
    if m.args_hash != hash_args(args_canonical) {
        return Err("confirmation does not match this request".into());
    }
    Ok(())
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
fn folder_args(path: &str) -> String {
    serde_json::to_string(&("disk_folder_delete", path)).expect("string serializes")
}

/// Own the confirmed handle until consumption or expiration. The reaper is
/// mandatory: expired, unused capabilities must not indefinitely pin folders.
#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
pub(super) fn mint_folder(path: &str, folder: crate::disk_tree_delete::PreparedTree) -> Result<String, String> {
    static REAPER: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REAPER.get_or_init(|| std::thread::Builder::new().name("folder-confirmation-expiry".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if let Ok(mut store) = STORE.lock() { store.retain(|_, m| m.at.elapsed() < TTL); }
    }).map(|_| ()).map_err(|_| "Could not arrange folder confirmation expiry".to_string())).clone()?;
    let mut raw = [0u8; 32];
    SysRng.try_fill_bytes(&mut raw).map_err(|_| "confirmation randomness unavailable".to_string())?;
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    {
        let mut store = STORE.lock().map_err(|_| "confirmation unavailable".to_string())?;
        store.retain(|_, m| m.at.elapsed() < TTL);
        if store.values().filter(|m| m.folder.is_some()).count() >= 4 {
            return Err("Too many pending folder confirmations".into());
        }
        store.insert(token.clone(), Minted { action: DestructiveAction::DiskDelete, args_hash: hash_args(&folder_args(path)), at: Instant::now(), folder: Some(folder) });
    }
    Ok(token)
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
pub(crate) fn consume_folder(token: Option<&str>, path: &str) -> Result<crate::disk_tree_delete::PreparedTree, String> {
    let token = token.filter(|s| !s.trim().is_empty()).ok_or("confirmation required")?;
    let mut store = STORE.lock().map_err(|_| "confirmation unavailable")?;
    let mut entry = store.remove(token).ok_or("confirmation required")?;
    if entry.at.elapsed() >= TTL || entry.action != DestructiveAction::DiskDelete || entry.args_hash != hash_args(&folder_args(path)) {
        return Err("confirmation does not match this request or has expired".into());
    }
    entry.folder.take().ok_or_else(|| "confirmation does not match this folder".into())
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

#[cfg(test)]
mod randomness_tests {
    use super::*;
    #[test]
    fn failed_randomness_cannot_mint_capability() {
        assert!(mint_with(DestructiveAction::DiskDelete, "target", |_| Err("unavailable".into())).is_err());
    }
}
