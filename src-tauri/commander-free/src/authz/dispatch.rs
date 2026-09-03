use std::collections::HashMap;

use super::arguments::{canonical_path, free_space_erase_args, secure_erase_args};
use super::capability::{consume_required, DestructiveAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendDispatchPolicy {
    Ordinary,
    Capability(DestructiveAction),
    NativeConfirmation,
    InternalOnly,
}

fn is_internal_lockdown_command(command: &str) -> bool {
    let local_user_removal =
        wincmd_shared::command_strings::join_parts(&["Remove~-", "Local~", "Users~"]);
    command == "run_destruct_step"
        || command == local_user_removal
        || matches!(
            command,
            "Destroy-VeraCryptHeader"
                | "Clear-BitLockerKeyProtectors"
                | "Invoke-7Wipe"
                | "Clear-MFTResidentSlack"
        )
}

/// Security policy for the renderer-facing generic backend dispatcher.
///
/// Ordinary cleanup commands intentionally remain available. Commands in the
/// lockdown cascade are Rust-owned and must never be callable by feature id
/// from the WebView; the two user-facing erase operations require a native,
/// argument-bound capability at this final dispatch boundary.
pub fn backend_dispatch_policy(command: &str) -> BackendDispatchPolicy {
    if is_internal_lockdown_command(command) {
        return BackendDispatchPolicy::InternalOnly;
    }
    match command {
        "Invoke-7Erase" => BackendDispatchPolicy::Capability(DestructiveAction::DiskDelete),
        "Invoke-UnallocatedSpaceErase" => {
            BackendDispatchPolicy::Capability(DestructiveAction::CryptoErase)
        }
        _ => match crate::cli::backend_script_risk(command) {
            Some(crate::cli::Risk::ReadOnly | crate::cli::Risk::Mutating) => {
                BackendDispatchPolicy::Ordinary
            }
            Some(crate::cli::Risk::Destructive) => BackendDispatchPolicy::NativeConfirmation,
            None => BackendDispatchPolicy::InternalOnly,
        },
    }
}

pub fn authorize_backend_dispatch(
    command: &str,
    params: &mut HashMap<String, String>,
) -> Result<(), String> {
    match backend_dispatch_policy(command) {
        BackendDispatchPolicy::Ordinary => Ok(()),
        BackendDispatchPolicy::InternalOnly => Err(format!(
            "{command} is available only through the Rust-owned lockdown path"
        )),
        BackendDispatchPolicy::NativeConfirmation => Err(format!(
            "{command} requires interactive native confirmation"
        )),
        BackendDispatchPolicy::Capability(action) => {
            let token = params.remove("CapabilityToken");
            let canonical_args = match command {
                "Invoke-7Erase" => {
                    if params.keys().any(|key| key != "Path" && key != "Type") {
                        return Err("Invoke-7Erase received unsupported parameters".to_string());
                    }
                    match params.get("Type").map(String::as_str).unwrap_or("File") {
                        "File" => {
                            params.insert("Type".to_string(), "File".to_string());
                        }
                        _ => {
                            return Err("Invoke-7Erase is restricted to file and folder targets"
                                .to_string());
                        }
                    }
                    let path = params
                        .get("Path")
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| "Invoke-7Erase requires Path".to_string())?;
                    let canonical = canonical_path(path);
                    let args = secure_erase_args(&canonical);
                    params.insert("Path".to_string(), canonical);
                    args
                }
                "Invoke-UnallocatedSpaceErase" => {
                    if params
                        .keys()
                        .any(|key| key != "DriveLetter" && key != "MediaType")
                    {
                        return Err(
                            "Invoke-UnallocatedSpaceErase received unsupported parameters"
                                .to_string(),
                        );
                    }
                    free_space_erase_args(
                        params.get("DriveLetter").map(String::as_str).unwrap_or("C"),
                        params
                            .get("MediaType")
                            .map(String::as_str)
                            .unwrap_or("Unknown"),
                    )
                }
                _ => return Err("unsupported destructive backend command".to_string()),
            };
            consume_required(token.as_deref(), action, &canonical_args)
        }
    }
}

/// Registry of every catastrophic Tauri command and the action it performs.
/// A CI gate (tools/ci/check-destructive-authz.sh) and the unit test
/// `registry_covers_known_catastrophic_commands` enforce that every destructive
/// command appears here, so a new one cannot silently ship without an
/// authorization review.
#[allow(dead_code)]
pub const DESTRUCTIVE_COMMANDS: &[(&str, DestructiveAction)] = &[
    ("lockdown", DestructiveAction::SelfDestruct),
    ("full_lockdown", DestructiveAction::SelfDestruct),
    ("run_destruct_step", DestructiveAction::SelfDestruct),
    ("fleet_connect", DestructiveAction::FleetReenroll),
    ("disk_delete_item", DestructiveAction::DiskDelete),
    ("delete_decoy", DestructiveAction::DecoyDelete),
    ("internet_kill_switch_set", DestructiveAction::KillSwitch),
    ("erase_encrypted_container", DestructiveAction::CryptoErase),
];

#[allow(dead_code)]
pub fn action_for(command: &str) -> Option<DestructiveAction> {
    DESTRUCTIVE_COMMANDS
        .iter()
        .find(|(n, _)| *n == command)
        .map(|(_, a)| *a)
}
