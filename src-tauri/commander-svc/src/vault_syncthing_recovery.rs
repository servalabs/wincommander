// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::{Deserialize, Serialize};
use wincmd_shared::vault_access::VaultMountReason;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryAction { Inspect, Recreate }

#[derive(Default)]
pub(crate) struct RecoveryOptions {
    pub action: Option<RecoveryAction>,
    pub token: Option<String>,
}

impl RecoveryOptions {
    pub fn validate(&self) -> Result<(), VaultMountReason> {
        match (self.action, self.token.as_deref()) {
            (Some(RecoveryAction::Recreate), Some(token)) if valid_token(token) => Ok(()),
            (None | Some(RecoveryAction::Inspect), None) => Ok(()),
            _ => Err(VaultMountReason::InvalidRequest),
        }
    }

    pub fn add_to(&self, value: &mut serde_json::Value) {
        if let Some(action) = self.action { value["recovery_action"] = serde_json::json!(action); }
        if let Some(token) = &self.token { value["recovery_token"] = serde_json::json!(token); }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryReason {
    ConfigurationMissing, RootMissing, MarkerMissing, ConfirmationRequired,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryRoot {
    pub relative_path: String,
    pub reason: RecoveryReason,
    pub token: String,
}

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn recovery_roots(value: &serde_json::Value) -> Result<Vec<RecoveryRoot>, VaultMountReason> {
    let roots: Vec<RecoveryRoot> = match value.get("recovery_roots") {
        None => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| VaultMountReason::BrokerRejected)?,
    };
    if roots.len() > 32 || roots.iter().any(|root|
        !super::vault_mount::valid_relative_sync_path(&root.relative_path) || !valid_token(&root.token)) {
        return Err(VaultMountReason::BrokerRejected);
    }
    let mut paths = std::collections::BTreeSet::new();
    if roots.iter().any(|root| !paths.insert(root.relative_path.replace('/', "\\").to_lowercase())) {
        return Err(VaultMountReason::BrokerRejected);
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recreation_requires_a_bounded_confirmation_token() {
        for action in [None, Some(RecoveryAction::Inspect)] {
            assert!(RecoveryOptions { action, token: None }.validate().is_ok());
            assert!(RecoveryOptions { action, token: Some("a".repeat(64)) }.validate().is_err());
        }
        for token in [None, Some(String::new()), Some("z".repeat(64))] {
            assert!(RecoveryOptions { action: Some(RecoveryAction::Recreate), token }.validate().is_err());
        }
        assert!(RecoveryOptions { action: Some(RecoveryAction::Recreate), token: Some("a".repeat(64)) }.validate().is_ok());
    }
    #[test]
    fn helper_recovery_receipts_reject_unsafe_or_ambiguous_targets() {
        let root = serde_json::json!({"relative_path":"Sync", "reason":"root_missing", "token":"a".repeat(64)});
        assert_eq!(recovery_roots(&serde_json::json!({"recovery_roots":[root.clone()]})).unwrap().len(), 1);
        for invalid in [serde_json::json!(null), serde_json::json!([root.clone(), root.clone()])] {
            assert!(recovery_roots(&serde_json::json!({"recovery_roots":invalid})).is_err());
        }
        for (field, value) in [("relative_path", "../outside"), ("reason", "invented"), ("token", "bad")] {
            let mut invalid = root.clone(); invalid[field] = serde_json::json!(value);
            assert!(recovery_roots(&serde_json::json!({"recovery_roots":[invalid]})).is_err());
        }
    }
}
