// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-wide fence for one protected licence release operation.
use super::licence_device_anchor::Anchor;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[path = "../../runtime-support/licence_release_path.rs"]
mod release_path;

const INVALID: &str = "license release is in progress or unavailable";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Marker {
    version: u8,
    context: String,
    #[serde(deserialize_with = "required_request")]
    request: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finalization: Option<Finalization>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed: Option<bool>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Finalization {
    version: u8,
    phase: String,
    licence_id: String,
    device_hash: String,
    credential_id: String,
    generation: u64,
    cache: String,
    cache_digest: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PreparedRelease {
    pub context: [u8; 32],
    pub licence_id: String,
    pub device_hash: String,
    pub credential_id: [u8; 32],
    pub generation: u64,
    pub cache: Vec<u8>,
}

fn canonical_fixed(value: &str, length: usize) -> bool {
    URL_SAFE_NO_PAD.decode(value).ok().filter(|bytes| bytes.len() == length && URL_SAFE_NO_PAD.encode(bytes) == value).is_some()
}

fn valid_finalization(value: &Finalization) -> bool {
    if value.version != 1 || value.phase != "prepared"
        || value.licence_id.is_empty() || value.licence_id.len() > 512
        || value.device_hash.is_empty() || value.device_hash.len() > 512
        || !canonical_fixed(&value.credential_id, 32) || value.generation == 0 || value.generation > 9_007_199_254_740_991 {
        return false;
    }
    let Ok(cache) = URL_SAFE_NO_PAD.decode(&value.cache) else { return false; };
    !cache.is_empty() && cache.len() <= 32 * 1024 && URL_SAFE_NO_PAD.encode(&cache) == value.cache
        && canonical_fixed(&value.cache_digest, 32) && digest(&cache) == value.cache_digest
}

fn required_request<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

pub(crate) fn machine() -> Anchor {
    Anchor::machine_named(release_path::RELEASE_OPERATION_REGISTRY_PATH)
}
fn digest(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value))
}

fn read(store: &Anchor, context: &[u8; 32]) -> Result<Marker, &'static str> {
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    if !matches!(marker.version, 1 | 2 | 3)
        || (marker.version == 1 && marker.finalization.is_some())
        || (marker.version >= 2 && marker.finalization.is_none())
        || (marker.version < 3 && marker.completed.is_some())
        || (marker.version == 3 && marker.completed != Some(true))
        || marker.context != URL_SAFE_NO_PAD.encode(context)
        || marker.request.as_ref().is_some_and(|value| {
            URL_SAFE_NO_PAD
                .decode(value)
                .ok()
                .filter(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == *value)
                .is_none()
        })
        || marker.finalization.as_ref().is_some_and(|value| marker.request.is_none() || !valid_finalization(value))
    {
        return Err(INVALID);
    }
    Ok(marker)
}

fn persist(store: &Anchor, marker: &Marker, context: &[u8; 32]) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(marker).map_err(|_| INVALID)?;
    store.write(&bytes).map_err(|_| INVALID)?;
    let reopened = read(store, context)?;
    if &reopened != marker {
        return Err(INVALID);
    }
    Ok(())
}

/// Read an already prepared record. This does not authenticate its licence,
/// credential or acknowledgement and never authorizes a deletion by itself.
pub(crate) fn prepared_at(store: &Anchor) -> Result<Option<PreparedRelease>, &'static str> {
    journal_at(store, 2)
}

/// Read a completed journal for exact idempotent recovery, not as cleanup authority.
pub(crate) fn completed_at(store: &Anchor) -> Result<Option<PreparedRelease>, &'static str> {
    journal_at(store, 3)
}

