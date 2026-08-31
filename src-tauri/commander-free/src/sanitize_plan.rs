// SPDX-License-Identifier: AGPL-3.0-or-later
//! Non-executable admission and lifecycle contract for hardware sanitize.
//!
//! It reuses the read-only storage probe and the F6 device-bound wipe token.
//! No ATA/NVMe destructive opcode, shell command, or arbitrary device path is
//! represented here. A signed recovery environment is still required to execute.

use crate::recovery_wipe_plan::DiskIdentity;
use crate::storage_probe::DriveCapabilities;
use ed25519_dalek::VerifyingKey;
use wincmd_shared::wipe_auth::verify_wipe_token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeOperation {
    AtaEnhancedErase,
    AtaSanitize,
    NvmeCryptoErase,
    NvmeSanitize,
    NvmeFormatNvm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootEnvironment {
    pub signed_recovery_environment: bool,
    pub one_shot_boot: bool,
    pub external_power: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizePlan<'a> {
    pub fleet_device_id: &'a str,
    pub target: DiskIdentity<'a>,
    pub operation: SanitizeOperation,
    pub approval_nonce: [u8; 32],
    pub approval_expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeLifecycle {
    DeliveredPendingObservation,
    ObservedComplete,
}

fn valid_identity(identity: &DiskIdentity<'_>) -> bool {
    !identity.serial_or_wwn.trim().is_empty()
        && !identity.device_instance_id.trim().is_empty()
        && identity.size_bytes > 0
}

fn supports(capabilities: &DriveCapabilities, operation: SanitizeOperation) -> bool {
    match operation {
        SanitizeOperation::AtaEnhancedErase => {
            capabilities.ata_security_supported && capabilities.ata_enhanced_erase_supported
        }
        SanitizeOperation::AtaSanitize => capabilities.ata_sanitize_supported,
        SanitizeOperation::NvmeCryptoErase => capabilities.nvme_crypto_erase_supported,
        SanitizeOperation::NvmeSanitize => capabilities.nvme_sanitize_supported,
        SanitizeOperation::NvmeFormatNvm => capabilities.nvme_format_supported,
    }
}

/// Build a per-device, per-disk plan only after re-observed hardware facts and
/// an F6 Ed25519 approval token have passed. The plan intentionally cannot run.
#[allow(clippy::too_many_arguments)]
pub fn build_plan<'a>(
    fleet_device_id: &'a str,
    target: DiskIdentity<'a>,
    capabilities: &DriveCapabilities,
    operation: SanitizeOperation,
    environment: BootEnvironment,
    approval_token: &str,
    approval_key: &VerifyingKey,
    now_unix: i64,
) -> Result<SanitizePlan<'a>, String> {
    if fleet_device_id.trim().is_empty() || fleet_device_id.contains(['*', '?', '[', ']']) {
        return Err("hardware sanitize requires one exact Fleet device id".to_string());
    }
    if !valid_identity(&target) || target.disk_number != capabilities.drive_index {
        return Err("hardware sanitize target no longer matches the re-observed disk".to_string());
    }
    if capabilities.is_usb_bridge {
        return Err("hardware sanitize is refused through a USB bridge".to_string());
    }
    if capabilities.ata_frozen {
        return Err("hardware sanitize is refused while ATA security is frozen".to_string());
    }
    if capabilities.probe_error.is_some() || !supports(capabilities, operation) {
        return Err(
            "requested hardware sanitize operation is not advertised by this disk".to_string(),
        );
    }
    if !environment.signed_recovery_environment
        || !environment.one_shot_boot
        || !environment.external_power
    {
        return Err(
            "hardware sanitize requires signed recovery, one-shot boot, and external power"
                .to_string(),
        );
    }
    let approval = verify_wipe_token(approval_token, approval_key, fleet_device_id, now_unix)
        .map_err(|_| "hardware sanitize approval token is invalid for this device".to_string())?;
    Ok(SanitizePlan {
        fleet_device_id,
        target,
        operation,
        approval_nonce: approval.nonce,
        approval_expires_at: approval.expires_at,
    })
}

/// Delivery only means the signed recovery environment accepted a plan. It is
/// deliberately not completion evidence.
pub fn record_delivery(_: &SanitizePlan<'_>) -> SanitizeLifecycle {
    SanitizeLifecycle::DeliveredPendingObservation
}

