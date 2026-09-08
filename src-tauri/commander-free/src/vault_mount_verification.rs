// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 ServaLabs Pvt Ltd. See LICENSE for terms.
//
// The Pro engine verifies its own logon session after mounting. This companion
// command runs in the desktop process itself, which is the same Windows session
// as the user's File Explorer. It prevents a service/session-0 mount from being
// presented as usable to the signed-in person.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use wincmd_shared::diagnostics::{
    DiagnosticEvent, DiagnosticLifecycle, DiagnosticOutcome, DiagnosticPrivacyClass,
    DiagnosticRetryability, DiagnosticSeverity,
};

static NEXT_VERIFICATION_ID: AtomicU64 = AtomicU64::new(1);

fn record_verification(
    outcome: DiagnosticOutcome,
    severity: DiagnosticSeverity,
    error_code: Option<&str>,
) {
    let id = NEXT_VERIFICATION_ID.fetch_add(1, Ordering::Relaxed);
    let event = DiagnosticEvent {
        event_id: format!("evt-vlt-verify-{id}"),
        operation_id: format!("VLT-verify-{id}"),
        parent_operation_id: None,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        component: "desktop".into(),
        feature: "vault".into(),
        action: "verify".into(),
        stage: "windows_readback".into(),
        lifecycle: DiagnosticLifecycle::Verified,
        outcome,
        error_code: error_code.map(str::to_string),
        severity,
        retryability: DiagnosticRetryability::Manual,
        suggested_next_action: if error_code.is_some() {
            "refresh_status"
        } else {
            "none"
        }
        .into(),
        duration_ms: None,
        privacy_class: DiagnosticPrivacyClass::LocalSensitive,
        redacted_context: BTreeMap::new(),
    };
    let _ = crate::diagnostics::record(event);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDriveVerification {
    pub drive: String,
    pub accessible: bool,
}

fn drive_root(raw: &str) -> Result<(char, String), String> {
    let trimmed = raw.trim().strip_suffix(':').unwrap_or(raw.trim());
    let mut chars = trimmed.chars();
    let letter = chars
        .next()
        .filter(char::is_ascii_alphabetic)
        .ok_or_else(|| "Drive must be one letter from A through Z".to_string())?
        .to_ascii_uppercase();
    if chars.next().is_some() {
        return Err("Drive must be one letter from A through Z".to_string());
    }
    Ok((letter, format!("{letter}:\\")))
}

#[tauri::command]
pub fn verify_vault_drive(drive: String) -> Result<VaultDriveVerification, String> {
    let (letter, root) = match drive_root(&drive) {
        Ok(value) => value,
        Err(error) => {
            record_verification(
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Warn,
                Some("VLT.VERIFY.REQUEST_INVALID"),
            );
            return Err(error);
        }
    };
    let metadata = match std::fs::metadata(Path::new(&root)) {
        Ok(value) => value,
        Err(error) => {
            record_verification(
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some("VLT.VERIFY.DRIVE_UNAVAILABLE"),
            );
            return Err(format!(
                "Drive {letter}: is not available in this signed-in Windows session: {error}"
            ));
        }
    };
    if !metadata.is_dir() {
        record_verification(
            DiagnosticOutcome::Failed,
            DiagnosticSeverity::Error,
            Some("VLT.VERIFY.DRIVE_UNAVAILABLE"),
        );
        return Err(format!(
            "Drive {letter}: is not available as an encrypted-volume root in this signed-in Windows session"
        ));
    }
    // Metadata can still be returned for a stale driver slot. Opening the
    // directory is the minimum operation File Explorer needs before it can
    // show the mounted container's contents.
    let mut entries = match std::fs::read_dir(Path::new(&root)) {
        Ok(value) => value,
        Err(error) => {
            record_verification(
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some("VLT.VERIFY.READBACK_FAILED"),
            );
            return Err(format!(
                "Drive {letter}: cannot be opened in this signed-in Windows session: {error}"
            ));
        }
    };
    if let Some(entry) = entries.next() {
        if let Err(error) = entry {
            record_verification(
                DiagnosticOutcome::Failed,
                DiagnosticSeverity::Error,
                Some("VLT.VERIFY.READBACK_FAILED"),
            );
            return Err(format!(
                "Drive {letter}: cannot be read in this signed-in Windows session: {error}"
            ));
        }
    }
    record_verification(DiagnosticOutcome::Succeeded, DiagnosticSeverity::Info, None);
    Ok(VaultDriveVerification {
        drive: format!("{letter}:"),
        accessible: true,
    })
}

#[cfg(test)]
mod tests {
    use super::drive_root;

    #[test]
    fn accepts_one_drive_letter_only() {
        assert_eq!(drive_root("q:").unwrap(), ('Q', "Q:\\".to_string()));
        assert!(drive_root("QQ").is_err());
        assert!(drive_root("C:\\").is_err());
        assert!(drive_root("C::").is_err());
        assert!(drive_root("").is_err());
    }
}
