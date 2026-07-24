// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/metadata_scrubber.rs
//
// ═══════════════════════════════════════════════════════════════════════
// METADATA SCRUBBER — Free-side dispatch wrappers (A-4 module 9)
// ═══════════════════════════════════════════════════════════════════════
//
// The paid scrubber implementation (ExifTool resolution, pre-scan,
// rayon-parallel batch executor, Office docProps ZIP rewrite, paranoid
// post-scrub hardening) lives in commander-pro/src/metadata_scrubber.rs.
// Free retains the two Tauri commands invoked by the Share Safely dialog
// so the frontend interface is unchanged; bodies now thin-dispatch via
// sidecar::dispatch_paid_command.
//
// Progress is reported via the `scrub-progress` event. Pro pushes one
// Notification per file (plus a final 100% tick); Free's sidecar reader
// re-emits each as `AppHandle::emit("scrub-progress", payload)` so the
// existing frontend listener fires unchanged. Payload shape is identical
// to the pre-migration version: `{ current, total, file, dryRun, done? }`.
//
// The struct definitions are kept in this file so serde can decode the
// dispatch return values (ScrubberStatus, ScrubReport) and so the
// frontend's TS types continue to match.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrippedField {
    pub category: String,
    pub label: String,
    pub bytes: u64,
    pub is_identifying: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpsCoords {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubResult {
    pub input_path: String,
    pub output_path: String,
    pub file_type: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub fields_stripped: Vec<StrippedField>,
    pub gps_coords: Option<GpsCoords>,
    #[serde(default)]
    pub sample_values: Vec<String>,
    #[serde(default)]
    pub dry_run: bool,
    /// Identifying metadata still present after the scrub (survivors), plus
    /// can't-fully-remove advisories. Mirrors the Pro producer field; non-empty
    /// means the file is NOT safe to share. See commander-pro metadata_scrubber.
    #[serde(default)]
    pub residual_fields: Vec<StrippedField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubError {
    pub input_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScrubReport {
    pub scrubbed: Vec<ScrubResult>,
    pub errors: Vec<ScrubError>,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
    #[serde(default)]
    pub skipped_count: u32,
    /// Paths of files not processed (unsupported / content-mismatch). Mirrors
    /// the Pro producer field so the UI can name the un-cleaned files.
    #[serde(default)]
    pub skipped_files: Vec<String>,
    /// Count of scrubbed files that still carry identifying metadata. Mirrors
    /// the Pro producer field; drives the report-header warning.
    #[serde(default)]
    pub residual_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubOptions {
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_recursive")]
    pub recursive: bool,
    #[serde(default)]
    pub paranoid: ParanoidOptions,
    #[serde(default)]
    pub replace_originals: bool,
}

fn default_recursive() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParanoidOptions {
    #[serde(default)]
    pub randomize_timestamps: bool,
    #[serde(default)]
    pub strip_alt_streams: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubberStatus {
    pub installed: bool,
    pub version: String,
    pub path: String,
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMANDS — thin dispatch wrappers
// ═══════════════════════════════════════════════════════════════════════

/// Probe whether ExifTool is installed + return its version. Always
/// dispatches to Pro because the engine resolution + version probe is
/// part of the paid scrubber. Status is not licence-gated so the UI
/// can render the "Install the Metadata Scrubber Engine" CTA without
/// teasing a paid feature.
#[tauri::command]
pub async fn get_metadata_scrubber_status() -> Result<ScrubberStatus, String> {
    let v = crate::sidecar::dispatch_paid_command(
        "get_metadata_scrubber_status",
        serde_json::Value::Null,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| format!("scrubber status decode: {}", e))
}

#[tauri::command]
pub async fn scrub_metadata_paths(
    _app: tauri::AppHandle,
    paths: Vec<String>,
    options: Option<ScrubOptions>,
) -> Result<ScrubReport, String> {
    crate::license::require_paid("metadata scrubber")?;
    let args = serde_json::json!({
        "paths": paths,
        "options": options,
    });
    let v = crate::sidecar::dispatch_paid_command("scrub_metadata_paths", args).await?;
    serde_json::from_value(v).map_err(|e| format!("scrub report decode: {}", e))
}