fn journal_at(store: &Anchor, version: u8) -> Result<Option<PreparedRelease>, &'static str> {
    if !store.exists().map_err(|_| INVALID)? { return Ok(None); }
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    let preliminary: Marker = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    let context: [u8; 32] = URL_SAFE_NO_PAD.decode(&preliminary.context).map_err(|_| INVALID)?
        .try_into().map_err(|_| INVALID)?;
    if URL_SAFE_NO_PAD.encode(context) != preliminary.context { return Err(INVALID); }
    let marker = read(store, &context)?;
    if marker.version != version { return Ok(None); }
    let Some(value) = marker.finalization else { return Ok(None); };
    let credential_id = URL_SAFE_NO_PAD.decode(&value.credential_id).map_err(|_| INVALID)?
        .try_into().map_err(|_| INVALID)?;
    let cache = URL_SAFE_NO_PAD.decode(&value.cache).map_err(|_| INVALID)?;
    Ok(Some(PreparedRelease { context, licence_id: value.licence_id, device_hash: value.device_hash,
        credential_id, generation: value.generation, cache }))
}

/// Caller holds both the cache and cross-process licence locks.
pub(crate) fn ensure_idle() -> Result<(), &'static str> {
    ensure_idle_at(&machine())
}
pub(crate) fn ensure_idle_at(store: &Anchor) -> Result<(), &'static str> {
    if idle_at(store)? { Ok(()) } else { Err(INVALID) }
}

fn idle_at(store: &Anchor) -> Result<bool, &'static str> {
    if !store.exists().map_err(|_| INVALID)? { return Ok(true); }
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    let context: [u8; 32] = URL_SAFE_NO_PAD.decode(&marker.context).map_err(|_| INVALID)?.try_into().map_err(|_| INVALID)?;
    Ok(read(store, &context)?.version == 3)
}

/// Only the credential and request context which owns an unfinished gate may
/// resume from retained history. A completed gate is not a retry authority.
pub(crate) fn matches_in_progress_at(store: &Anchor, expected: &[u8; 32]) -> Result<bool, &'static str> {
    if !store.exists().map_err(|_| INVALID)? { return Ok(false); }
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    let context: [u8; 32] = URL_SAFE_NO_PAD.decode(&marker.context).map_err(|_| INVALID)?
        .try_into().map_err(|_| INVALID)?;
    let marker = read(store, &context)?;
    Ok(marker.version != 3 && &context == expected)
}

/// A failed write remains fenced, including an empty key created before failure.
pub(crate) fn begin_at(store: &Anchor, context: &[u8; 32]) -> Result<(), &'static str> {
    if !idle_at(store)? {
        read(store, context)?;
        return Ok(());
    }
    persist(
        store,
        &Marker {
            version: 1,
            context: URL_SAFE_NO_PAD.encode(context),
            request: None,
            finalization: None,
            completed: None,
        },
        context,
    )
}

/// Only called under all licence locks after the exact acknowledgement, cache
/// deletion and protected completion floor have been reverified and persisted.
pub(crate) fn finish_at(store: &Anchor, expected: &PreparedRelease, request: &[u8]) -> Result<(), &'static str> {
    let actual = prepared_at(store)?.or(completed_at(store)?).ok_or(INVALID)?;
    if &actual != expected { return Err(INVALID); }
    check_request_proof_at(store, &expected.context, request)?;
    let mut marker = read(store, &expected.context)?;
    if marker.version == 3 { return Ok(()); }
    marker.version = 3;
    marker.completed = Some(true);
    persist(store, &marker, &expected.context)
}

/// The caller has verified the acknowledged request, released ledger floor and
/// exact cache under the credential, cache and process locks. No deletion or
/// gate clearing is authorized by this preparation alone.
pub(crate) fn prepare_finalization_at(
    store: &Anchor, context: &[u8; 32], request: &[u8], licence_id: &str, device_hash: &str,
    credential_id: &[u8; 32], generation: u64, cache: &[u8],
) -> Result<(), &'static str> {
    let mut marker = read(store, context)?;
    if request.is_empty() || marker.request.as_deref() != Some(digest(request).as_str()) { return Err(INVALID); }
    let journal = Finalization { version: 1, phase: "prepared".into(), licence_id: licence_id.into(),
        device_hash: device_hash.into(), credential_id: URL_SAFE_NO_PAD.encode(credential_id), generation,
        cache: URL_SAFE_NO_PAD.encode(cache), cache_digest: digest(cache) };
    if !valid_finalization(&journal) { return Err(INVALID); }
    match &marker.finalization {
        Some(existing) if existing == &journal => Ok(()),
        Some(_) => Err(INVALID),
        None => { marker.version = 2; marker.finalization = Some(journal); persist(store, &marker, context) }
    }
}

