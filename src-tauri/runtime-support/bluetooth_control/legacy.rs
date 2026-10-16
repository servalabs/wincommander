// SPDX-License-Identifier: AGPL-3.0-or-later
//! Old adapter receipts are inspected, never adopted, edited or removed.
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Legacy {
    version: u32,
    blocked: bool,
    records: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: String,
    identity: String,
    confirmed: bool,
}

pub(super) fn active(bytes: &[u8]) -> Result<bool, String> {
    let value: Legacy =
        serde_json::from_slice(bytes).map_err(|_| super::engine::STATE_ERROR.to_string())?;
    if bytes.len() > 1_048_576
        || value.version != 1
        || value.records.len() > 128
        || value.records.iter().any(|record| {
            record.id.is_empty()
                || record.id.len() > 1024
                || record.identity.is_empty()
                || record.identity.len() > 8192
        })
    {
        return Err(super::engine::STATE_ERROR.into());
    }
    // Reading the flag ensures both old confirmed and pending records are
    // handled as legacy evidence, without granting any restore authority.
    let _confirmed = value
        .records
        .iter()
        .filter(|record| record.confirmed)
        .count();
    Ok(value.blocked || !value.records.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_empty_inactive_v1_can_be_ignored() {
        assert!(!super::active(br#"{"version":1,"blocked":false,"records":[]}"#).unwrap());
        assert!(super::active(br#"{"version":1,"blocked":true,"records":[]}"#).unwrap());
        for confirmed in [false, true] {
            let bytes = format!(
                r#"{{"version":1,"blocked":false,"records":[{{"id":"test","identity":"test","confirmed":{confirmed}}}]}}"#
            );
            assert!(super::active(bytes.as_bytes()).unwrap());
        }
        assert!(super::active(br#"{"version":3,"blocked":false,"records":[]}"#).is_err());
        assert!(super::active(b"invalid").is_err());
    }
}
