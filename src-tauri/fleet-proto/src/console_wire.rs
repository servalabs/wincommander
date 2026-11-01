// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wire additions for the Fleet console redesign: Privacy Shield detection
//! events, artifact-level cleanup selection and outcome, and (behind the
//! default-off `sensitive-finder` feature) a sensitive-finder finding.
//!
//! Every type here is closed and content-free. None carries a path (except the
//! feature-gated finder `relative_path`), file content, frame, window title or
//! device name. Serialization is additive: readers must ignore unknown fields,
//! so no type uses `deny_unknown_fields`.

use serde::{Deserialize, Serialize};

/// What a Privacy Shield detection episode observed. The agent never decides
/// severity; the server derives it from the zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum PrivacyShieldDetectionKind {
    /// Nobody in front of the camera.
    NoFace,
    /// Two or more faces.
    MultiFace,
    /// A phone or secondary device.
    Phone,
}

/// Privacy Shield operating mode as the device reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum PrivacyShieldDetectionMode {
    /// Notify only.
    NotifyOnly,
    /// Blur the screen and notify.
    BlurNotify,
}

/// One Privacy Shield detection episode. Carries no frame, biometric value,
/// device name or window title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct PrivacyShieldDetectionEvent {
    pub kind: PrivacyShieldDetectionKind,
    /// RFC3339 time the condition began.
    pub started_at: String,
    /// Whole seconds the condition lasted. Absent while the episode is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
    pub mode: PrivacyShieldDetectionMode,
}

/// Longest id accepted in a cleanup selection.
pub const CLEANUP_ID_MAX_LEN: usize = 64;
/// Most sources one selection may name.
pub const CLEANUP_MAX_SOURCES: usize = 32;
/// Most artifacts one source may name.
pub const CLEANUP_MAX_ARTIFACTS_PER_SOURCE: usize = 16;

/// Artifacts selected inside one cleanup source (for example one browser).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CleanupSourceSelection {
    /// Opaque closed id such as `vivaldi`. Never a path.
    pub source_id: String,
    /// Closed artifact ids such as `history`. Approved ids only.
    pub artifact_ids: Vec<String>,
}

/// Artifact-level cleanup request. The device removes only what is listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CleanupSelection {
    /// Cleanup category such as `browser_traces`.
    pub category: String,
    pub sources: Vec<CleanupSourceSelection>,
}

fn valid_cleanup_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= CLEANUP_ID_MAX_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

impl CleanupSelection {
    /// Structural validation. Ids are lowercase ASCII words, so a path
    /// separator, drive, dot-dot or whitespace can never be expressed. The
    /// device still checks each id against its own closed table.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_cleanup_id(&self.category) {
            return Err("cleanup category is not a closed id");
        }
        if self.sources.is_empty() || self.sources.len() > CLEANUP_MAX_SOURCES {
            return Err("cleanup selection must name 1 to 32 sources");
        }
        let mut seen = std::collections::BTreeSet::new();
        for source in &self.sources {
            if !valid_cleanup_id(&source.source_id) || !seen.insert(source.source_id.as_str()) {
                return Err("cleanup source id invalid or repeated");
            }
            if source.artifact_ids.is_empty()
                || source.artifact_ids.len() > CLEANUP_MAX_ARTIFACTS_PER_SOURCE
            {
                return Err("cleanup source must name 1 to 16 artifacts");
            }
            let mut artifacts = std::collections::BTreeSet::new();
            for artifact in &source.artifact_ids {
                if !valid_cleanup_id(artifact) || !artifacts.insert(artifact.as_str()) {
                    return Err("cleanup artifact id invalid or repeated");
                }
            }
        }
        Ok(())
    }
}

/// One browsable artifact: counts, size and last-write time only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CleanupBrowseArtifact {
    pub artifact_id: String,
    pub item_count: u64,
    pub size_bytes: u64,
    /// RFC3339 newest write time, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_write: Option<String>,
    /// The owning application is running, so deleting would be skipped.
    #[serde(default)]
    pub in_use: bool,
}

/// One browsable source (for example one browser) and its artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CleanupBrowseSource {
    pub source_id: String,
    pub artifacts: Vec<CleanupBrowseArtifact>,
}

/// An artifact that was selected but not removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CleanupSkippedArtifact {
    pub source_id: String,
    pub artifact_id: String,
    /// Closed reason such as `application_open` or `not_present`.
    pub reason: String,
}

