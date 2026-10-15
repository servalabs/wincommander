// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed, content-free Syncthing stages. Never persist a raw broker message.
use super::*;

fn broker_stage(message: &str) -> Option<(&'static str, &'static str)> {
    Some(match message {
        "syncthing_vault_binary_unavailable"
        | "syncthing_vault_install_download_failed"
        | "syncthing_vault_install_integrity_failed"
        | "syncthing_vault_install_failed"
        | "syncthing_vault_install_unsupported_platform" => {
            ("prerequisite", "VLT.SYNC.PREREQUISITE_FAILED")
        }
        "syncthing_vault_profile_unavailable"
        | "syncthing_vault_profile_invalid"
        | "syncthing_vault_start_failed" => ("profile_readiness", "VLT.SYNC.PROFILE_UNAVAILABLE"),
        "syncthing_vault_api_unavailable" => ("syncthing_api", "VLT.SYNC.API_UNAVAILABLE"),
        "syncthing_vault_enrollment_readback_failed" => {
            ("folder_readback", "VLT.SYNC.FOLDER_READBACK_FAILED")
        }
        "syncthing_vault_pause_readback_failed" => {
            ("pause_readback", "VLT.SYNC.PAUSE_READBACK_FAILED")
        }
        "syncthing_vault_resume_readback_failed" => {
            ("resume_readback", "VLT.SYNC.RESUME_READBACK_FAILED")
        }
        "syncthing_vault_default_folder_invalid" => {
            ("folder_defaults", "VLT.SYNC.FOLDER_DEFAULTS_INVALID")
        }
        "syncthing_vault_binding_conflict"
        | "syncthing_vault_binding_mismatch"
        | "syncthing_vault_folder_identity_mismatch" => {
            ("binding_validation", "VLT.SYNC.BINDING_REJECTED")
        }
        "syncthing_vault_state_unavailable" => ("owner_state", "VLT.SYNC.STATE_UNAVAILABLE"),
        "syncthing_vault_sync_root_overlap"
        | "syncthing_vault_folder_path_conflict"
        | "syncthing_vault_folder_path_mismatch" => {
            ("folder_path", "VLT.SYNC.FOLDER_PATH_REJECTED")
        }
        "syncthing_vault_recovery_stale" => ("folder_recovery", "VLT.SYNC.RECOVERY_STALE"),
        _ => return None,
    })
}

fn terminal_stage(reason: VaultMountReason) -> (&'static str, &'static str) {
    match reason {
        VaultMountReason::NotAuthorized | VaultMountReason::EntitlementDenied => {
            ("authorization", "VLT.SYNC.NOT_AUTHORIZED")
        }
        VaultMountReason::InvalidRequest => ("validation", "VLT.SYNC.REQUEST_INVALID"),
        VaultMountReason::MountStateUnknown => ("mount_receipt", "VLT.SYNC.MOUNT_UNVERIFIED"),
        VaultMountReason::SyncthingNotEnabled => ("consent", "VLT.SYNC.NOT_ENABLED"),
        VaultMountReason::SyncthingInstallFailed => {
            ("prerequisite", "VLT.SYNC.PREREQUISITE_FAILED")
        }
        VaultMountReason::SyncthingProfileUnavailable => {
            ("profile_or_api", "VLT.SYNC.PROFILE_UNAVAILABLE")
        }
        VaultMountReason::SyncthingRootConflict => ("folder_path", "VLT.SYNC.FOLDER_PATH_REJECTED"),
        VaultMountReason::BrokerUnavailable => ("broker_transport", "VLT.SYNC.BROKER_UNAVAILABLE"),
        VaultMountReason::BrokerHandshakeRejected => {
            ("broker_handshake", "VLT.SYNC.BROKER_UNAVAILABLE")
        }
        VaultMountReason::BrokerReplyRejected => ("broker_receipt", "VLT.SYNC.RECEIPT_UNVERIFIED"),
        VaultMountReason::DismountFailed => ("mount_journal", "VLT.SYNC.MOUNT_JOURNAL_UNCONFIRMED"),
        _ => ("broker_completion", "VLT.SYNC.COMPLETION_UNCONFIRMED"),
    }
}

