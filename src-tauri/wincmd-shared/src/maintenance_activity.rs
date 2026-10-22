// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub enum ActivityArea {
    Browser,
    Applications,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub struct ActivityScan {
    pub scan_id: String,
    pub user_scope: String,
    pub sources: Vec<ActivitySource>,
    pub complete: bool,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub struct ActivitySource {
    pub id: String,
    pub name: String,
    pub profile: Option<String>,
    pub location: String,
    pub items: Vec<ActivityItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub struct ActivityItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub description: String,
    #[cfg_attr(feature = "ts-codegen", ts(type = "number"))]
    pub count: u64,
    pub status: String,
    pub reason: Option<String>,
    pub can_select_records: bool,
    pub can_clear: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub struct ActivityRecord {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub struct ActivityRecordPage {
    pub scan_id: String,
    pub item_id: String,
    pub records: Vec<ActivityRecord>,
    #[cfg_attr(feature = "ts-codegen", ts(type = "number"))]
    pub total_count: u64,
    #[cfg_attr(feature = "ts-codegen", ts(type = "number"))]
    pub offset: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "activity.ts"))]
pub struct ActivityRemoveResult {
    #[cfg_attr(feature = "ts-codegen", ts(type = "number"))]
    pub removed_count: u64,
    #[cfg_attr(feature = "ts-codegen", ts(type = "number"))]
    pub remaining_count: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityRecordsRequest {
    pub scan_id: String,
    pub item_id: String,
    pub offset: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityRemoveRequest {
    pub scan_id: String,
    pub item_id: String,
    pub record_ids: Vec<String>,
    pub all_records: bool,
}
