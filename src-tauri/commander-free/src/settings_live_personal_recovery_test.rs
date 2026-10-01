// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in, same-process proof for the personal-settings recovery boundary.
//! It uses the current Windows account's already-provisioned local service and
//! changes only process memory; no preference or secret is written.
use super::*;

#[test]
#[ignore = "requires WINCOMMANDER_LIVE_SETTINGS_PROBE=1 and a provisioned current-user service"]
fn live_service_replaces_a_temporary_cached_snapshot_without_replaying_it() {
    assert_eq!(
        std::env::var("WINCOMMANDER_LIVE_SETTINGS_PROBE")
            .ok()
            .as_deref(),
        Some("1"),
        "set WINCOMMANDER_LIVE_SETTINGS_PROBE=1 before running this ignored current-account probe"
    );
    let _global = GLOBAL_STATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let baseline = get_settings_sync().expect("current service settings must load");
    assert_eq!(baseline["personalSettingsStatus"]["mode"], "service");
    assert_eq!(baseline["personalSettingsStatus"]["canSave"], true);
    let baseline_app = baseline["app"].clone();
    let before_cache = SETTINGS_CACHE.lock().expect("settings cache lock").clone();
    let mut stale = before_cache
        .clone()
        .expect("baseline get_settings must warm the cache");
    stale.app.logging_enabled = Some(!stale.app.logging_enabled.unwrap_or(true));
    stale.snapshot_revision = Some(uuid::Uuid::new_v4());
    let stale_revision = stale.snapshot_revision;

    let temporary = super::personal_settings::replace_session_with_temporary_for_test()
        .expect("temporary fixture");
    *SETTINGS_CACHE.lock().expect("settings cache lock") = Some(stale);

    let recovered = get_settings_sync().expect("service recovery must reload settings");
    assert_eq!(recovered["personalSettingsStatus"]["mode"], "service");
    assert_eq!(recovered["personalSettingsStatus"]["canSave"], true);
    assert_eq!(recovered["app"], baseline_app);
    assert_ne!(
        cached_settings().and_then(|settings| settings.snapshot_revision),
        stale_revision,
        "the temporary in-memory revision must not survive recovery"
    );

    *SETTINGS_CACHE.lock().expect("settings cache lock") = before_cache;
    drop(temporary);
}
