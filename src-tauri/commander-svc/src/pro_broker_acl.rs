// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-shot service-authorized root-permission phase for a private mount.

use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::File;
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use wincmd_shared::vault_access::{VaultMountReason, VaultPresentation};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub(super) const READY_EVENT: &str = "vault.broker.root_acl_ready";

pub(super) struct RootAclPhase {
    operation_id: u64,
    container_path: String,
    container_identity: String,
    mounted_root_acl_sddl: String,
    // Deny delete sharing until the final result, so identity cannot be recycled.
    _container: File,
    consumed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ready {
    pub operation_id: u64,
    pub internal_drive: u8,
    pub mount_instance_id: i32,
}

impl RootAclPhase {
    pub(super) fn capture(
        feature: &str,
        operation_id: u64,
        presentation: VaultPresentation,
        args: &Value,
    ) -> Result<Option<Self>, VaultMountReason> {
        if feature != "vault.broker.mount"
            || presentation != VaultPresentation::PerUser
            || args.get("personal").and_then(Value::as_bool) != Some(false)
        {
            return Ok(None);
        }
        let denied = || VaultMountReason::BrokerRejected;
        if args.get("operation_id").and_then(Value::as_u64) != Some(operation_id)
            || args.get("presentation").and_then(Value::as_str) != Some("per-user")
        {
            return Err(denied());
        }
        let container_path = args.get("container_path").and_then(Value::as_str).ok_or_else(denied)?;
        let sddl = args.get("mounted_root_acl_sddl").and_then(Value::as_str).ok_or_else(denied)?;
        if container_path.is_empty() || sddl.is_empty() {
            return Err(denied());
        }
        let container = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(container_path)
            .map_err(|_| denied())?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The file owns this handle throughout the synchronous query and phase.
        if unsafe { GetFileInformationByHandle(container.as_raw_handle(), &mut info) } == 0
            || info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        {
            return Err(denied());
        }
        Ok(Some(Self {
            operation_id,
            container_path: container_path.to_owned(),
            container_identity: format!("v:{}:i:{}", info.dwVolumeSerialNumber,
                ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64),
            mounted_root_acl_sddl: sddl.to_owned(),
            _container: container,
            consumed: false,
        }))
    }

    pub(super) fn take_ready(&mut self, payload: Value) -> Result<(Ready, Value), VaultMountReason> {
        let ready = validate_ready(payload, self.operation_id, &mut self.consumed)?;
        let args = json!({
            "operation_id": self.operation_id,
            "internal_drive": ready.internal_drive,
            "mount_instance_id": ready.mount_instance_id,
            "container_path": self.container_path,
            "container_identity": self.container_identity,
            "mounted_root_acl_sddl": self.mounted_root_acl_sddl,
        });
        Ok((ready, args))
    }
}

fn validate_ready(payload: Value, operation_id: u64, consumed: &mut bool) -> Result<Ready, VaultMountReason> {
    if std::mem::replace(consumed, true) {
        return Err(VaultMountReason::BrokerReplyRejected);
    }
    let ready: Ready = serde_json::from_value(payload).map_err(|_| VaultMountReason::BrokerReplyRejected)?;
    if ready.operation_id != operation_id || ready.internal_drive > 25 {
        return Err(VaultMountReason::BrokerReplyRejected);
    }
    Ok(ready)
}

pub(super) fn acknowledgement(ready: &Ready, result: &Result<Value, VaultMountReason>) -> Value {
    let expected = json!({"phase":"root_acl", "internal_drive":ready.internal_drive,
        "mount_instance_id":ready.mount_instance_id, "applied":true});
    json!({"phase":"root_acl", "internal_drive":ready.internal_drive,
        "mount_instance_id":ready.mount_instance_id, "applied":result.as_ref() == Ok(&expected)})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload() -> Value { json!({"operation_id":41,"internal_drive":4,"mount_instance_id":12}) }
    #[test]
    fn only_one_exact_bound_phase_is_accepted() {
        let mut used = false;
        assert!(validate_ready(payload(), 41, &mut used).is_ok());
        assert!(validate_ready(payload(), 41, &mut used).is_err());
    }
    #[test]
    fn rejects_wrong_operation_slot_and_injected_authority() {
        for value in [json!({"operation_id":42,"internal_drive":4,"mount_instance_id":12}),
            json!({"operation_id":41,"internal_drive":26,"mount_instance_id":12}),
            json!({"operation_id":41,"internal_drive":4,"mount_instance_id":12,"container_path":"other"}),
            json!({"operation_id":41,"internal_drive":4})] {
            assert!(validate_ready(value, 41, &mut false).is_err());
        }
    }
    #[test]
    fn helper_must_confirm_the_exact_mount_instance() {
        let ready = validate_ready(payload(), 41, &mut false).unwrap();
        let expected = json!({"phase":"root_acl","internal_drive":4,"mount_instance_id":12,"applied":true});
        assert_eq!(acknowledgement(&ready, &Ok(expected))["applied"], true);
        assert_eq!(acknowledgement(&ready, &Ok(json!({"applied":true})))["applied"], false);
        assert_eq!(acknowledgement(&ready, &Err(VaultMountReason::BrokerUnavailable))["applied"], false);
    }
    #[test]
    fn ordinary_and_machine_mounts_have_no_privileged_phase() {
        assert!(RootAclPhase::capture("vault.broker.mount",41,VaultPresentation::PerUser,
            &json!({"personal":true})).unwrap().is_none());
        assert!(RootAclPhase::capture("vault.broker.mount",41,VaultPresentation::Machine,
            &json!({"personal":false})).unwrap().is_none());
    }
}
