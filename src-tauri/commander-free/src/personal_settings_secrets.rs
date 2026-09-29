// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::{json, Value};
use std::path::Path;

const LEGACY_FILENAME: &str = "user-settings.dat";
const MAX_SECRET_BYTES: usize = 1_048_576;
const SECRET_PATHS: &[&[&str]] = &[
    &["app", "flowSigningSeedB64"],
    &["app", "flows"],
    &["app", "proFlows"],
    &["app", "contingency"],
    &["ideal", "privacy", "distressPhrases"],
    &["current", "privacy", "distressPhrases"],
];

fn set_path(value: &mut Value, path: &[&str], child: Value) {
    if !value.is_object() {
        *value = json!({});
    }
    if path.len() == 1 {
        value[path[0]] = child;
    } else {
        set_path(&mut value[path[0]], &path[1..], child);
    }
}

/// Separate credential-derived state before constructing the service record.
pub(super) fn split(mut value: Value) -> (Value, Value) {
    let mut secrets = json!({});
    for path in SECRET_PATHS {
        let mut owner = Some(&mut value);
        for segment in &path[..path.len() - 1] {
            owner = owner.and_then(|value| value.get_mut(*segment));
        }
        let Some(owner) = owner.and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(secret) = owner.remove(path[path.len() - 1]) {
            let empty = secret.is_null()
                || secret.as_str().is_some_and(str::is_empty)
                || secret.as_array().is_some_and(Vec::is_empty)
                || (*path == ["app", "contingency"]
                    && serde_json::to_value(crate::flow_engine::ContingencySettings::default())
                        .is_ok_and(|default| secret == default));
            if !empty {
                set_path(&mut secrets, path, secret);
            }
        }
    }
    (value, secrets)
}

pub(super) fn merge(ordinary: &mut Value, secrets: &Value) {
    for path in SECRET_PATHS {
        let mut secret = Some(secrets);
        for segment in *path {
            secret = secret.and_then(|value| value.get(*segment));
        }
        if let Some(secret) = secret {
            set_path(ordinary, path, secret.clone());
        }
    }
}

fn existing_file(directory: &Path) -> Result<Option<&'static str>, String> {
    match std::fs::symlink_metadata(directory.join(LEGACY_FILENAME)) {
        Ok(metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("Personal secrets storage cannot be redirected".to_string());
                }
            }
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("Personal secrets storage is not a regular file".to_string());
            }
            Ok(Some(LEGACY_FILENAME))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Could not inspect personal secrets storage".to_string()),
    }
}

pub(super) fn load() -> Result<Value, String> {
    let directory = crate::paths::user_data_dir()?;
    // load_user_blob opens the key before checking its file: absent secrets
    // must not create a replacement key or disturb unrelated legacy data.
    let Some(filename) = existing_file(&directory)? else {
        return Ok(json!({}));
    };
    let bytes = crate::datastore::load_user_blob(filename, MAX_SECRET_BYTES)?
        .ok_or_else(|| "Personal secrets storage changed while reading".to_string())?;
    parse_secrets(&bytes)
}

fn parse_secrets(bytes: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Personal secrets storage could not be parsed".to_string())?;
    if !value.is_object() {
        return Err("Personal secrets storage is invalid".to_string());
    }
    Ok(split(value).1)
}

pub(super) fn seal(secrets: &Value, require_existing: bool) -> Result<String, String> {
    let bytes = serde_json::to_vec(&split(secrets.clone()).1)
        .map_err(|_| "Personal secrets could not be encoded".to_string())?;
    crate::datastore::encode_user_secrets(&bytes, require_existing)
}

