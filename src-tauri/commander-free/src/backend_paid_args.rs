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
    // Old clients cannot reactivate permission repair through the string transport.
    if matches_parts(command, &["Mount-~", "Encryption~", "Volume~"]) {
        const REPAIR_PARAM: &str = "RepairCurrentAccountAccess";
        if let Some(raw) = params.get(REPAIR_PARAM) {
            if raw != "false" {
                return Err("Recovered-volume permission repair is not available in this build".into());
            }
            args.remove(REPAIR_PARAM);
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
    fn stego_replacement_confirmation_survives_the_string_transport() {
        for command in ["Attach-StegoContainer", "Restore-StegoContainer", "Refresh-StegoContainer"] {
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
            assert!(paid_command_args(command, &HashMap::new()).unwrap().get("ReplaceExisting").is_none());
        }
    }

    #[test]
    fn stego_replacement_rejects_malformed_confirmation_without_coercing_other_commands() {
        for command in ["Attach-StegoContainer", "Restore-StegoContainer", "Refresh-StegoContainer"] {
            for invalid in ["", "1", "yes", "True", " true ", "null", "{}"] {
                let params = HashMap::from([("ReplaceExisting".into(), invalid.into())]);
                assert!(paid_command_args(command, &params).is_err(), "{command}: {invalid}");
            }
        }
        let params = HashMap::from([("ReplaceExisting".into(), "true".into())]);
        assert_eq!(paid_command_args("Get-EncryptionStatus", &params).unwrap()["ReplaceExisting"].as_str(), Some("true"));
    }

    #[test]
    fn mount_rejects_old_repair_requests_and_omits_false() {
        let params = HashMap::from([(REPAIR_PARAM.to_string(), "true".to_string())]);
        assert!(paid_command_args(MOUNT_COMMAND, &params).is_err());
        let params = HashMap::from([(REPAIR_PARAM.to_string(), "false".to_string())]);
        assert!(paid_command_args(MOUNT_COMMAND, &params).unwrap().get(REPAIR_PARAM).is_none());
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