/// Completion requires a second, matching offline observation. A command exit
/// code or delivery acknowledgement can never promote this state to complete.
pub fn observe_completion(
    plan: &SanitizePlan<'_>,
    observed: &DiskIdentity<'_>,
    offline_verification_passed: bool,
) -> Result<SanitizeLifecycle, String> {
    if &plan.target != observed || !valid_identity(observed) {
        return Err("offline sanitize observation does not match the approved disk".to_string());
    }
    if !offline_verification_passed {
        return Err("sanitize completion remains unverified".to_string());
    }
    Ok(SanitizeLifecycle::ObservedComplete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use wincmd_shared::wipe_auth::issue_wipe_token;

    fn disk(number: u32) -> DiskIdentity<'static> {
        DiskIdentity {
            disk_number: number,
            serial_or_wwn: "wwn-1",
            device_instance_id: "pci-1",
            size_bytes: 1024,
        }
    }
    fn capabilities() -> DriveCapabilities {
        DriveCapabilities {
            drive_index: 2,
            nvme_sanitize_supported: true,
            nvme_crypto_erase_supported: true,
            ..Default::default()
        }
    }
    fn environment() -> BootEnvironment {
        BootEnvironment {
            signed_recovery_environment: true,
            one_shot_boot: true,
            external_power: true,
        }
    }
    fn approved_token(key: &SigningKey) -> String {
        issue_wipe_token("device-1", 60, 100, key)
    }
    fn plan() -> SanitizePlan<'static> {
        let key = SigningKey::from_bytes(&[7; 32]);
        build_plan(
            "device-1",
            disk(2),
            &capabilities(),
            SanitizeOperation::NvmeSanitize,
            environment(),
            &approved_token(&key),
            &key.verifying_key(),
            101,
        )
        .unwrap()
    }

    #[test]
    fn rejects_usb_bridge_frozen_and_unsupported_operations() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let token = approved_token(&key);
        let mut usb = capabilities();
        usb.is_usb_bridge = true;
        assert!(build_plan(
            "device-1",
            disk(2),
            &usb,
            SanitizeOperation::NvmeSanitize,
            environment(),
            &token,
            &key.verifying_key(),
            101
        )
        .is_err());
        let mut frozen = capabilities();
        frozen.ata_frozen = true;
        assert!(build_plan(
            "device-1",
            disk(2),
            &frozen,
            SanitizeOperation::NvmeSanitize,
            environment(),
            &token,
            &key.verifying_key(),
            101
        )
        .is_err());
        assert!(build_plan(
            "device-1",
            disk(2),
            &capabilities(),
            SanitizeOperation::AtaSanitize,
            environment(),
            &token,
            &key.verifying_key(),
            101
        )
        .is_err());
        let mut advertised = capabilities();
        advertised.ata_security_supported = true;
        advertised.ata_enhanced_erase_supported = true;
        advertised.ata_sanitize_supported = true;
        advertised.nvme_format_supported = true;
        for operation in [
            SanitizeOperation::AtaEnhancedErase,
            SanitizeOperation::AtaSanitize,
            SanitizeOperation::NvmeCryptoErase,
            SanitizeOperation::NvmeSanitize,
            SanitizeOperation::NvmeFormatNvm,
        ] {
            assert!(build_plan(
                "device-1",
                disk(2),
                &advertised,
                operation,
                environment(),
                &token,
                &key.verifying_key(),
                101
            )
            .is_ok());
        }
    }

    #[test]
    fn rejects_wrong_disk_and_wildcard_fleet_target() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let token = approved_token(&key);
        assert!(build_plan(
            "device-1",
            disk(3),
            &capabilities(),
            SanitizeOperation::NvmeSanitize,
            environment(),
            &token,
            &key.verifying_key(),
            101
        )
        .is_err());
        for wildcard in ["device?", "device[12]", "device]"] {
            assert!(build_plan(
                wildcard,
                disk(2),
                &capabilities(),
                SanitizeOperation::NvmeSanitize,
                environment(),
                &token,
                &key.verifying_key(),
                101
            )
            .is_err());
        }
        assert!(build_plan(
            "*",
            disk(2),
            &capabilities(),
            SanitizeOperation::NvmeSanitize,
            environment(),
            &token,
            &key.verifying_key(),
            101
        )
        .is_err());
    }

    #[test]
    fn delivery_is_not_false_success_and_observation_rechecks_disk() {
        let plan = plan();
        assert_eq!(
            record_delivery(&plan),
            SanitizeLifecycle::DeliveredPendingObservation
        );
        assert!(observe_completion(&plan, &disk(3), true).is_err());
        assert!(observe_completion(&plan, &disk(2), false).is_err());
        assert_eq!(
            observe_completion(&plan, &disk(2), true).unwrap(),
            SanitizeLifecycle::ObservedComplete
        );
    }
}