pub(super) fn open(encoded: &str) -> Result<Value, String> {
    parse_secrets(&crate::datastore::decode_user_secrets(encoded)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_owner_preferences_and_removes_only_secret_paths() {
        let input = json!({
            "app": {"theme": "dark", "lockedPanelIds": ["vault"],
                "flowSigningSeedB64": "synthetic-seed"},
            "ideal": {"privacy": {"distressPhrases": [{"hash": "test"}],
                "startupPin": {"realHash": "machine-owned"}}},
            "current": {"privacy": {"distressPhrases": [{"hash": "observed"}],
                "clipboard": {"historyDisabled": true}}}
        });
        let (mut ordinary, secrets) = split(input.clone());
        for pointer in [
            "/app/flowSigningSeedB64",
            "/ideal/privacy/distressPhrases",
            "/current/privacy/distressPhrases",
        ] {
            assert!(ordinary.pointer(pointer).is_none());
            assert_eq!(secrets.pointer(pointer), input.pointer(pointer));
        }
        assert_eq!(ordinary["app"]["lockedPanelIds"], json!(["vault"]));
        assert_eq!(
            ordinary["ideal"]["privacy"]["startupPin"],
            input["ideal"]["privacy"]["startupPin"]
        );
        merge(&mut ordinary, &secrets);
        assert_eq!(ordinary, input);
    }

    #[test]
    fn absent_null_and_empty_secret_defaults_have_the_same_projection() {
        for value in [
            json!({}),
            json!({"app": {"flowSigningSeedB64": null}}),
            json!({"app": {"flowSigningSeedB64": ""},
                "ideal": {"privacy": {"distressPhrases": []}},
                "current": {"privacy": {"distressPhrases": null}}}),
        ] {
            assert_eq!(split(value).1, json!({}));
        }
    }

    #[test]
    fn similarly_named_fields_outside_secret_paths_are_preserved() {
        let input = json!({"flowSigningSeedB64": "ordinary-root-field",
            "ideal": {"distressPhrases": ["ordinary-parent-field"]}});
        let (ordinary, secrets) = split(input.clone());
        assert_eq!(ordinary, input);
        assert_eq!(secrets, json!({}));
    }

    #[test]
    fn flow_and_contingency_credentials_never_enter_ordinary_preferences() {
        let input = json!({"app": {
            "theme": "dark",
            "flows": [{"triggers": [{"type": "WebhookTrigger", "secret": "test-webhook-secret"}]}],
            "proFlows": [{"actions": [{"authorization": "test-api-token"}]}],
            "contingency": {"hmacKey": "test-hmac", "identities": [{"phrase": "test-phrase"}]}
        }});
        let (mut ordinary, secrets) = split(input.clone());
        assert_eq!(ordinary, json!({"app": {"theme": "dark"}}));
        for name in ["flows", "proFlows", "contingency"] {
            assert_eq!(secrets["app"][name], input["app"][name]);
        }
        merge(&mut ordinary, &secrets);
        assert_eq!(ordinary, input);
    }

    #[test]
    fn default_automation_fields_do_not_become_secret_edits() {
        let defaults = json!({"app": {
            "flows": [], "proFlows": [],
            "contingency": crate::flow_engine::ContingencySettings::default()
        }});
        assert_eq!(split(defaults).1, split(json!({})).1);
        let mut customized =
            serde_json::to_value(crate::flow_engine::ContingencySettings::default()).unwrap();
        customized["hmacKey"] = json!("synthetic-key");
        let (_, secrets) = split(json!({"app": {"contingency": customized}}));
        assert_eq!(secrets["app"]["contingency"]["hmacKey"], "synthetic-key");
    }

    #[test]
    fn merging_secrets_cannot_overwrite_other_owner_settings() {
        let mut ordinary = json!({"app": {"theme": "dark"}});
        let secrets = json!({"app": {"theme": "light", "flowSigningSeedB64": "test"},
            "policy": {"managed": false}});
        merge(&mut ordinary, &secrets);
        assert_eq!(
            ordinary,
            json!({"app": {"theme": "dark", "flowSigningSeedB64": "test"}})
        );
    }

    #[test]
    fn legacy_secret_inspection_does_not_create_any_file() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(existing_file(directory.path()).unwrap(), None);
        std::fs::write(directory.path().join(LEGACY_FILENAME), b"legacy").unwrap();
        assert_eq!(
            existing_file(directory.path()).unwrap(),
            Some(LEGACY_FILENAME)
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(
            std::fs::read(directory.path().join(LEGACY_FILENAME)).unwrap(),
            b"legacy"
        );
    }

    #[test]
    fn invalid_legacy_store_is_not_treated_as_absent() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join(LEGACY_FILENAME)).unwrap();
        assert!(existing_file(directory.path()).is_err());
    }

    #[test]
    fn malformed_secret_payloads_fail_without_echoing_content() {
        let error = parse_secrets(b"secret test invalid json").unwrap_err();
        assert!(!error.contains("secret test"));
        assert!(parse_secrets(b"[]").is_err());
    }
}
