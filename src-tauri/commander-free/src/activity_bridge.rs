// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use wincmd_shared::maintenance_activity::{
    ActivityArea, ActivityRecordPage, ActivityRemoveResult, ActivityScan,
};

fn validate_ids(scan_id: &str, item_id: &str) -> Result<(), String> {
    if [scan_id, item_id].iter().any(|id| {
        id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err("This activity review is invalid. Scan again.".into());
    }
    Ok(())
}

async fn request<T: DeserializeOwned>(
    area: ActivityArea,
    operation: &str,
    args: Value,
) -> Result<T, String> {
    if crate::settings::is_decoy_mode() {
        return Err("Activity review is unavailable in Decoy mode.".into());
    }
    crate::license::require_paid("activity record review")?;
    let source = match area {
        ActivityArea::Browser => "Browser",
        ActivityArea::Applications => "App",
    };
    let feature = match operation {
        "scan" => format!("Get-{source}Activity"),
        "records" => format!("Get-{source}ActivityRecords"),
        "remove" => {
            if crate::license::is_advanced_mode() {
                return Err("Activity removal is unavailable in Investigator mode.".into());
            }
            ["Clear-", source, "ActivityRecords"].concat()
        }
        _ => return Err("Unsupported activity operation.".into()),
    };
    let result = crate::sidecar::dispatch_paid_command(&feature, args).await?;
    serde_json::from_value(result).map_err(|_| {
        "The activity backend returned an incompatible response. Update both app components.".into()
    })
}

fn valid_record_token(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[tauri::command]
pub async fn activity_scan(area: ActivityArea) -> Result<ActivityScan, String> {
    request(area, "scan", json!({})).await
}

#[tauri::command]
pub async fn activity_records(
    area: ActivityArea,
    scan_id: String,
    item_id: String,
    offset: u64,
) -> Result<ActivityRecordPage, String> {
    validate_ids(&scan_id, &item_id)?;
    request(
        area,
        "records",
        json!({"scanId":scan_id,"itemId":item_id,"offset":offset}),
    )
    .await
}

#[tauri::command]
pub async fn activity_remove(
    area: ActivityArea,
    scan_id: String,
    item_id: String,
    record_ids: Vec<String>,
    all_records: bool,
) -> Result<ActivityRemoveResult, String> {
    validate_ids(&scan_id, &item_id)?;
    if record_ids.len() > 500
        || all_records && !record_ids.is_empty()
        || !all_records && record_ids.is_empty()
        || record_ids.iter().any(|id| !valid_record_token(id))
        || record_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != record_ids.len()
    {
        return Err("Review one data type or up to 500 selected records.".into());
    }
    request(
        area,
        "remove",
        json!({"scanId":scan_id,"itemId":item_id,"recordIds":record_ids,"allRecords":all_records}),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_ids_reject_paths_and_commands() {
        assert!(validate_ids("scan-123", "item-456").is_ok());
        for value in [
            "",
            "C:\\Browser\\History",
            "../History",
            "a;Remove-Item",
            "a b",
        ] {
            assert!(validate_ids(value, "item-456").is_err());
        }
    }
    #[test]
    fn record_capabilities_accept_signed_tokens_but_not_paths_or_commands() {
        let signed = format!("{}.{}", "A_0-".repeat(13), "b2_-".repeat(11));
        assert!(valid_record_token(&signed));
        assert!(valid_record_token("a8c21cc0-57a1-470f-9512-3647aec87df0"));
        for value in [
            "",
            "C:\\Browser\\History",
            "https://example.com",
            "a;Remove-Item",
        ] {
            assert!(!valid_record_token(value));
        }
        assert!(!valid_record_token(&"a".repeat(161)));
    }
}
