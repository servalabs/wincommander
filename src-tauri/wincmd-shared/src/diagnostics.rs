use std::collections::BTreeMap;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

/// Neutral, privacy-classified diagnostic record shared by every WinCommander process.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub struct DiagnosticEvent {
    pub event_id: String,
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_operation_id: Option<String>,
    pub occurred_at: String,
    pub component: String,
    pub feature: String,
    pub action: String,
    pub stage: String,
    pub lifecycle: DiagnosticLifecycle,
    pub outcome: DiagnosticOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub severity: DiagnosticSeverity,
    pub retryability: DiagnosticRetryability,
    pub suggested_next_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub privacy_class: DiagnosticPrivacyClass,
    #[serde(default)]
    pub redacted_context: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum DiagnosticLifecycle {
    Requested,
    Delivered,
    Acknowledged,
    Applying,
    Applied,
    Verified,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum DiagnosticOutcome {
    Started,
    Progress,
    Succeeded,
    Failed,
    Degraded,
    Recovered,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum DiagnosticSeverity {
    Debug,
    Info,
    Warn,
    Error,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum DiagnosticRetryability {
    Never,
    Manual,
    Automatic,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "ipc.ts"))]
pub enum DiagnosticPrivacyClass {
    Public,
    LocalSensitive,
    Restricted,
}

impl DiagnosticEvent {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("event_id", &self.event_id),
            ("operation_id", &self.operation_id),
            ("occurred_at", &self.occurred_at),
            ("component", &self.component),
            ("feature", &self.feature),
            ("action", &self.action),
            ("stage", &self.stage),
            ("suggested_next_action", &self.suggested_next_action),
        ] {
            if value.is_empty() || value.len() > 128 {
                return Err(format!(
                    "diagnostic {name} is required and must be at most 128 characters"
                ));
            }
        }
        if !is_id_token(&self.event_id)
            || !is_id_token(&self.operation_id)
            || self
                .parent_operation_id
                .as_ref()
                .is_some_and(|id| !is_id_token(id))
        {
            return Err(
                "diagnostic event and operation identifiers must be stable tokens".to_string(),
            );
        }
        if self.occurred_at.len() < 10 || !self.occurred_at.is_ascii() {
            return Err("diagnostic occurred_at must begin with an ASCII date".to_string());
        }
        for (name, value) in [
            ("component", &self.component),
            ("feature", &self.feature),
            ("action", &self.action),
            ("stage", &self.stage),
            ("suggested_next_action", &self.suggested_next_action),
        ] {
            if !is_identifier(value) {
                return Err(format!("diagnostic {name} must be a stable identifier"));
            }
        }
        if let Some(code) = &self.error_code {
            if code.len() > 128
                || !code.bytes().all(|byte| {
                    byte.is_ascii_uppercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'.' | b'_' | b'-')
                })
            {
                return Err("diagnostic error_code must be a stable uppercase code".to_string());
            }
        }
        if self.redacted_context.len() > 32 {
            return Err("diagnostic context exceeds 32 fields".to_string());
        }
        Ok(())
    }
}

/// Authenticated, per-record envelope used by diagnostics stores. The caller
/// owns key protection and file permissions; this neutral contract never
/// chooses a platform keystore or a filesystem path.
pub fn seal_diagnostic_record(
    key: &[u8; 32],
    date: &str,
    scope: &str,
    plaintext: &[u8],
    nonce: [u8; 12],
) -> Result<String, String> {
    if !is_date(date) || !is_identifier(scope) || plaintext.len() > 16 * 1024 {
        return Err("invalid diagnostic record envelope input".to_string());
    }
    let cipher = Aes256Gcm::new(key.into());
    let aad = format!("wincommander:diagnostic:{scope}:{date}");
    let nonce =
        Nonce::try_from(nonce.as_slice()).map_err(|_| "invalid diagnostic nonce".to_string())?;
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "diagnostic record encryption failed".to_string())?;
    let mut body = Vec::with_capacity(nonce.len() + ciphertext.len());
    body.extend_from_slice(&nonce);
    body.extend_from_slice(&ciphertext);
    Ok(format!("D1:{date}:{}", B64.encode(body)))
}

pub fn open_diagnostic_record(key: &[u8; 32], scope: &str, record: &str) -> Option<Vec<u8>> {
    let rest = record.strip_prefix("D1:")?;
    let (date, body) = rest.split_once(':')?;
    if !is_date(date) || !is_identifier(scope) {
        return None;
    }
    let payload = B64.decode(body).ok()?;
    let (nonce, ciphertext) = payload.split_at_checked(12)?;
    let cipher = Aes256Gcm::new(key.into());
    let aad = format!("wincommander:diagnostic:{scope}:{date}");
    let nonce = Nonce::try_from(nonce).ok()?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .ok()
}

fn is_date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn is_identifier(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
    })
}

fn is_id_token(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_contract_rejects_unstable_action_and_error_codes() {
        let mut event = DiagnosticEvent {
            event_id: "evt-1".into(),
            operation_id: "VLT-1".into(),
            parent_operation_id: None,
            occurred_at: "2026-09-07T00:00:00Z".into(),
            component: "free".into(),
            feature: "vault".into(),
            action: "mount volume".into(),
            stage: "broker".into(),
            lifecycle: DiagnosticLifecycle::Requested,
            outcome: DiagnosticOutcome::Started,
            error_code: None,
            severity: DiagnosticSeverity::Info,
            retryability: DiagnosticRetryability::Automatic,
            suggested_next_action: "retry".into(),
            duration_ms: None,
            privacy_class: DiagnosticPrivacyClass::LocalSensitive,
            redacted_context: BTreeMap::new(),
        };
        assert!(event.validate().is_err());
        event.action = "mount".into();
        event.error_code = Some("vault failure".into());
        assert!(event.validate().is_err());
    }

    #[test]
    fn sealed_record_rejects_tampering_and_scope_changes() {
        let key = [7; 32];
        let line = seal_diagnostic_record(&key, "2026-09-07", "service", b"safe", [1; 12]).unwrap();
        assert_eq!(
            open_diagnostic_record(&key, "service", &line),
            Some(b"safe".to_vec())
        );
        assert_eq!(open_diagnostic_record(&key, "free", &line), None);
        assert_eq!(
            open_diagnostic_record(&key, "service", &format!("{line}x")),
            None
        );
    }
}
