use crate::command_strings::matches_parts;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub(super) fn paid_command_args(
    command: &str,
    params: &HashMap<String, String>,
) -> Result<Value, String> {
    let mut args: Map<String, Value> = params
        .iter()
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    let schedule = matches_parts(command, &["Set-~", "Auto~", "Erase~", "Schedule~"])
        || matches_parts(
            command,
            &["Set-~", "Multi~", "User~", "Auto~", "Erase~", "Schedule~"],
        );
    let manual_cleanup = [
        &["Remove-~", "Wlan~", "Profile~"][..],
        &["Clear-~", "All~", "Wireless~", "Profiles~"][..],
        &["Clear-~", "Fixture~", "Wireless~", "Profiles~"][..],
        &["Clear-~", "Browser~", "Footprints~"][..],
        &["Clear-~", "Notepad~", "State~"][..],
        &["Clear-~", "WSL~", "Data~"][..],
        &["Clear-~", "Docker~", "Desktop~", "Data~"][..],
        &["Clear-~", "Virtual~", "Machine~", "Artifacts~"][..],
        &["Clear-~", "Developer~", "Caches~"][..],
        &["Clear-~", "Credential~", "Manager~"][..],
        &["Clear-~", "SSH~", "State~"][..],
        &["Clear-~", "Password~", "Manager~", "Caches~"][..],
    ]
    .iter()
    .any(|parts| matches_parts(command, parts));
    for key in [
        "AllowDefaultExcluded",
        "ManagedByAutoSet",
        "PreserveExisting",
        "RunAsSystem",
    ] {
        if !schedule
            && !(manual_cleanup && matches!(key, "AllowDefaultExcluded" | "ManagedByAutoSet"))
        {
            continue;
        }
        if let Some(raw) = params.get(key) {
            let flag = match raw.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(format!("{key} must be true or false")),
            };
            args.insert(key.into(), Value::Bool(flag));
        }
    }
    if matches_parts(command, &["Attach-~", "Stego~", "Container~"])
        || matches_parts(command, &["Restore-~", "Stego~", "Container~"])
        || matches_parts(command, &["Refresh-~", "Stego~", "Container~"])
    {
        if let Some(raw) = params.get("ReplaceExisting") {
            let confirmed = match raw.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err("ReplaceExisting must be true or false".into()),
            };
            args.insert("ReplaceExisting".into(), Value::Bool(confirmed));
        }
    }
    // Preserve the explicit recovery consent as a boolean. The service still
    // derives the actual SID and only allows it for a local administrator on
    // an unmanaged machine mount.
    if matches_parts(command, &["Mount-~", "Encryption~", "Volume~"]) {
        const REPAIR_PARAM: &str = "RepairCurrentAccountAccess";
        if let Some(raw) = params.get(REPAIR_PARAM) {
            let consent = match raw.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err("RepairCurrentAccountAccess must be true or false".into()),
            };
            args.insert(REPAIR_PARAM.into(), Value::Bool(consent));
        }
    }
    Ok(Value::Object(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNT_COMMAND: &str = "Mount-EncryptionVolume";
    const REPAIR_PARAM: &str = "RepairCurrentAccountAccess";

    #[test]
    fn cleanup_consent_and_schedule_ownership_survive_string_transport() {
        for command in ["Set-AutoEraseSchedule", "Set-MultiUserAutoEraseSchedule"] {
            for flag in [false, true] {
                let params = HashMap::from([
                    ("AllowDefaultExcluded".into(), flag.to_string()),
                    ("ManagedByAutoSet".into(), flag.to_string()),
                    ("PreserveExisting".into(), flag.to_string()),
                    ("RunAsSystem".into(), flag.to_string()),
                    ("TargetUsers".into(), "true".into()),
                ]);
                let args = paid_command_args(command, &params).unwrap();
                for key in [
                    "AllowDefaultExcluded",
                    "ManagedByAutoSet",
                    "PreserveExisting",
                    "RunAsSystem",
                ] {
                    assert_eq!(args[key].as_bool(), Some(flag), "{command}: {key}");
                }
                assert_eq!(args["TargetUsers"].as_str(), Some("true"));
            }
        }
        for command in [
            "Remove-WlanProfile",
            "Clear-AllWirelessProfiles",
            "Clear-FixtureWirelessProfiles",
            "Clear-BrowserFootprints",
            "Clear-NotepadState",
            "Clear-WSLData",
            "Clear-DockerDesktopData",
            "Clear-VirtualMachineArtifacts",
            "Clear-DeveloperCaches",
            "Clear-CredentialManager",
            "Clear-SSHState",
            "Clear-PasswordManagerCaches",
        ] {
            let args = paid_command_args(
                command,
                &HashMap::from([("AllowDefaultExcluded".into(), "true".into())]),
            )
            .unwrap();
            assert_eq!(
                args["AllowDefaultExcluded"].as_bool(),
                Some(true),
                "{command}"
            );
            assert!(paid_command_args(command, &HashMap::new())
                .unwrap()
                .get("AllowDefaultExcluded")
                .is_none());
            for invalid in ["", "1", "True", " true ", "yes"] {
                assert!(paid_command_args(
                    command,
                    &HashMap::from([("AllowDefaultExcluded".into(), invalid.into())])
                )
                .is_err());
            }
        }
    }

    #[test]
    fn cleanup_flags_are_not_coerced_for_unrelated_or_aggregate_commands() {
        let params = HashMap::from([("AllowDefaultExcluded".into(), "true".into())]);
        for command in [
            "Get-EncryptionStatus",
            "run_destruct_step",
            "Invoke-ForensicClearAllUsers",
        ] {
            assert_eq!(
                paid_command_args(command, &params).unwrap()["AllowDefaultExcluded"].as_str(),
                Some("true")
            );
        }
        for command in ["Set-AutoEraseSchedule", "Set-MultiUserAutoEraseSchedule"] {
            for key in [
                "AllowDefaultExcluded",
                "ManagedByAutoSet",
                "PreserveExisting",
                "RunAsSystem",
            ] {
                assert!(
                    paid_command_args(command, &HashMap::from([(key.into(), "yes".into())]))
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn stego_replacement_confirmation_survives_the_string_transport() {
        for command in [
            "Attach-StegoContainer",
            "Restore-StegoContainer",
            "Refresh-StegoContainer",
        ] {
            for consent in [false, true] {
                let params = HashMap::from([
                    ("ReplaceExisting".into(), consent.to_string()),
                    ("Password".into(), "true".into()),
                    ("ContainerPath".into(), "false".into()),
                ]);
                let args = paid_command_args(command, &params).unwrap();
                assert_eq!(args["ReplaceExisting"].as_bool(), Some(consent));
                assert_eq!(args["Password"].as_str(), Some("true"));
                assert_eq!(args["ContainerPath"].as_str(), Some("false"));
            }
            assert!(paid_command_args(command, &HashMap::new())
                .unwrap()
                .get("ReplaceExisting")
                .is_none());
        }
    }

    #[test]
    fn stego_replacement_rejects_malformed_confirmation_without_coercing_other_commands() {
        for command in [
            "Attach-StegoContainer",
            "Restore-StegoContainer",
            "Refresh-StegoContainer",
        ] {
            for invalid in ["", "1", "yes", "True", " true ", "null", "{}"] {
                let params = HashMap::from([("ReplaceExisting".into(), invalid.into())]);
                assert!(
                    paid_command_args(command, &params).is_err(),
                    "{command}: {invalid}"
                );
            }
        }
        let params = HashMap::from([("ReplaceExisting".into(), "true".into())]);
        assert_eq!(
            paid_command_args("Get-EncryptionStatus", &params).unwrap()["ReplaceExisting"].as_str(),
            Some("true")
        );
    }

    #[test]
    fn mount_preserves_explicit_repair_consent_as_a_boolean() {
        let params = HashMap::from([(REPAIR_PARAM.to_string(), "true".to_string())]);
        assert_eq!(
            paid_command_args(MOUNT_COMMAND, &params).unwrap()[REPAIR_PARAM],
            Value::Bool(true)
        );
        let params = HashMap::from([(REPAIR_PARAM.to_string(), "false".to_string())]);
        assert_eq!(
            paid_command_args(MOUNT_COMMAND, &params).unwrap()[REPAIR_PARAM],
            Value::Bool(false)
        );
    }

    #[test]
    fn mount_does_not_invent_missing_repair_consent() {
        let args = paid_command_args(MOUNT_COMMAND, &HashMap::new()).unwrap();
        assert!(args.get(REPAIR_PARAM).is_none());
    }

    #[test]
    fn malformed_repair_consent_is_rejected() {
        for invalid in ["", "1", "yes", "True", " true ", "null", "{}"] {
            let params = HashMap::from([(REPAIR_PARAM.to_string(), invalid.to_string())]);
            assert!(paid_command_args(MOUNT_COMMAND, &params).is_err());
        }
    }

    #[test]
    fn transport_keeps_passwords_and_other_values_verbatim() {
        for password in ["true", "false", "001234", "1e3", "{\"value\":true}"] {
            let params = HashMap::from([
                ("Password".to_string(), password.to_string()),
                ("Path".to_string(), "true".to_string()),
                ("PIM".to_string(), "00042".to_string()),
                (REPAIR_PARAM.to_string(), "false".to_string()),
            ]);
            let args = paid_command_args(MOUNT_COMMAND, &params).unwrap();
            assert_eq!(args["Password"].as_str(), Some(password));
            assert_eq!(args["Path"].as_str(), Some("true"));
            assert_eq!(args["PIM"].as_str(), Some("00042"));
        }
    }

    #[test]
    fn unrelated_commands_keep_existing_string_transport() {
        let params = HashMap::from([(REPAIR_PARAM.to_string(), "true".to_string())]);
        let args = paid_command_args("Get-EncryptionStatus", &params).unwrap();
        assert_eq!(args[REPAIR_PARAM].as_str(), Some("true"));
    }
}
