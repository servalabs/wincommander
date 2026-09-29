// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{json, Value};
use wincmd_shared::vault_access::PersonalVaultMountedVolume;
use wincmd_shared::vault_inventory::VAULT_INVENTORY_VERSION;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InventoryVersion {
    Legacy,
    Current,
}

pub(super) fn parse_query(args: &Value) -> Option<InventoryVersion> {
    let object = args.as_object()?;
    if object.get("personal") != Some(&Value::Bool(true)) {
        return None;
    }
    if object.len() == 1 {
        return Some(InventoryVersion::Legacy);
    }
    (object.len() == 2
        && object.get("inventory_version").and_then(Value::as_u64)
            == Some(u64::from(VAULT_INVENTORY_VERSION)))
    .then_some(InventoryVersion::Current)
}

// Version selects only the wire shape; callers must authorize every row first.
pub(super) fn reply(
    version: InventoryVersion,
    mounts: &[PersonalVaultMountedVolume],
) -> Result<Value, serde_json::Error> {
    match version {
        InventoryVersion::Current => serde_json::to_value(mounts),
        InventoryVersion::Legacy => Ok(Value::Array(
            mounts
                .iter()
                .map(|mount| {
                    json!({
                        "drive_letter": mount.drive_letter,
                        "internal_drive": mount.internal_drive,
                        "presentation": mount.presentation,
                        "cleanup_required": mount.cleanup_required,
                    })
                })
                .collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincmd_shared::vault_access::{VaultMountReason, VaultPresentation};

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct InstalledLegacyMount {
        drive_letter: String,
        internal_drive: u8,
        presentation: VaultPresentation,
        cleanup_required: bool,
    }

    fn fixture() -> PersonalVaultMountedVolume {
        PersonalVaultMountedVolume {
            drive_letter: "V:".into(),
            internal_drive: 12,
            presentation: VaultPresentation::Machine,
            cleanup_required: false,
            browse_allowed: true,
            dismount_allowed: false,
            dismount_reason: Some(VaultMountReason::AdministratorRequired),
            canonical_container_path: Some(r"D:\Vaults\fixture.hc".into()),
        }
    }

    #[test]
    fn legacy_inventory_request_remains_readable_by_installed_pro() {
        let version = parse_query(&json!({"personal":true})).unwrap();
        let response = reply(version, &[fixture()]).unwrap();
        assert_eq!(response[0].as_object().unwrap().len(), 4);
        assert!(serde_json::from_value::<Vec<InstalledLegacyMount>>(response).is_ok());
    }

    #[test]
    fn enriched_inventory_requires_explicit_version_and_preserves_authority() {
        let version = parse_query(&json!({"personal":true,"inventory_version":2})).unwrap();
        let fixture = fixture();
        let response = reply(version, &[fixture.clone()]).unwrap();
        assert_eq!(
            serde_json::from_value::<Vec<PersonalVaultMountedVolume>>(response).unwrap(),
            vec![fixture]
        );
        assert_eq!(reply(version, &[]).unwrap(), json!([]));
    }

    #[test]
    fn unknown_versions_and_identity_fields_never_select_a_fallback() {
        for invalid in [
            json!({"personal":false}),
            json!({"personal":true,"inventory_version":1}),
            json!({"personal":true,"inventory_version":3}),
            json!({"personal":true,"inventory_version":"2"}),
            json!({"personal":true,"inventory_version":null}),
            json!({"personal":true,"inventory_version":2,"caller_sid":"forged"}),
            json!({"personal":true,"caller_sid":"forged"}),
            json!({"inventory_version":2}),
        ] {
            assert!(parse_query(&invalid).is_none());
        }
    }
}