/// Result of an artifact-level cleanup, with independent read-back of what
/// remains after the delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CleanupDeleteOutcome {
    /// Artifacts verified gone by a fresh read, grouped by source.
    pub removed: Vec<CleanupSourceSelection>,
    pub skipped: Vec<CleanupSkippedArtifact>,
    /// Fresh browse read taken after the delete.
    pub remaining: Vec<CleanupBrowseSource>,
}

/// A sensitive-finder finding. Never carries the matched value. Compiled only
/// with the `sensitive-finder` feature (off by default) until the owner
/// approves reporting relative paths.
#[cfg(feature = "sensitive-finder")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct SensitiveFinding {
    pub category: String,
    pub relative_path: String,
    pub size: u64,
    /// RFC3339 modified time.
    pub modified: String,
    pub match_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> CleanupSelection {
        CleanupSelection {
            category: "browser_traces".into(),
            sources: vec![CleanupSourceSelection {
                source_id: "vivaldi".into(),
                artifact_ids: vec!["history".into(), "cookies".into()],
            }],
        }
    }

    #[test]
    fn detection_round_trips_and_uses_snake_case() {
        let event = PrivacyShieldDetectionEvent {
            kind: PrivacyShieldDetectionKind::MultiFace,
            started_at: "2026-11-01T10:00:00Z".into(),
            duration_seconds: Some(12),
            mode: PrivacyShieldDetectionMode::BlurNotify,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "multi_face");
        assert_eq!(json["mode"], "blur_notify");
        assert_eq!(
            serde_json::from_value::<PrivacyShieldDetectionEvent>(json).unwrap(),
            event
        );
    }

    #[test]
    fn open_episode_omits_duration_and_unknown_fields_are_ignored() {
        let json = serde_json::json!({
            "kind": "phone", "started_at": "2026-11-01T10:00:00Z",
            "mode": "notify_only", "future_field": {"x": 1}
        });
        let event: PrivacyShieldDetectionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.duration_seconds, None);
        assert!(serde_json::to_value(&event)
            .unwrap()
            .get("duration_seconds")
            .is_none());
        assert!(serde_json::from_value::<PrivacyShieldDetectionEvent>(
            serde_json::json!({"kind":"face_id","started_at":"x","mode":"notify_only"})
        )
        .is_err());
    }

    #[test]
    fn cleanup_selection_round_trips_with_unknown_fields_ignored() {
        let mut json = serde_json::to_value(selection()).unwrap();
        json["extra"] = serde_json::json!(true);
        json["sources"][0]["extra"] = serde_json::json!(1);
        let parsed: CleanupSelection = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, selection());
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn cleanup_selection_rejects_paths_and_repeats() {
        for bad in ["..", "C:\\x", "a/b", "Hist", "", "a b"] {
            let mut s = selection();
            s.sources[0].artifact_ids = vec![bad.into()];
            assert!(s.validate().is_err(), "{bad}");
            let mut s = selection();
            s.sources[0].source_id = bad.into();
            assert!(s.validate().is_err(), "{bad}");
        }
        let mut s = selection();
        s.sources[0].artifact_ids = vec!["history".into(), "history".into()];
        assert!(s.validate().is_err());
        let mut s = selection();
        s.sources.clear();
        assert!(s.validate().is_err());
        let mut s = selection();
        s.sources[0].artifact_ids.clear();
        assert!(s.validate().is_err());
    }

    #[test]
    fn browse_and_outcome_carry_only_counts_and_ids() {
        let outcome = CleanupDeleteOutcome {
            removed: selection().sources,
            skipped: vec![CleanupSkippedArtifact {
                source_id: "chrome".into(),
                artifact_id: "cache_index".into(),
                reason: "application_open".into(),
            }],
            remaining: vec![CleanupBrowseSource {
                source_id: "chrome".into(),
                artifacts: vec![CleanupBrowseArtifact {
                    artifact_id: "history".into(),
                    item_count: 3,
                    size_bytes: 4096,
                    last_write: None,
                    in_use: true,
                }],
            }],
        };
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(!json.contains("path"));
        assert_eq!(
            serde_json::from_str::<CleanupDeleteOutcome>(&json).unwrap(),
            outcome
        );
    }

    #[cfg(feature = "sensitive-finder")]
    #[test]
    fn finding_has_no_matched_value_field() {
        let finding = SensitiveFinding {
            category: "card_number".into(),
            relative_path: "Documents/a.txt".into(),
            size: 10,
            modified: "2026-11-01T10:00:00Z".into(),
            match_count: 2,
        };
        let json = serde_json::to_value(&finding).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["category", "match_count", "modified", "relative_path", "size"]
        );
    }
}
