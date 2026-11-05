// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coordinate direct desktop cache changes with an in-progress service release.
#[path = "updater/hybrid_free_anchor.rs"]
#[allow(dead_code)] // Only marker presence is needed here.
mod anchor;
#[path = "fleet_lifecycle_mutex.rs"]
#[allow(dead_code)] // Only named-lock acquisition is needed here.
mod coordination;
#[path = "../../runtime-support/licence_release_path.rs"]
mod release_path;

const LOCK: &str = r"Global\WinCommander.Licence.Security.v1";
const UNAVAILABLE: &str = "A licence release is in progress or its security state is unavailable. Retry the release before changing the licence.";

pub(super) struct Guard {
    _guard: coordination::Guard,
}

pub(super) fn acquire() -> Result<Guard, String> {
    acquire_at(
        &anchor::Anchor::machine_named(release_path::RELEASE_OPERATION_REGISTRY_PATH),
        LOCK,
    )
}

fn acquire_at(store: &anchor::Anchor, name: &str) -> Result<Guard, String> {
    let guard = coordination::acquire_named(name, 30_000).map_err(|_| UNAVAILABLE.to_string())?;
    if store.exists().map_err(|_| UNAVAILABLE.to_string())? {
        return Err(UNAVAILABLE.into());
    }
    Ok(Guard { _guard: guard })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_marker_blocks_direct_cache_changes_after_reopen() {
        let id = uuid::Uuid::new_v4();
        let path = format!(r"SOFTWARE\WinCommanderDesktopReleaseGateTest-{id}");
        let name = format!(r"Local\WinCommander.DesktopReleaseTest.{id}");
        let store = anchor::Anchor::test_user(path.clone());
        drop(acquire_at(&store, &name).unwrap());
        store.write(b"pending").unwrap();
        assert!(acquire_at(&store, &name).is_err());
        assert!(acquire_at(&anchor::Anchor::test_user(path.clone()), &name).is_err());
        anchor::remove_test_key(&path).unwrap();
        drop(acquire_at(&store, &name).unwrap());
    }
}
