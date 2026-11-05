// SPDX-License-Identifier: AGPL-3.0-or-later
//! Portable password-protected settings backups with fixed, bounded parameters.

use aes_gcm::{
    aead::{consts::U12, Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const FORMAT: &str = "wincommander-encrypted-settings-v1";
const AAD: &[u8] = b"WinCommander portable settings backup v1\0";
const MAX_PLAINTEXT: usize = 4 * 1024 * 1024;
pub(super) const MAX_FILE: u64 = 6 * 1024 * 1024;
pub(super) const SUPPORTED_SETTINGS_VERSION: u64 = 2;
const AUTH_ERROR: &str = "Password incorrect or backup damaged.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn key(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    // Fixed costs are not read from the backup: hostile files cannot increase them.
    if password.chars().count() < 12 || password.len() > 1024 {
        return Err("Use a backup password of at least 12 characters.".into());
    }
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|_| AUTH_ERROR)?;
    let mut result = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut *result)
        .map_err(|_| AUTH_ERROR)?;
    Ok(result)
}

pub(super) fn encrypt(plaintext: &str, password: &str) -> Result<String, String> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err("Settings exceed the backup size limit.".into());
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| "Backup randomness unavailable.")?;
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| "Backup randomness unavailable.")?;
    let derived = key(password, &salt)?;
    let nonce_ref =
        <&Nonce<U12>>::try_from(nonce.as_slice()).map_err(|_| "Backup nonce unavailable.")?;
    let ciphertext = Aes256Gcm::new((&*derived).into())
        .encrypt(
            nonce_ref,
            Payload {
                msg: plaintext.as_bytes(),
                aad: AAD,
            },
        )
        .map_err(|_| "Settings backup encryption failed.")?;
    serde_json::to_string(&Envelope {
        format: FORMAT.into(),
        salt: B64.encode(salt),
        nonce: B64.encode(nonce),
        ciphertext: B64.encode(ciphertext),
    })
    .map_err(|_| "Settings backup encoding failed.".into())
}

pub(super) fn encrypted(value: &serde_json::Value) -> Result<bool, String> {
    let object = value
        .as_object()
        .ok_or("Settings file must contain a JSON object.")?;
    match value.get("format").and_then(serde_json::Value::as_str) {
        Some(FORMAT) => {
            serde_json::from_value::<Envelope>(value.clone()).map_err(|_| AUTH_ERROR)?;
            Ok(true)
        }
        Some(format) if format.starts_with("wincommander-encrypted-settings-") => {
            Err("This backup requires a newer WinCommander version.".into())
        }
        _ if ["salt", "nonce", "ciphertext"]
            .iter()
            .any(|name| object.contains_key(*name)) =>
        {
            Err(AUTH_ERROR.into())
        }
        _ => {
            // The import writer deserializes v2 directly; v1 root fields would
            // otherwise be ignored and silently replaced with defaults.
            if [
                "privacy",
                "tweaks",
                "network",
                "identity",
                "apps",
                "productivity",
            ]
            .iter()
            .any(|name| object.contains_key(*name))
                || object
                    .get("settingsVersion")
                    .and_then(serde_json::Value::as_u64)
                    .is_none_or(|version| version < SUPPORTED_SETTINGS_VERSION)
            {
                return Err("This older settings backup cannot be imported safely.".into());
            }
            if object
                .get("settingsVersion")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|version| version > SUPPORTED_SETTINGS_VERSION)
            {
                return Err("This settings backup requires a newer WinCommander version.".into());
            }
            if ["app", "ideal", "current", "policy"]
                .iter()
                .any(|name| object.get(*name).is_none_or(|entry| !entry.is_object()))
                || object
                    .get("settingsVersion")
                    .is_some_and(|entry| entry.as_u64().is_none())
            {
                return Err("Settings file does not contain a valid configuration.".into());
            }
            Ok(false)
        }
    }
}

