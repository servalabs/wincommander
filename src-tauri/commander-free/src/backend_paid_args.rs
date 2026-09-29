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
    // The legacy frontend transport stringifies flags; Pro requires typed consent.
    if matches_parts(command, &["Mount-~", "Encryption~", "Volume~"]) {
        const REPAIR_PARAM: &str = "RepairCurrentAccountAccess";
        if let Some(raw) = params.get(REPAIR_PARAM) {
            let consent = match raw.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(format!("{REPAIR_PARAM} must be a boolean consent value")),
            };
            args.insert(REPAIR_PARAM.to_string(), Value::Bool(consent));
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
    fn mount_repair_consent_reaches_pro_as_a_boolean() {
        for consent in [true, false] {
            let params = HashMap::from([(REPAIR_PARAM.to_string(), consent.to_string())]);
            let args = paid_command_args(MOUNT_COMMAND, &params).unwrap();
            assert_eq!(args[REPAIR_PARAM].as_bool(), Some(consent));
        }
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
                (REPAIR_PARAM.to_string(), "true".to_string()),
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