fn sync_event(
    request_id: u64,
    action: &'static str,
    stage: &'static str,
    code: &'static str,
    succeeded: bool,
    started: Instant,
) -> DiagnosticEvent {
    let mut event = vault_event(
        &format!("VLT-{request_id}"),
        action,
        if succeeded {
            DiagnosticOutcome::Succeeded
        } else {
            DiagnosticOutcome::Failed
        },
        code,
        if succeeded {
            DiagnosticSeverity::Info
        } else {
            DiagnosticSeverity::Error
        },
        false,
        if succeeded {
            "none"
        } else {
            "refresh_sync_status"
        },
        started,
        BTreeMap::new(),
    );
    event.stage = stage.into();
    event
}

pub(crate) fn record_sync_broker_failure(request_id: u64, message: &str) {
    let Some((stage, code)) = broker_stage(message) else {
        return;
    };
    let event = sync_event(request_id, "syncthing", stage, code, false, Instant::now());
    // Unit tests exercise event construction without touching device diagnostics.
    #[cfg(not(test))]
    let _ = record_event(event);
    #[cfg(test)]
    let _ = event;
}

pub(crate) fn record_sync_terminal(
    request_id: u64,
    action: &'static str,
    reason: Option<VaultMountReason>,
    requires_recovery: bool,
    started: Instant,
) {
    let event = terminal_event(request_id, action, reason, requires_recovery, started);
    #[cfg(not(test))]
    let _ = record_event(event);
    #[cfg(test)]
    let _ = event;
}

fn terminal_event(
    request_id: u64,
    action: &'static str,
    reason: Option<VaultMountReason>,
    requires_recovery: bool,
    started: Instant,
) -> DiagnosticEvent {
    let (stage, code) = reason
        .map(terminal_stage)
        .unwrap_or(("confirmed", "VLT.SYNC.CONFIRMED"));
    let mut event = sync_event(request_id, action, stage, code, reason.is_none(), started);
    if reason.is_none() && requires_recovery {
        event.stage = "folder_recovery".into();
        event.error_code = Some("VLT.SYNC.RECOVERY_REQUIRED".into());
        event.outcome = DiagnosticOutcome::Degraded;
        event.suggested_next_action = "review_folder_recovery".into();
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_folder_recovery_never_claims_setup_success() {
        let event = terminal_event(12, "syncthing_setup", None, true, Instant::now());
        assert_eq!(event.outcome, DiagnosticOutcome::Degraded);
        assert_eq!(event.stage, "folder_recovery");
        assert_eq!(
            event.error_code.as_deref(),
            Some("VLT.SYNC.RECOVERY_REQUIRED")
        );
        assert!(event.validate().is_ok());
        let failure = terminal_event(
            12,
            "syncthing_setup",
            Some(VaultMountReason::NotAuthorized),
            true,
            Instant::now(),
        );
        assert_eq!(failure.outcome, DiagnosticOutcome::Failed);
        assert_eq!(failure.stage, "authorization");
    }
    #[test]
    fn sync_codes_accept_only_whole_known_messages_and_omit_private_context() {
        assert_eq!(
            broker_stage("syncthing_vault_api_unavailable"),
            Some(("syncthing_api", "VLT.SYNC.API_UNAVAILABLE"))
        );
        assert_eq!(
            broker_stage("syncthing_vault_enrollment_readback_failed"),
            Some(("folder_readback", "VLT.SYNC.FOLDER_READBACK_FAILED"))
        );
        for raw in [
            "syncthing_vault_api_unavailable private-context",
            "private-account",
            "unknown",
            "",
        ] {
            assert!(broker_stage(raw).is_none());
        }
        let event = sync_event(
            12,
            "syncthing_list",
            "syncthing_api",
            "VLT.SYNC.API_UNAVAILABLE",
            false,
            Instant::now(),
        );
        assert!(event.validate().is_ok());
        assert!(event.redacted_context.is_empty());
        assert_eq!(event.operation_id, "VLT-12");
        assert_eq!(event.retryability, DiagnosticRetryability::Never);
        assert!(!serde_json::to_string(&event)
            .unwrap()
            .contains("private-context"));
    }
    #[test]
    fn service_receipt_failure_is_not_reported_as_profile_readiness() {
        assert_eq!(
            terminal_stage(VaultMountReason::BrokerReplyRejected),
            ("broker_receipt", "VLT.SYNC.RECEIPT_UNVERIFIED")
        );
        assert_eq!(
            terminal_stage(VaultMountReason::NotAuthorized),
            ("authorization", "VLT.SYNC.NOT_AUTHORIZED")
        );
        assert_eq!(
            terminal_stage(VaultMountReason::MountStateUnknown),
            ("mount_receipt", "VLT.SYNC.MOUNT_UNVERIFIED")
        );
    }
}
