use serde::Serialize;

pub(super) fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort_unstable();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON object key serializes"),
                        canonical_json(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).expect("JSON scalar serializes"),
    }
}

#[derive(Clone, Serialize)]
pub struct LockdownPlanSnapshot {
    pub self_destruct: crate::settings::SelfDestructSettings,
    pub shred_mft_slack: bool,
    pub destructive_identities: Vec<(String, wincmd_shared::DestructiveRequestV2)>,
}

impl LockdownPlanSnapshot {
    pub fn from_settings(settings: &crate::settings::AppSettings) -> Result<Self, String> {
        let self_destruct = settings.ideal.privacy.self_destruct.clone();
        let mut destructive_identities = Vec::new();
        for path in self_destruct
            .crypto_erase_veracrypt_paths
            .as_deref()
            .unwrap_or_default()
        {
            let identity =
                crate::path_identity::ExpectedFileIdentity::capture(std::path::Path::new(path))?;
            destructive_identities.push((
                identity.canonical_path().to_string_lossy().into_owned(),
                identity.request(),
            ));
        }
        for drive in self_destruct
            .crypto_erase_bitlocker_drives
            .as_deref()
            .unwrap_or_default()
        {
            destructive_identities.push((
                drive.trim().trim_end_matches('\\').to_ascii_uppercase(),
                crate::path_identity::bitlocker_identity(drive)?,
            ));
        }
        for device in self_destruct
            .crypto_erase_veracrypt_devices
            .as_deref()
            .unwrap_or_default()
        {
            destructive_identities.push((
                device.device_path.clone(),
                crate::path_identity::raw_partition_identity(
                    device.disk_number,
                    device.partition_number,
                    &device.partition_guid,
                    device.offset_bytes,
                    device.size_bytes,
                    &device.disk_unique_id,
                ),
            ));
        }
        Ok(Self {
            self_destruct,
            shred_mft_slack: settings
                .ideal
                .tweaks
                .security
                .shred_mft_slack_enabled
                .unwrap_or(false),
            destructive_identities,
        })
    }

    pub fn destructive_identity(
        &self,
        target: &str,
    ) -> Result<&wincmd_shared::DestructiveRequestV2, String> {
        self.destructive_identities
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(target))
            .map(|(_, identity)| identity)
            .ok_or_else(|| format!("authorized destructive identity is missing for {target}"))
    }
}

fn self_destruct_config_args(plan: &LockdownPlanSnapshot) -> String {
    canonical_json(&serde_json::to_value(plan).expect("Lockdown plan serializes"))
}

pub fn lockdown_args(
    deactivate_license_first: bool,
    shutdown_system: bool,
    plan: &LockdownPlanSnapshot,
) -> String {
    format!(
        "lockdown|deactivate={deactivate_license_first}|shutdown={shutdown_system}|{}",
        self_destruct_config_args(plan)
    )
}

pub fn full_lockdown_args(plan: &LockdownPlanSnapshot) -> String {
    format!("full_lockdown|{}", self_destruct_config_args(plan))
}

pub fn canonical_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}

fn path_bound_args(command: &str, path: &str) -> String {
    let canonical = canonical_path(path);
    let identity = crate::routine_cleaner::file_identity(std::path::Path::new(&canonical));
    serde_json::to_string(&(command, canonical, identity)).expect("path-bound arguments serialize")
}

pub fn disk_delete_args(path: &str) -> String {
    path_bound_args("disk_delete_item", path)
}

pub fn decoy_delete_args(path: &str) -> String {
    path_bound_args("delete_decoy", path)
}

pub fn kill_switch_args(enable: bool) -> String {
    serde_json::to_string(&("internet_kill_switch_set", enable))
        .expect("kill-switch arguments serialize")
}

pub fn secure_erase_args(path: &str) -> String {
    path_bound_args("Invoke-7Erase", path)
}

pub fn free_space_erase_args(drive_letter: &str, media_type: &str) -> String {
    serde_json::to_string(&("Invoke-UnallocatedSpaceErase", drive_letter, media_type))
        .expect("free-space erase arguments serialize")
}
