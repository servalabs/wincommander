// SPDX-License-Identifier: AGPL-3.0-or-later
//! Neutral adapters for service-owned inventory and bounded receipts.
//! These are not authorization rules: only the authenticated service decides.

use crate::vault_access::{PersonalVaultMountedVolume, VaultMountReason};
use crate::vault_display_path::normalize_vault_display_path;
use serde_json::{json, Value};

pub fn parse_mounts(value: Value) -> Result<Vec<PersonalVaultMountedVolume>, String> {
    let mut mounts: Vec<PersonalVaultMountedVolume> = serde_json::from_value(value)
        .map_err(|_| "vault_service_personal_status_invalid".to_owned())?;
    let mut slots = std::collections::HashSet::new();
    let mut letters = std::collections::HashSet::new();
    if mounts.len() > 26
        || mounts.iter().any(|mount| {
            let letter = mount.drive_letter.as_bytes();
            mount.internal_drive > 25
                || !slots.insert(mount.internal_drive)
                || !(letter.len() == 1 || letter.len() == 2)
                || !letter[0].is_ascii_alphabetic()
                || (letter.len() == 2 && letter[1] != b':')
                || !letters.insert(letter[0].to_ascii_uppercase())
        })
    {
        return Err("vault_service_personal_status_invalid".into());
    }
    for mount in &mut mounts {
        mount.drive_letter = format!(
            "{}:",
            char::from(mount.drive_letter.as_bytes()[0].to_ascii_uppercase())
        );
    }
    Ok(mounts)
}

pub fn project_mount(mount: &PersonalVaultMountedVolume, accessible: bool) -> Value {
    json!({
        "letter": mount.drive_letter,
        "path": mount.canonical_container_path.as_deref().and_then(normalize_vault_display_path),
        "type": "VeraCrypt", "internalDrive": mount.internal_drive,
        "presentation": mount.presentation,
        "accessible": accessible && !mount.cleanup_required,
        "cleanupRequired": mount.cleanup_required,
        "dismountAllowed": mount.dismount_allowed,
        "dismountReason": mount.dismount_reason,
        "browseAllowed": mount.browse_allowed,
    })
}

/// Resolve a drive-root selection only from the service-authorized inventory.
/// A renderer path, even one resembling a drive root, is never accepted.
pub fn authorized_root(
    mounts: &[PersonalVaultMountedVolume],
    letter: &str,
) -> Result<String, String> {
    let clean = letter.strip_suffix(':').unwrap_or(letter);
    if clean.len() != 1 || !clean.as_bytes()[0].is_ascii_alphabetic() {
        return Err("mount_state_unknown".into());
    }
    let selected = format!("{}:", clean.to_ascii_uppercase());
    let mount = mounts
        .iter()
        .find(|mount| mount.drive_letter.eq_ignore_ascii_case(&selected))
        .ok_or_else(|| "mount_state_unknown".to_owned())?;
    if !mount.browse_allowed || mount.cleanup_required {
        return Err("not_authorized".into());
    }
    Ok(format!("{selected}\\"))
}

pub fn confirmed_dismount(result: Value) -> Result<Value, String> {
    match result.get("state").and_then(Value::as_str) {
        Some("unmounted") => Ok(result),
        Some("denied" | "failed") => Err(bounded_reason(
            result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .to_owned()),
        _ => Err("vault_service_dismount_result_invalid".into()),
    }
}

pub fn bounded_reason(reason: &str) -> &'static str {
    // Exact adapters for existing Pro kind and Free service ErrorReply text.
    // Never search arbitrary messages for a familiar authorization substring.
    let kind = reason
        .strip_prefix("service rejected request: ")
        .and_then(|tail| tail.split_once(" (").map(|(kind, _)| kind))
        .unwrap_or(reason);
    let kind = kind.strip_prefix("vault_").unwrap_or(kind);
    VaultMountReason::from_wire(kind)
        .unwrap_or(VaultMountReason::MountStateUnknown)
        .as_str()
}

/// Slot responses use a synthetic ID so private policy IDs never cross this
/// adapter. Require the receipt to confirm the exact target requested.
pub fn confirmed_slot_dismount(result: Value, slot: u8) -> Result<Value, String> {
    let result = confirmed_dismount(result)?;
    if result.get("entry_id").and_then(Value::as_str) != Some(format!("personal:{slot}").as_str()) {
        return Err("mount_state_unknown".into());
    }
    Ok(result)
}

