//! Persisted fleet client state + the `SecretStore` persistence seam.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Platform-supplied secret persistence seam.
///
/// `fleet-agent-core` is behavior-identical whether or not a `SecretStore` is
/// wired in: with `persist: None` (the default), the client behaves exactly as
/// an in-memory-only agent always has (identity forgotten on restart). A
/// platform that wants the device identity/secrets to survive a restart wires
/// a `SecretStore` impl (e.g. a root-only file on Linux, DPAPI on Windows).
///
/// Implementations MUST be fail-safe: any store error should be treated by the
/// caller as "not persisted" rather than crashing the check-in loop.
pub trait SecretStore: Send + Sync {
    fn get(&self, k: &str) -> anyhow::Result<Option<Vec<u8>>>;
    fn put(&self, k: &str, v: &[u8]) -> anyhow::Result<()>;
    fn delete(&self, k: &str) -> anyhow::Result<()>;
}

/// Fleet state held by the agent (in-memory by default).
///
/// When `persist` is `Some`, the owning platform is responsible for reading the
/// persisted values back into this struct at startup and writing them out on
/// change (fleet-agent-core does not auto-wire persistence timing — see the
/// `transport` loop for where `enroll()` reads/writes each field). When
/// `persist` is `None`, behavior is identical to the historical in-memory-only
/// state.
#[derive(Clone, Default)]
pub struct FleetClientState {
    /// Device ID assigned by the fleet server at enrollment.
    pub device_id: Option<String>,
    /// Pinned OPERATOR command public key (base64) — verifies DURESS commands
    /// (`fleet_proto::is_duress_catalog`). Set at enroll; refused if changed.
    pub pinned_pubkey_b64: Option<String>,
    /// Pinned SERVER signing key (base64) — verifies ordinary (server-signed)
    /// commands (WinCommander's lockdown/dismount/etc.). Set at enroll; refused
    /// if changed. `None` for agents/servers that don't supply it.
    pub pinned_server_key_b64: Option<String>,
    /// Per-device HMAC secret returned by the server at enroll time (raw bytes).
    /// Overrides `FleetConfig::checkin_secret` when present (per-device secret
    /// takes precedence over the globally-configured one).
    pub checkin_secret: Option<Vec<u8>>,
    /// Seen-nonce map for replay defense on fleet commands.
    /// Value: Unix timestamp when the nonce was first seen (for TTL eviction).
    pub seen_nonces: HashMap<String, i64>,
    /// Number of check-in intervals where no all_clear was received.
    pub dead_man_misses: u32,
    /// RFC3339 timestamp of the last successful check-in.
    pub last_checkin_at: Option<String>,
    /// Optional persistence seam. `None` preserves today's in-memory-only behavior.
    pub persist: Option<Arc<dyn SecretStore>>,
}

impl std::fmt::Debug for FleetClientState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FleetClientState")
            .field("device_id", &self.device_id)
            .field("pinned_pubkey_b64", &self.pinned_pubkey_b64)
            .field("pinned_server_key_b64", &self.pinned_server_key_b64)
            .field(
                "checkin_secret",
                &self.checkin_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("seen_nonces_len", &self.seen_nonces.len())
            .field("dead_man_misses", &self.dead_man_misses)
            .field("last_checkin_at", &self.last_checkin_at)
            .field("persist", &self.persist.is_some())
            .finish()
    }
}

/// Shared fleet state accessible from both the background loop and the health / CLI surface.
pub type SharedFleetState = Arc<Mutex<FleetClientState>>;

/// Create a new shared fleet state with no persistence seam (in-memory default).
pub fn new_shared_fleet_state() -> SharedFleetState {
    Arc::new(Mutex::new(FleetClientState::default()))
}

/// Create a new shared fleet state backed by a `SecretStore`.
///
/// This only wires the seam into the struct; the platform's `enroll()` call
/// site is responsible for reading persisted values back in at startup if it
/// wants restart-survives-identity behavior (fleet-agent-core does not do this
/// implicitly, to keep the in-memory default behavior byte-for-byte unchanged
/// when `persist` is absent).
pub fn new_shared_fleet_state_with_persist(persist: Arc<dyn SecretStore>) -> SharedFleetState {
    Arc::new(Mutex::new(FleetClientState {
        persist: Some(persist),
        ..Default::default()
    }))
}

// ── Status (for health + CLI surface) ─────────────────────────────────────────

/// Fleet status snapshot, surfaced by a platform's health/CLI surface.
#[derive(Debug, Clone)]
pub struct FleetStatus {
    /// Whether the device is enrolled (`device_id` is set).
    pub enrolled: bool,
    /// The fleet device ID, if enrolled.
    pub device_id: Option<String>,
    /// RFC3339 timestamp of the last successful check-in, if any.
    pub last_checkin_at: Option<String>,
    /// Number of consecutive dead-man misses.
    pub dead_man_misses: u32,
}

impl FleetStatus {
    pub fn from_state(state: &FleetClientState) -> Self {
        Self {
            enrolled: state.device_id.is_some(),
            device_id: state.device_id.clone(),
            last_checkin_at: state.last_checkin_at.clone(),
            dead_man_misses: state.dead_man_misses,
        }
    }

    pub fn not_configured() -> Self {
        Self {
            enrolled: false,
            device_id: None,
            last_checkin_at: None,
            dead_man_misses: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    struct MemStore(StdMutex<HashMap<String, Vec<u8>>>);

    impl SecretStore for MemStore {
        fn get(&self, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn put(&self, k: &str, v: &[u8]) -> anyhow::Result<()> {
            self.0.lock().unwrap().insert(k.to_string(), v.to_vec());
            Ok(())
        }
        fn delete(&self, k: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }

    #[test]
    fn default_state_has_no_persist_seam() {
        let state = new_shared_fleet_state();
        let s = state.lock().unwrap();
        assert!(s.persist.is_none());
        assert!(s.device_id.is_none());
    }

    #[test]
    fn state_with_persist_wires_the_seam() {
        let store: Arc<dyn SecretStore> = Arc::new(MemStore(StdMutex::new(HashMap::new())));
        let state = new_shared_fleet_state_with_persist(store);
        let s = state.lock().unwrap();
        assert!(s.persist.is_some());
    }

    #[test]
    fn secret_store_roundtrips() {
        let store = MemStore(StdMutex::new(HashMap::new()));
        assert_eq!(store.get("k").unwrap(), None);
        store.put("k", b"v").unwrap();
        assert_eq!(store.get("k").unwrap(), Some(b"v".to_vec()));
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
    }

    #[test]
    fn fleet_status_not_configured_defaults() {
        let status = FleetStatus::not_configured();
        assert!(!status.enrolled);
        assert_eq!(status.dead_man_misses, 0);
    }
}
