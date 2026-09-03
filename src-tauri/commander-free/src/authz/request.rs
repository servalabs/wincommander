use serde::Deserialize;

use super::arguments::{
    canonical_json, canonical_path, decoy_delete_args, disk_delete_args, free_space_erase_args,
    full_lockdown_args, kill_switch_args, lockdown_args, secure_erase_args, LockdownPlanSnapshot,
};
use super::capability::{mint, DestructiveAction};
use super::trusted::native_confirm;

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum DestructiveRequest {
    Lockdown {
        #[serde(rename = "deactivateLicenseFirst")]
        deactivate_license_first: bool,
        #[serde(rename = "shutdownSystem")]
        shutdown_system: bool,
    },
    FullLockdown,
    DiskDeleteItem {
        path: String,
    },
    DeleteDecoy {
        path: String,
    },
    InternetKillSwitchSet {
        enable: bool,
    },
    SecureErase {
        path: String,
    },
    FreeSpaceErase {
        #[serde(rename = "driveLetter")]
        drive_letter: String,
        #[serde(rename = "mediaType")]
        media_type: String,
    },
    SelectiveCryptoErase {
        target: crate::selective_erase::EraseTargetInput,
    },
}

impl DestructiveRequest {
    pub(super) fn action(&self) -> DestructiveAction {
        match self {
            Self::Lockdown { .. } | Self::FullLockdown => DestructiveAction::SelfDestruct,
            Self::DiskDeleteItem { .. } => DestructiveAction::DiskDelete,
            Self::DeleteDecoy { .. } => DestructiveAction::DecoyDelete,
            Self::InternetKillSwitchSet { .. } => DestructiveAction::KillSwitch,
            Self::SecureErase { .. } => DestructiveAction::DiskDelete,
            Self::FreeSpaceErase { .. } => DestructiveAction::CryptoErase,
            Self::SelectiveCryptoErase { .. } => DestructiveAction::CryptoErase,
        }
    }

    pub(super) fn canonical_args(
        &self,
        plan: Option<&LockdownPlanSnapshot>,
    ) -> Result<String, String> {
        Ok(match self {
            Self::Lockdown {
                deactivate_license_first,
                shutdown_system,
            } => lockdown_args(
                *deactivate_license_first,
                *shutdown_system,
                plan.expect("Lockdown plan was loaded"),
            ),
            Self::FullLockdown => full_lockdown_args(plan.expect("full Lockdown plan was loaded")),
            Self::DiskDeleteItem { path } => disk_delete_args(path),
            Self::DeleteDecoy { path } => decoy_delete_args(path),
            Self::InternetKillSwitchSet { enable } => kill_switch_args(*enable),
            Self::SecureErase { path } => secure_erase_args(path),
            Self::FreeSpaceErase {
                drive_letter,
                media_type,
            } => free_space_erase_args(drive_letter, media_type),
            Self::SelectiveCryptoErase { target } => {
                return crate::selective_erase::canonical_erase_args(target)
            }
        })
    }