/// Remaining refers only to caller-visible targets not confirmed dismounted;
/// a physical failure does not prove those targets are still mounted.
pub fn bulk_failure(reason: &str, dismounted: usize, remaining: usize) -> String {
    let reason = bounded_reason(reason);
    if dismounted == 0 {
        return reason.to_owned();
    }
    format!("vault_bulk_dismount_partial:{reason}:{dismounted}:{remaining}")
}

pub fn bulk_success(dismounted: usize) -> Value {
    json!({"status":"authorized_dismounted", "state":"unmounted", "scope":"authorized", "dismounted":dismounted})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mount() -> Value {
        json!({"drive_letter":"V:","internal_drive":12,"presentation":"machine","cleanup_required":false,"dismount_allowed":false,"dismount_reason":"administrator_required","canonical_container_path":r"\??\D:\Vault\test5"})
    }

    #[test]
    fn inventory_rejects_duplicate_slots_letters_and_invalid_shapes() {
        let mut bare = mount();
        bare["drive_letter"] = json!("v");
        assert_eq!(parse_mounts(json!([bare])).unwrap()[0].drive_letter, "V:");
        let original = mount();
        assert!(parse_mounts(json!([original.clone(), original.clone()])).is_err());
        for (field, value) in [
            ("internal_drive", json!(26)),
            ("drive_letter", json!("C:\\secret")),
        ] {
            let mut invalid = original.clone();
            invalid[field] = value;
            assert!(parse_mounts(json!([invalid])).is_err());
        }
        let mut duplicate_letter = original.clone();
        duplicate_letter["internal_drive"] = json!(13);
        assert!(parse_mounts(json!([original, duplicate_letter])).is_err());
        assert!(parse_mounts(json!({"error":true})).is_err());
    }

    #[test]
    fn projection_preserves_service_authority_and_normalizes_only_known_paths() {
        let mut mounts = parse_mounts(json!([mount()])).unwrap();
        let projected = project_mount(&mounts[0], true);
        assert_eq!(projected["path"], r"D:\Vault\test5");
        assert_eq!(projected["dismountAllowed"], false);
        assert_eq!(projected["dismountReason"], "administrator_required");
        mounts[0].canonical_container_path = Some("???".into());
        assert!(project_mount(&mounts[0], false)["path"].is_null());
    }

    #[test]
    fn receipts_never_echo_arbitrary_errors_or_claim_global_success() {
        assert_eq!(
            bounded_reason("vault_administrator_required"),
            "administrator_required"
        );
        assert_eq!(
            bounded_reason(
                "service rejected request: vault_policy_access_denied (No access.) [operation 12]"
            ),
            "policy_access_denied"
        );
        assert_eq!(
            bounded_reason("arbitrary text vault_policy_access_denied"),
            "mount_state_unknown"
        );
        assert_eq!(
            bounded_reason(
                "service rejected request: vault_unknown (policy_access_denied) [operation 12]"
            ),
            "mount_state_unknown"
        );
        assert!(
            confirmed_slot_dismount(json!({"state":"unmounted","entry_id":"personal:12"}), 12)
                .is_ok()
        );
        assert!(
            confirmed_slot_dismount(json!({"state":"unmounted","entry_id":"personal:13"}), 12)
                .is_err()
        );
        assert!(confirmed_slot_dismount(json!({"state":"unmounted"}), 12).is_err());
        assert_eq!(
            confirmed_dismount(json!({"state":"denied","reason":"private_owner_required"}))
                .unwrap_err(),
            "private_owner_required"
        );
        assert_eq!(
            confirmed_dismount(json!({"state":"failed","reason":"secret-path"})).unwrap_err(),
            "mount_state_unknown"
        );
        assert!(confirmed_dismount(json!({"status":"dismounted"})).is_err());
        assert_eq!(
            bulk_failure("private_owner_required", 2, 1),
            "vault_bulk_dismount_partial:private_owner_required:2:1"
        );
        assert_eq!(
            bulk_failure("secret-path", 1, 2),
            "vault_bulk_dismount_partial:mount_state_unknown:1:2"
        );
        assert_eq!(bulk_success(2)["scope"], "authorized");
    }

    #[test]
    fn browse_requires_explicit_service_grant_and_exact_letter() {
        let mut mounts = parse_mounts(json!([mount()])).unwrap();
        assert!(authorized_root(&mounts, "V:").is_err());
        mounts[0].browse_allowed = true;
        assert_eq!(authorized_root(&mounts, "v").unwrap(), r"V:\");
        for denied in [r"V:\", r"V:\secret", "D:", "V:;bad"] {
            assert!(authorized_root(&mounts, denied).is_err());
        }
        mounts[0].cleanup_required = true;
        assert!(authorized_root(&mounts, "V:").is_err());
    }
}