/// A staged request becomes the only request this operation may send or retry.
pub(crate) fn check_request(
    context: &[u8; 32],
    request: Option<&[u8]>,
) -> Result<(), &'static str> {
    check_request_at(&machine(), context, request)
}
pub(crate) fn check_request_at(
    store: &Anchor,
    context: &[u8; 32],
    request: Option<&[u8]>,
) -> Result<(), &'static str> {
    if idle_at(store)? {
        return if request.is_none() {
            Ok(())
        } else {
            Err(INVALID)
        };
    }
    let marker = read(store, context)?;
    match (marker.request.as_deref(), request) {
        (None, None) => Ok(()),
        (Some(saved), Some(request)) if saved == digest(request) => Ok(()),
        _ => Err(INVALID),
    }
}

/// Compare a request against an exact prepared or completed journal.
pub(crate) fn check_request_proof_at(store: &Anchor, context: &[u8; 32], request: &[u8]) -> Result<(), &'static str> {
    let marker = read(store, context)?;
    if matches!(marker.version, 2 | 3) && !request.is_empty() && marker.request.as_deref() == Some(digest(request).as_str()) {
        Ok(())
    } else { Err(INVALID) }
}

pub(crate) fn bind_request_at(
    store: &Anchor,
    context: &[u8; 32],
    request: &[u8],
) -> Result<(), &'static str> {
    if request.is_empty() {
        return Err(INVALID);
    }
    let mut marker = read(store, context)?;
    let request = digest(request);
    match marker.request.as_deref() {
        Some(saved) if saved == request => Ok(()),
        None => {
            marker.request = Some(request);
            persist(store, &marker, context)
        }
        _ => Err(INVALID),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_progress_gate_survives_process_restart() {
        const CHILD_PATH: &str = "WINCMD_TEST_RELEASE_RETRY_GATE";
        if let Ok(path) = std::env::var(CHILD_PATH) {
            let reopened = Anchor::test_user(path);
            assert!(matches_in_progress_at(&reopened, &[19; 32]).unwrap());
            assert!(!matches_in_progress_at(&reopened, &[20; 32]).unwrap());
            check_request_at(&reopened, &[19; 32], Some(b"saved request")).unwrap();
            return;
        }
        let path = format!(r"SOFTWARE\WinCommanderReleaseRestartTest-{}", uuid::Uuid::new_v4());
        let store = Anchor::test_user(path.clone());
        begin_at(&store, &[19; 32]).unwrap();
        bind_request_at(&store, &[19; 32], b"saved request").unwrap();
        drop(store);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "license_cache::release_gate::tests::in_progress_gate_survives_process_restart"])
            .env(CHILD_PATH, &path).output().unwrap();
        super::super::licence_device_anchor::remove_test_key(&path).unwrap();
        assert!(child.status.success());
        assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
    }

    #[test]
    fn release_marker_survives_reopen_and_allows_only_exact_request() {
        let path = format!(
            r"SOFTWARE\WinCommanderReleaseGateTest-{}",
            uuid::Uuid::new_v4()
        );
        let store = Anchor::test_user(path.clone());
        let context = [7; 32];
        assert!(!matches_in_progress_at(&store, &context).unwrap());
        assert!(ensure_idle_at(&store).is_ok());
        assert!(check_request_at(&store, &context, None).is_ok());
        assert!(check_request_at(&store, &context, Some(b"request")).is_err());
        begin_at(&store, &context).unwrap();
        assert!(matches_in_progress_at(&store, &context).unwrap());
        assert!(!matches_in_progress_at(&store, &[8; 32]).unwrap());
        assert!(ensure_idle_at(&store).is_err());
        assert!(begin_at(&store, &[8; 32]).is_err());
        check_request_at(&store, &context, None).unwrap();
        assert!(check_request_at(&store, &context, Some(b"request")).is_err());
        bind_request_at(&store, &context, b"request").unwrap();
        let reopened = Anchor::test_user(path.clone());
        assert!(matches_in_progress_at(&reopened, &context).unwrap());
        begin_at(&reopened, &context).unwrap();
        check_request_at(&reopened, &context, Some(b"request")).unwrap();
        assert!(check_request_at(&reopened, &context, Some(b"changed")).is_err());
        assert!(check_request_at(&reopened, &context, None).is_err());
        let prepare = |cache: &[u8], generation: u64| prepare_finalization_at(
            &reopened, &context, b"request", "licence", "device", &[3; 32], generation, cache);
        assert!(prepare_finalization_at(&reopened, &context, b"other", "licence", "device", &[3; 32], 2, b"cache").is_err());
        assert!(prepare(b"cache", 0).is_err());
        prepare(b"cache", 2).unwrap();
        prepare(b"cache", 2).unwrap();
        let journal = prepared_at(&Anchor::test_user(path.clone())).unwrap().unwrap();
        assert_eq!((journal.context, journal.credential_id, journal.generation), (context, [3; 32], 2));
        assert_eq!((journal.licence_id.as_str(), journal.device_hash.as_str(), journal.cache.as_slice()), ("licence", "device", b"cache".as_slice()));
        assert!(prepare(b"different", 2).is_err());
        assert!(prepare(b"cache", 3).is_err());
        assert!(ensure_idle_at(&Anchor::test_user(path.clone())).is_err());
        assert!(read(&Anchor::test_user(path.clone()), &context).unwrap().finalization.is_some());
        let bytes = reopened.read().unwrap().unwrap();
        let mut damaged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        damaged["finalization"]["cache_digest"] = URL_SAFE_NO_PAD.encode([4; 32]).into();
        reopened.write(&serde_json::to_vec(&damaged).unwrap()).unwrap();
        assert!(prepare(b"cache", 2).is_err());
        let mut missing: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        missing.as_object_mut().unwrap().remove("finalization");
        reopened.write(&serde_json::to_vec(&missing).unwrap()).unwrap();
        assert!(prepare(b"cache", 2).is_err());
        let mut fake_completion: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        fake_completion["version"] = 3.into();
        reopened.write(&serde_json::to_vec(&fake_completion).unwrap()).unwrap();
        assert!(ensure_idle_at(&reopened).is_err());
        reopened.write(&bytes).unwrap();
        let complete = prepared_at(&reopened).unwrap().unwrap();
        let mut wrong = complete.clone();
        wrong.generation += 1;
        assert!(finish_at(&reopened, &wrong, b"request").is_err());
        assert!(finish_at(&reopened, &complete, b"wrong").is_err());
        assert!(ensure_idle_at(&reopened).is_err());
        finish_at(&reopened, &complete, b"request").unwrap();
        assert!(!matches_in_progress_at(&reopened, &context).unwrap());
        assert!(ensure_idle_at(&Anchor::test_user(path.clone())).is_ok());
        assert!(prepared_at(&reopened).unwrap().is_none());
        assert!(completed_at(&reopened).unwrap().is_some());
        finish_at(&reopened, &complete, b"request").unwrap();
        let completed_bytes = reopened.read().unwrap().unwrap();
        let mut missing_completion: serde_json::Value = serde_json::from_slice(&completed_bytes).unwrap();
        missing_completion.as_object_mut().unwrap().remove("completed");
        reopened.write(&serde_json::to_vec(&missing_completion).unwrap()).unwrap();
        assert!(ensure_idle_at(&reopened).is_err());
        reopened.write(&completed_bytes).unwrap();
        let newer_context = [9; 32];
        begin_at(&reopened, &newer_context).unwrap();
        assert!(finish_at(&reopened, &complete, b"request").is_err());
        assert!(ensure_idle_at(&reopened).is_err());
        reopened
            .write(
                format!(
                    r#"{{"version":1,"context":"{}"}}"#,
                    URL_SAFE_NO_PAD.encode(context)
                )
                .as_bytes(),
            )
            .unwrap();
        assert!(begin_at(&reopened, &context).is_err());
        reopened.write(b"damaged").unwrap();
        assert!(matches_in_progress_at(&reopened, &context).is_err());
        assert!(begin_at(&reopened, &context).is_err());
        assert!(ensure_idle_at(&reopened).is_err());
        super::super::licence_device_anchor::remove_test_key(&path).unwrap();
    }
}
