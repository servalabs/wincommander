// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read current owner settings for one authenticated Fleet IPC request.

use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const FEATURE_ID: &str = "free.fleet.local_arming";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Query {
    device_id: String,
    signing_key_pub: String,
}

pub(crate) struct OwnerSettings<'a> {
    pub device_id: &'a str,
    pub fleet_enabled: bool,
    pub dispatch_enabled: bool,
    pub signing_key_pub: &'a str,
    pub enabled: Option<bool>,
}

pub(crate) fn evaluate(args: &Value, owner: OwnerSettings<'_>) -> Result<Value, String> {
    let query: Query = serde_json::from_value(args.clone())
        .map_err(|_| "local_arming_invalid_request".to_string())?;
    if query.device_id.is_empty()
        || query.device_id != owner.device_id
        || query.signing_key_pub.is_empty()
        || (!owner.signing_key_pub.is_empty() && query.signing_key_pub != owner.signing_key_pub)
        || !owner.fleet_enabled
        || !owner.dispatch_enabled
    {
        return Err("local_arming_owner_changed".into());
    }
    // The USB extension has its own gate; it does not authorize this setting read.
    Ok(json!({ "enabled": owner.enabled == Some(true) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(enabled: Option<bool>) -> OwnerSettings<'static> {
        OwnerSettings {
            device_id: "device-a",
            fleet_enabled: true,
            dispatch_enabled: true,
            signing_key_pub: "key-a",
            enabled,
        }
    }
    fn args() -> Value {
        json!({"deviceId":"device-a", "signingKeyPub":"key-a"})
    }

    #[test]
    fn reads_current_setting_without_usb_setup_or_cached_arm() {
        for enabled in [None, Some(false), Some(true), Some(false)] {
            assert_eq!(
                evaluate(&args(), owner(enabled)).unwrap()["enabled"],
                enabled == Some(true)
            );
        }
    }
    #[test]
    fn refuses_changed_owner_management_and_malformed_queries() {
        for field in ["deviceId", "signingKeyPub"] {
            let mut query = args();
            query[field] = json!("other");
            assert!(evaluate(&query, owner(Some(true))).is_err());
        }
        for query in [
            json!({}),
            json!({"deviceId":"device-a", "signingKeyPub":"key-a", "enabled":true}),
        ] {
            assert!(evaluate(&query, owner(Some(true))).is_err());
        }
        let mut settings = owner(Some(true));
        settings.fleet_enabled = false;
        assert!(evaluate(&args(), settings).is_err());
        let mut settings = owner(Some(true));
        settings.dispatch_enabled = false;
        assert!(evaluate(&args(), settings).is_err());
    }
    #[test]
    fn existing_enrollment_without_optional_local_pin_needs_no_new_setup() {
        let mut settings = owner(Some(true));
        settings.signing_key_pub = "";
        assert_eq!(evaluate(&args(), settings).unwrap()["enabled"], true);
    }
}
