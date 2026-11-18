// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::collections::BTreeMap;

#[test]
fn every_registered_tauri_handler_is_cli_executable() {
    let catalog = catalog().expect("catalog parses");
    let tauri_commands: Vec<_> = catalog
        .commands
        .iter()
        .filter(|entry| entry.transport == Transport::Tauri)
        .collect();
    let registry = expected_cli_registry();
    assert_catalog_matches(&catalog, &registry).expect("catalog must match the actual registry");
    assert!(!tauri_commands
        .iter()
        .any(|entry| entry.name == "apply_admin_config_cmd"));
    assert!(tauri_commands.iter().all(|entry| entry.registered));
    for name in [
        "decoy_read_audit_status",
        "fleet_report_local_alert",
        "fleet_report_privacy_shield_status",
        "fleet_sync_shield_state",
        "list_search_known_folders",
        "list_search_storage_roots",
        "open_event_log",
        "set_decoy_read_audit_enabled",
        "set_update_channel",
        "get_content_detector_catalog",
        "test_local_content_rules",
        "reconcile_shared_shortcut_claim",
        "require_calculator_peek_hotkey",
        "vault_enroll_personal_syncthing",
        "vault_manage_personal_syncthing",
        "vault_setup_cloud_backup",
        "vault_manage_cloud_backup",
    ] {
        assert!(
            tauri_commands.iter().any(|entry| entry.name == name),
            "generated CLI catalog is missing registered handler {name}"
        );
    }
    assert_eq!(
        tauri_commands
            .iter()
            .filter(|entry| entry.debug_only)
            .count(),
        registry.values().filter(|debug_only| **debug_only).count()
    );
    assert!(tauri_commands
        .iter()
        .filter(|entry| available_in_this_build(entry))
        .all(|entry| match classify_risk(entry) {
            Risk::ReadOnly => classify_headless_support(entry) == HeadlessSupport::Executable,
            Risk::Mutating | Risk::Destructive => {
                classify_headless_support(entry) == HeadlessSupport::UiOnly
            }
        }));
    assert_eq!(
        tauri_commands
            .iter()
            .filter(|entry| available_in_this_build(entry))
            .count(),
        registry
            .values()
            .filter(|debug_only| cfg!(debug_assertions) || !**debug_only)
            .count()
    );
}

// The generator deliberately excludes these internal UI/startup hooks. Keep
// exceptions explicit so widening that policy still requires a reviewed change.
fn expected_cli_registry() -> BTreeMap<String, bool> {
    let mut registry =
        registered_handlers(include_str!("lib.rs")).expect("handler registry parses");
    for internal in [
        "content_cleanup_old_cache",
        "content_recover_interrupted_cache",
        "mark_tauri_cli_ready",
        "complete_tauri_cli",
        "startup_window_ready",
        "calculator_gate_generation",
        "open_windows_calculator",
        "dashboard_camera_presence",
        "is_autostart_enabled",
        "enable_autostart_task",
    ] {
        assert!(
            registry.remove(internal).is_some(),
            "missing internal handler {internal}"
        );
    }
    registry
}

// Follow the generator's registration format, but read lib.rs independently:
// a stale, duplicated or wrong-gated catalog must not define its own expectation.
fn registered_handlers(source: &str) -> Result<BTreeMap<String, bool>, &'static str> {
    let (_, after) = source
        .split_once("tauri::generate_handler![")
        .ok_or("missing registry")?;
    let (block, _) = after
        .split_once("        ])")
        .ok_or("unterminated registry")?;
    let handler =
        regex::Regex::new(r"^(?:[A-Za-z_][A-Za-z0-9_]*::)*([A-Za-z_][A-Za-z0-9_]*),$").unwrap();
    let cfg = regex::Regex::new(r"^#\[cfg\((.+)\)\]$").unwrap();
    let mut result = BTreeMap::new();
    let mut debug_only = false;
    for line in block.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(captures) = cfg.captures(line) {
            if captures[1].trim() != "debug_assertions" || debug_only {
                return Err("unsupported handler cfg");
            }
            debug_only = true;
            continue;
        }
        let captures = handler
            .captures(line)
            .ok_or("unsupported handler registration")?;
        if result.insert(captures[1].to_string(), debug_only).is_some() {
            return Err("duplicate registered handler");
        }
        debug_only = false;
    }
    if debug_only || result.is_empty() {
        return Err("incomplete handler registry");
    }
    Ok(result)
}

fn assert_catalog_matches(
    catalog: &GeneratedCatalog,
    registry: &BTreeMap<String, bool>,
) -> Result<(), &'static str> {
    let mut actual = BTreeMap::new();
    for entry in catalog
        .commands
        .iter()
        .filter(|entry| entry.transport == Transport::Tauri)
    {
        if actual
            .insert(entry.name.clone(), entry.debug_only)
            .is_some()
        {
            return Err("duplicate catalog handler");
        }
    }
    if actual != *registry {
        return Err("catalog does not match registered names and debug gates");
    }
    Ok(())
}