pub(super) fn decrypt(input: &str, password: &str) -> Result<String, String> {
    if input.len() as u64 > MAX_FILE {
        return Err(AUTH_ERROR.into());
    }
    let envelope: Envelope = serde_json::from_str(input).map_err(|_| AUTH_ERROR)?;
    if envelope.format != FORMAT
        || envelope.salt.len() > 24
        || envelope.nonce.len() > 16
        || envelope.ciphertext.len() > ((MAX_PLAINTEXT + 16 + 2) / 3) * 4
    {
        return Err(AUTH_ERROR.into());
    }
    let salt: [u8; 16] = B64
        .decode(&envelope.salt)
        .map_err(|_| AUTH_ERROR)?
        .try_into()
        .map_err(|_| AUTH_ERROR)?;
    let nonce: [u8; 12] = B64
        .decode(&envelope.nonce)
        .map_err(|_| AUTH_ERROR)?
        .try_into()
        .map_err(|_| AUTH_ERROR)?;
    let ciphertext = B64.decode(&envelope.ciphertext).map_err(|_| AUTH_ERROR)?;
    let derived = key(password, &salt).map_err(|_| AUTH_ERROR)?;
    let nonce_ref = <&Nonce<U12>>::try_from(nonce.as_slice()).map_err(|_| AUTH_ERROR)?;
    let plaintext = Aes256Gcm::new((&*derived).into())
        .decrypt(
            nonce_ref,
            Payload {
                msg: &ciphertext,
                aad: AAD,
            },
        )
        .map_err(|_| AUTH_ERROR)?;
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(AUTH_ERROR.into());
    }
    let plaintext = String::from_utf8(plaintext).map_err(|_| AUTH_ERROR)?;
    let value = serde_json::from_str::<serde_json::Value>(&plaintext).map_err(|_| AUTH_ERROR)?;
    if encrypted(&value).map_err(|_| AUTH_ERROR)? {
        return Err(AUTH_ERROR.into());
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_roundtrip_hides_secrets_and_uses_fresh_salt_and_nonce() {
        let original = r#"{"settingsVersion":2,"app":{"flows":[{"secret":"do-not-expose"}]},"ideal":{},"current":{},"policy":{}}"#;
        let first = encrypt(original, "a-long-backup-password").unwrap();
        let second = encrypt(original, "a-long-backup-password").unwrap();
        assert!(!first.contains("do-not-expose"));
        assert_ne!(first, second);
        assert_eq!(decrypt(&first, "a-long-backup-password").unwrap(), original);
    }

    #[test]
    fn rejects_wrong_password_tampering_and_untrusted_costs() {
        let first = encrypt(r#"{"setting":true}"#, "a-long-backup-password").unwrap();
        assert_eq!(
            decrypt(&first, "a-wrong-backup-password").unwrap_err(),
            AUTH_ERROR
        );
        let mut value: serde_json::Value = serde_json::from_str(&first).unwrap();
        value["nonce"] = serde_json::Value::String(B64.encode([0u8; 12]));
        assert_eq!(
            decrypt(&value.to_string(), "a-long-backup-password").unwrap_err(),
            AUTH_ERROR
        );
        value["memory"] = 999999999.into();
        assert_eq!(
            decrypt(&value.to_string(), "a-long-backup-password").unwrap_err(),
            AUTH_ERROR
        );
        let mut changed: serde_json::Value = serde_json::from_str(&first).unwrap();
        changed["salt"] = B64.encode([1u8; 16]).into();
        assert_eq!(
            decrypt(&changed.to_string(), "a-long-backup-password").unwrap_err(),
            AUTH_ERROR
        );
        let mut changed: serde_json::Value = serde_json::from_str(&first).unwrap();
        let mut ciphertext = B64.decode(changed["ciphertext"].as_str().unwrap()).unwrap();
        ciphertext[0] ^= 1;
        changed["ciphertext"] = B64.encode(ciphertext).into();
        assert_eq!(
            decrypt(&changed.to_string(), "a-long-backup-password").unwrap_err(),
            AUTH_ERROR
        );
    }

    #[test]
    fn malformed_envelopes_and_empty_documents_are_never_plaintext_backups() {
        let good = encrypt(r#"{"app":{}}"#, "a-long-backup-password").unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&good).unwrap();
        assert!(encrypted(&envelope).unwrap());
        for field in ["format", "salt", "nonce", "ciphertext"] {
            let mut changed = envelope.clone();
            changed.as_object_mut().unwrap().remove(field);
            assert!(encrypted(&changed).is_err(), "missing {field}");
        }
        let mut changed = envelope.clone();
        changed["format"] = "different".into();
        assert!(encrypted(&changed).is_err());
        assert!(encrypted(&serde_json::json!({})).is_err());
        assert!(encrypted(&serde_json::json!({"app": 1})).is_err());
        assert!(encrypted(&serde_json::json!({"settingsVersion":2})).is_err());
        assert!(encrypted(&serde_json::json!({"settingsVersion":2,"app":{}})).is_err());
        assert!(!encrypted(
            &serde_json::json!({"settingsVersion":2,"app":{},"ideal":{},"current":{},"policy":{}})
        )
        .unwrap());
        for missing in ["app", "ideal", "current", "policy"] {
            let mut partial = serde_json::json!({"settingsVersion":2,"app":{},"ideal":{},"current":{},"policy":{}});
            partial.as_object_mut().unwrap().remove(missing);
            assert!(encrypted(&partial).is_err(), "missing {missing}");
        }
        for legacy in [
            serde_json::json!({"privacy":{"telemetry":{"windowsDisabled":true}}}),
            serde_json::json!({"settingsVersion":1,"tweaks":{"security":{"windowsUpdateDisabled":true}}}),
            serde_json::json!({"settingsVersion":1,"app":{}}),
        ] {
            assert!(encrypted(&legacy)
                .unwrap_err()
                .contains("older settings backup"));
        }
    }

    #[test]
    fn decrypts_independently_generated_argon2id_aes256_gcm_fixture() {
        // Produced by Node's independent Argon2id and AES-GCM implementation.
        let backup = r#"{"format":"wincommander-encrypted-settings-v1","salt":"AAECAwQFBgcICQoLDA0ODw==","nonce":"EBESExQVFhcYGRob","ciphertext":"xviMunp7aCOtqbGICmUdxkAswVogoL6p56mcxirUyR6KbwPV4se9aa33871lyB7OHqG82c4pTU1XQdWdmgNsYDeS8Y9Xl6H/DYcUXcyKCQUoEQ=="}"#;
        assert_eq!(
            decrypt(backup, "a-long-backup-password").unwrap(),
            r#"{"settingsVersion":2,"app":{},"ideal":{},"current":{},"policy":{}}"#
        );
    }
}
