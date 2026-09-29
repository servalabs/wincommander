// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{parse_mounts, PersonalVaultMountedVolume, VAULT_INVENTORY_VERSION};
use serde_json::{json, Value};

/// Negotiate only the read-only wire shape; permissions always come from the service.
pub async fn query_mounts<F, Fut>(mut call: F) -> Result<Vec<PersonalVaultMountedVolume>, String>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let value = match call(
        "svc.vault.list_authorized",
        json!({"personal":true,"inventory_version":VAULT_INVENTORY_VERSION}),
    )
    .await
    {
        Ok(value) => value,
        // The transitional service emitted enriched rows before accepting versions.
        Err(error) if is_query_validation_error(&error) => {
            call("svc.vault.list_authorized", json!({"personal":true})).await?
        }
        Err(error) => return Err(error),
    };
    let rows = value
        .as_array()
        .ok_or("vault_service_personal_status_invalid")?;
    if rows.iter().any(|row| {
        ["browse_allowed", "dismount_allowed", "cleanup_required"]
            .iter()
            .any(|field| row.get(field).and_then(Value::as_bool).is_none())
            || row.get("dismount_reason").is_none()
            || row.get("canonical_container_path").is_none()
    }) {
        return Err("vault_service_personal_status_invalid".into());
    }
    parse_mounts(value)
}

fn is_query_validation_error(error: &str) -> bool {
    error == "vault_validation_failed"
        || error.starts_with("service rejected request: vault_validation_failed (")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_rows() -> Value {
        json!([{"drive_letter":"V:","internal_drive":12,"presentation":"machine",
            "cleanup_required":false,"browse_allowed":true,"dismount_allowed":false,
            "dismount_reason":"administrator_required","canonical_container_path":null}])
    }

    #[tokio::test]
    async fn versioned_query_preserves_explicit_service_decisions() {
        let rows = query_mounts(|verb, args| async move {
            assert_eq!(verb, "svc.vault.list_authorized");
            assert_eq!(args, json!({"personal":true,"inventory_version":2}));
            Ok(current_rows())
        })
        .await
        .unwrap();
        assert!(rows[0].browse_allowed);
        assert!(!rows[0].dismount_allowed);
    }

    #[tokio::test]
    async fn transitional_service_retries_only_query_validation() {
        for error in ["vault_validation_failed",
            "service rejected request: vault_validation_failed (personal mount query is invalid) [operation 4]"] {
            let mut calls = 0;
            let rows = query_mounts(|_, args| {
                calls += 1;
                let result = if calls == 1 {
                    assert_eq!(args["inventory_version"], 2);
                    Err(error.to_owned())
                } else {
                    assert_eq!(args, json!({"personal":true}));
                    Ok(current_rows())
                };
                async move { result }
            }).await.unwrap();
            assert_eq!(calls, 2);
            assert!(!rows[0].dismount_allowed);
        }
    }

    #[tokio::test]
    async fn authorization_transport_and_malformed_results_never_fallback() {
        for error in [
            "vault_not_authorized",
            "vault_service_unavailable",
            "vault_mount_state_unknown",
            "arbitrary vault_validation_failed",
            "service rejected request: vault_not_authorized (vault_validation_failed)",
        ] {
            let mut calls = 0;
            assert_eq!(
                query_mounts(|_, _| {
                    calls += 1;
                    async move { Err(error.to_owned()) }
                })
                .await
                .unwrap_err(),
                error
            );
            assert_eq!(calls, 1);
        }
        let mut calls = 0;
        assert!(query_mounts(|_, _| {
            calls += 1;
            async { Ok(json!({"volumes":[]})) }
        })
        .await
        .is_err());
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn legacy_rows_cannot_synthesize_permission_defaults() {
        for field in [
            "browse_allowed",
            "dismount_allowed",
            "cleanup_required",
            "dismount_reason",
            "canonical_container_path",
        ] {
            let mut rows = current_rows();
            rows[0].as_object_mut().unwrap().remove(field);
            let mut calls = 0;
            assert_eq!(
                query_mounts(|_, _| {
                    calls += 1;
                    let result = if calls == 1 {
                        Err("vault_validation_failed".into())
                    } else {
                        Ok(rows.clone())
                    };
                    async move { result }
                })
                .await
                .unwrap_err(),
                "vault_service_personal_status_invalid"
            );
            assert_eq!(calls, 2);
        }
        assert!(query_mounts(|_, _| async { Ok(json!([])) })
            .await
            .unwrap()
            .is_empty());
    }
}