    pub(super) fn confirmation_detail(&self, plan: Option<&LockdownPlanSnapshot>) -> String {
        match self {
            Self::Lockdown {
                deactivate_license_first,
                shutdown_system,
            } => lockdown_confirmation(
                plan.expect("Lockdown plan was loaded"),
                *deactivate_license_first,
                *shutdown_system,
            ),
            Self::FullLockdown => {
                full_lockdown_confirmation(plan.expect("full Lockdown plan was loaded"))
            }
            Self::DiskDeleteItem { path } => format!(
                "Permanently delete this disk item?\n\nTarget: {}",
                safe_detail(&canonical_path(path))
            ),
            Self::DeleteDecoy { path } => format!(
                "Permanently delete this decoy file?\n\nTarget: {}",
                safe_detail(&canonical_path(path))
            ),
            Self::InternetKillSwitchSet { enable } => format!(
                "Turn the Internet Kill Switch {} now? This changes machine-wide network access.",
                if *enable { "ON" } else { "OFF" }
            ),
            Self::SecureErase { path } => format!(
                "Securely overwrite and permanently delete this item?\n\nTarget: {}",
                safe_detail(&canonical_path(path))
            ),
            Self::FreeSpaceErase { drive_letter, .. } => format!(
                "Overwrite free space on drive {}? Deleted-data recovery will be intentionally prevented.",
                safe_detail(drive_letter)
            ),
            Self::SelectiveCryptoErase { target } => format!(
                "Permanently crypto-erase this encrypted target?\n\nKind: {}\nPath: {}\nMount: {}\nOS/system target: {}",
                safe_detail(&target.kind),
                target
                    .path
                    .as_deref()
                    .map(canonical_path)
                    .map(|path| safe_detail(&path))
                    .unwrap_or_else(|| "none".to_string()),
                target
                    .mount_point
                    .as_deref()
                    .or(target.mount_letter.as_deref())
                    .map(safe_detail)
                    .unwrap_or_else(|| "none".to_string()),
                yes_no(crate::selective_erase::is_os_target(
                    &target.kind,
                    target.mount_point.as_deref(),
                    target.path.as_deref(),
                    &crate::selective_erase::system_drive(),
                )),
            ),
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn listed(values: Option<&Vec<String>>) -> String {
    match values {
        Some(values) if !values.is_empty() => values
            .iter()
            .map(|value| safe_detail(value))
            .collect::<Vec<_>>()
            .join("; "),
        _ => "none".to_string(),
    }
}

fn lockdown_confirmation(
    plan: &LockdownPlanSnapshot,
    deactivate_license_first: bool,
    shutdown_system: bool,
) -> String {
    let config = &plan.self_destruct;
    format!(
        "Run Lockdown with this fixed plan?\n\nDeactivate licence: {}\nShutdown Windows: {}\nFolder targets: {}\nLocal users: {}\nVeraCrypt containers: {}\nBitLocker drives: {}\nMFT resident-slack wipe: {}\nStep overrides: {}",
        yes_no(deactivate_license_first),
        yes_no(shutdown_system),
        listed(config.shred_folders.as_ref()),
        listed(config.users_to_remove.as_ref()),
        listed(config.crypto_erase_veracrypt_paths.as_ref()),
        listed(config.crypto_erase_bitlocker_drives.as_ref()),
        yes_no(plan.shred_mft_slack),
        config
            .steps
            .as_ref()
            .map(|steps| canonical_json(&serde_json::to_value(steps).expect("steps serialize")))
            .unwrap_or_else(|| "documented defaults".to_string()),
    )
}

fn full_lockdown_confirmation(plan: &LockdownPlanSnapshot) -> String {
    let config = &plan.self_destruct;
    let mut detail = lockdown_confirmation(
        plan,
        config.deactivate_license_first.unwrap_or(false),
        config.shutdown_system.unwrap_or(true),
    );
    let devices = config
        .crypto_erase_veracrypt_devices
        .as_ref()
        .filter(|devices| !devices.is_empty())
        .map(|devices| {
            devices
                .iter()
                .map(|device| {
                    format!(
                        "{} (disk {}, partition {}, id {})",
                        safe_detail(&device.device_path),
                        device.disk_number,
                        device.partition_number,
                        safe_detail(&device.disk_unique_id)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_else(|| "none".to_string());
    detail.push_str(&format!("\nVeraCrypt device targets: {devices}"));
    detail
}

pub(super) fn safe_detail(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

/// Verify a PIN against the configured startup-PIN hashes, returning the mode
/// ("real" | "decoy" | "destroy" | "open" | "wrong"). Mirrors
/// startup_auth::verify_startup_pin but is callable synchronously from Rust.
fn verify_pin_local(pin: &str) -> Result<&'static str, String> {
    let s = crate::settings::read_settings().map_err(|e| format!("read settings: {e}"))?;
    let sp = &s.ideal.privacy.startup_pin;
    crate::startup_auth::verify_pin_mode_limited(
        pin,
        sp.real_hash.as_deref(),
        sp.decoy_hash.as_deref(),
        sp.destroy_hash.as_deref(),
    )
}

/// Mint a capability after verifying the requested authorization path.
/// - A renderer-entered PIN is rate-limited and must resolve to the mode required
///   for `action` (`destroy` for the self-destruct/wipe family;
///   `real`/`destroy` otherwise). It is not an unforgeable factor against a
///   compromised WebView.
/// - If no PIN is supplied, fall back to a Rust-initiated native confirm dialog
///   that a WebView script can display but cannot answer.
pub(super) async fn request_destructive_confirmation_impl(
    app: tauri::AppHandle,
    request: DestructiveRequest,
    pin: Option<String>,
) -> Result<String, String> {
    let action = request.action();
    let lockdown_plan = if matches!(
        request,
        DestructiveRequest::Lockdown { .. } | DestructiveRequest::FullLockdown
    ) {
        let settings =
            crate::settings::read_settings().map_err(|error| format!("read settings: {error}"))?;
        Some(LockdownPlanSnapshot::from_settings(&settings)?)
    } else {
        None
    };
    let args_canonical = request.canonical_args(lockdown_plan.as_ref())?;
    let confirmation_detail = request.confirmation_detail(lockdown_plan.as_ref());
    let needs_destroy = matches!(
        action,
        DestructiveAction::SelfDestruct
            | DestructiveAction::RemoveUsers
            | DestructiveAction::CryptoErase
    );
    let authorized = match pin {
        Some(p) if !p.trim().is_empty() => {
            let mode = verify_pin_local(&p)?;
            if needs_destroy {
                mode == "destroy"
            } else {
                mode == "real" || mode == "destroy"
            }
        }
        _ => native_confirm(&app, &confirmation_detail).await,
    };
    if !authorized {
        return Err("confirmation failed".into());
    }
    Ok(mint(action, &args_canonical))
}
