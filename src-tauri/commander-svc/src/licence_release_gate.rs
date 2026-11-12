// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-wide fence for one protected licence release operation.
use super::licence_device_anchor::Anchor;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[path = "../../runtime-support/licence_release_path.rs"]
mod release_path;

const INVALID: &str = "license release is in progress or unavailable";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    version: u8,
    context: String,
    #[serde(deserialize_with = "required_request")]
    request: Option<String>,
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
    if marker.version != 1
        || marker.context != URL_SAFE_NO_PAD.encode(context)
        || marker.request.as_ref().is_some_and(|value| {
            URL_SAFE_NO_PAD
                .decode(value)
                .ok()
                .filter(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == *value)
                .is_none()
        })
    {
        return Err(INVALID);
    }
    Ok(marker)
}

fn persist(store: &Anchor, marker: &Marker, context: &[u8; 32]) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(marker).map_err(|_| INVALID)?;
    store.write(&bytes).map_err(|_| INVALID)?;
    let reopened = read(store, context)?;
    if reopened.request != marker.request {
        return Err(INVALID);
    }
    Ok(())
}

/// Caller holds both the cache and cross-process licence locks.
pub(crate) fn ensure_idle() -> Result<(), &'static str> {
    ensure_idle_at(&machine())
}
pub(crate) fn ensure_idle_at(store: &Anchor) -> Result<(), &'static str> {
    if store.exists().map_err(|_| INVALID)? {
        return Err(INVALID);
    }
    Ok(())
}

/// A failed write remains fenced, including an empty key created before failure.
pub(crate) fn begin_at(store: &Anchor, context: &[u8; 32]) -> Result<(), &'static str> {
    if store.exists().map_err(|_| INVALID)? {
        read(store, context)?;
        return Ok(());
    }
    persist(
        store,
        &Marker {
            version: 1,
            context: URL_SAFE_NO_PAD.encode(context),
            request: None,
        },
        context,
    )
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
    if !store.exists().map_err(|_| INVALID)? {
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
    fn release_marker_survives_reopen_and_allows_only_exact_request() {
        let path = format!(
            r"SOFTWARE\WinCommanderReleaseGateTest-{}",
            uuid::Uuid::new_v4()
        );
        let store = Anchor::test_user(path.clone());
        let context = [7; 32];
        assert!(ensure_idle_at(&store).is_ok());
        assert!(check_request_at(&store, &context, None).is_ok());
        assert!(check_request_at(&store, &context, Some(b"request")).is_err());
        begin_at(&store, &context).unwrap();
        assert!(ensure_idle_at(&store).is_err());
        assert!(begin_at(&store, &[8; 32]).is_err());
        check_request_at(&store, &context, None).unwrap();
        assert!(check_request_at(&store, &context, Some(b"request")).is_err());
        bind_request_at(&store, &context, b"request").unwrap();
        let reopened = Anchor::test_user(path.clone());
        begin_at(&reopened, &context).unwrap();
        check_request_at(&reopened, &context, Some(b"request")).unwrap();
        assert!(check_request_at(&reopened, &context, Some(b"changed")).is_err());
        assert!(check_request_at(&reopened, &context, None).is_err());
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
        assert!(begin_at(&reopened, &context).is_err());
        assert!(ensure_idle_at(&reopened).is_err());
        super::super::licence_device_anchor::remove_test_key(&path).unwrap();
    }
}
