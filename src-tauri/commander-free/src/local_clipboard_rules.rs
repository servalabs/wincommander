// SPDX-License-Identifier: AGPL-3.0-or-later
//! Encrypted, per-Windows-user persistence for personally authored clipboard
//! rules. Fleet policy intentionally has no path through this module.

use clipboard_guard_helper::policy::ClipboardPolicyResponse;

const POLICY_FILE: &str = "clipboard-guard-rules.dat";
const MAX_POLICY_BYTES: usize = 128 * 1024;

fn empty_policy() -> ClipboardPolicyResponse {
    ClipboardPolicyResponse {
        policy_version: 0,
        rules: Vec::new(),
    }
}

fn decode_policy(bytes: &[u8]) -> Result<ClipboardPolicyResponse, String> {
    let policy: ClipboardPolicyResponse = serde_json::from_slice(bytes)
        .map_err(|_| "local clipboard rules are invalid".to_string())?;
    if policy.policy_version != 0 {
        return Err("local clipboard rules are invalid".to_string());
    }
    Ok(policy)
}

fn encode_policy(policy: &ClipboardPolicyResponse) -> Result<Vec<u8>, String> {
    serde_json::to_vec(policy).map_err(|_| "local clipboard rules could not be stored".to_string())
}

fn activate(policy: ClipboardPolicyResponse) -> Result<ClipboardPolicyResponse, String> {
    crate::paste_monitor::replace_local_clipboard_guard_rules(policy)
        .map_err(|_| "local clipboard rules were rejected".to_string())
}

/// Loads personal rules, activates them with the immutable local builtins, and
/// returns only the editable portion to the UI. Missing storage is the normal
/// first-run state. A malformed/tampered blob activates the safe empty custom
/// source so stale process state cannot keep an untrusted rule live.
#[tauri::command]
pub fn load_local_clipboard_guard_rules() -> Result<ClipboardPolicyResponse, String> {
    let policy = match crate::datastore::load_user_blob(POLICY_FILE, MAX_POLICY_BYTES) {
        Ok(Some(bytes)) => match decode_policy(&bytes) {
            Ok(policy) => policy,
            Err(_) => {
                let _ = activate(empty_policy());
                return Err("local clipboard rules could not be loaded".to_string());
            }
        },
        Ok(None) => empty_policy(),
        Err(_) => {
            let _ = activate(empty_policy());
            return Err("local clipboard rules could not be loaded".to_string());
        }
    };

    activate(policy.clone())
        .map_err(|_| "local clipboard rules could not be activated".to_string())?;
    Ok(policy)
}

/// Validates and activates a personal policy before persisting it. If the
/// atomic disk replacement fails, the previously active personal policy is
/// restored so runtime and disk never disagree.
#[tauri::command]
pub fn save_local_clipboard_guard_rules(policy: ClipboardPolicyResponse) -> Result<(), String> {
    let previous = activate(policy.clone())?;
    let bytes = match encode_policy(&policy) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = activate(previous);
            return Err(error);
        }
    };
    if crate::datastore::save_user_blob(POLICY_FILE, &bytes, MAX_POLICY_BYTES).is_err() {
        activate(previous)
            .map_err(|_| "local clipboard rules could not be restored".to_string())?;
        return Err("local clipboard rules could not be stored".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nonlocal_policy_version() {
        assert!(decode_policy(br#"{"policy_version":1,"rules":[]}"#).is_err());
    }

    #[test]
    fn malformed_policy_fails_without_echoing_input() {
        let input = br#"{"not_a_policy":"clipboard text must stay private"}"#;
        let error = decode_policy(input).unwrap_err();
        assert!(!error.contains("clipboard text"));
    }

    #[test]
    fn empty_policy_serializes_with_local_version() {
        let decoded = decode_policy(&encode_policy(&empty_policy()).unwrap()).unwrap();
        assert_eq!(decoded.policy_version, 0);
        assert!(decoded.rules.is_empty());
    }
}
