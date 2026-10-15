// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::file_metadata::StrippedField;

const KNOWN_DETAILS: &[&str] = &[
    "GPS location coordinates",
    "Device serial number",
    "Camera or device model",
    "Lens information",
    "Capture or edit date and time",
    "Author, account, or organisation identity",
    "Document author (Creator)",
    "Last modified by",
    "Company",
    "Manager",
    "Copyright or owner information",
    "Location or address",
    "Comments, captions, or description",
    "Keywords, subject, or rating",
    "Website or source link",
    "Embedded preview image",
    "Embedded document preview",
    "Custom document properties",
    "Revision author, account, or edit date",
    "Windows tracking or download marker",
    "Title",
    "Producer",
    "Software",
    "File creation, modification, or access time",
];

fn field(details: &[&str]) -> StrippedField {
    StrippedField {
        category: "private-category".into(),
        label: "private-stream-or-label".into(),
        details: details.iter().map(|detail| (*detail).into()).collect(),
        bytes: 10,
        is_identifying: true,
    }
}

fn file(details: &[&str]) -> ScrubResult {
    ScrubResult {
        input_path: "private-input-name".into(),
        output_path: "private-output-path".into(),
        file_type: "private-file-type".into(),
        bytes_in: 10,
        bytes_out: 8,
        fields_stripped: vec![field(details)],
        gps_coords: Some(crate::file_metadata::GpsCoords {
            lat: 37.7749,
            lon: -122.4194,
            label: "private-location-label".into(),
        }),
        sample_values: vec!["private-author-value".into()],
        dry_run: false,
        residual_fields: vec![],
    }
}

fn report(files: Vec<ScrubResult>) -> ScrubReport {
    ScrubReport {
        scrubbed: files,
        ..Default::default()
    }
}

#[test]
fn confirmed_receipt_uses_only_static_kinds_and_deduplicates_aliases() {
    let summary = confirmed_summary(&report(vec![file(&[
        "GPS location coordinates",
        "GPS location coordinates",
        "Document author (Creator)",
        "Company",
        "Capture or edit date and time",
        "private-tag-name",
        "GPS location coordinates private-value",
    ])]));
    assert_eq!(
        summary,
        "File 1 — Removed: GPS location, author/account/org, capture/edit dates."
    );
    for forbidden in ["private", "37.7749", "122.4194"] {
        assert!(!summary.contains(forbidden));
    }
}

#[test]
fn unconfirmed_outputs_never_supply_removal_claims_and_ordinals_are_preserved() {
    let mut preview = file(&["GPS location coordinates"]);
    preview.dry_run = true;
    let mut residual = file(&["GPS location coordinates"]);
    residual
        .residual_fields
        .push(field(&["GPS location coordinates"]));
    let mut unwritten = file(&["GPS location coordinates"]);
    unwritten.output_path.clear();
    let summary = confirmed_summary(&report(vec![
        preview,
        residual,
        unwritten,
        file(&["Title"]),
    ]));
    assert_eq!(summary, "File 4 — Removed: document title.");
    assert!(!summary.contains("GPS"));
}

#[test]
fn aggregate_only_residual_warning_prevents_unsafe_file_attribution() {
    let mut report = report(vec![file(&["GPS location coordinates"])]);
    report.residual_count = 1;
    assert!(confirmed_summary(&report).is_empty());
}

#[test]
fn unknown_or_empty_details_do_not_invent_removed_kinds() {
    let mut unknown = file(&["private-tag-name"]);
    unknown.fields_stripped[0].category = "exif".into();
    unknown.fields_stripped[0].label = "Camera & GPS".into();
    let summary = confirmed_summary(&report(vec![unknown, file(&[])]));
    assert_eq!(
        summary,
        "File 1 — No removal details confirmed.\nFile 2 — No removal details confirmed."
    );
    assert!(!summary.contains("Removed:"));
    assert!(!summary.contains("GPS"));
    assert!(!summary.contains("private"));
}

#[test]
fn nonidentifying_headers_do_not_become_removal_receipts() {
    let mut header = file(&["GPS location coordinates"]);
    header.fields_stripped[0].is_identifying = false;
    assert_eq!(
        confirmed_summary(&report(vec![header])),
        "File 1 — No removal details confirmed."
    );
}

#[test]
fn timestamps_are_changed_not_removed() {
    assert_eq!(
        confirmed_summary(&report(vec![file(&[
            "File creation, modification, or access time"
        ])])),
        "File 1 — Changed: file timestamps."
    );
}

#[test]
fn later_files_keep_all_recognized_kinds_in_a_bounded_union() {
    let mut files = vec![file(&[]); 3];
    files.push(file(&["GPS location coordinates"]));
    files.push(file(&["Device serial number", "GPS location coordinates"]));
    let summary = confirmed_summary(&report(files));
    assert!(summary.ends_with("2 more verified file(s) — Removed: GPS location, device serial."));
    assert_eq!(summary.lines().count(), 4);
    assert!(!summary.contains("File 4"));
    assert!(!summary.contains("private"));
}

#[test]
fn all_recognized_kinds_are_retained_with_a_1500_byte_bound() {
    let summary = confirmed_summary(&report(vec![file(KNOWN_DETAILS); 10]));
    assert!(
        summary.len() <= 1500,
        "receipt length was {}",
        summary.len()
    );
    for line in summary.lines() {
        for kind in [
            "GPS location",
            "device serial",
            "device model",
            "lens information",
            "capture/edit dates",
            "author/account/org",
            "copyright/owner",
            "location/address",
            "comments/captions",
            "keywords/subject",
            "source links",
            "embedded previews",
            "custom properties",
            "revision authors/dates",
            "Windows tracking",
            "document title",
            "software information",
            "Changed: file timestamps",
        ] {
            assert!(line.contains(kind), "missing static kind {kind}");
        }
    }
    assert!(summary
        .lines()
        .last()
        .unwrap()
        .starts_with("7 more verified file(s)"));
    let mut kinds = Kinds::default();
    kinds.include(&file(KNOWN_DETAILS));
    let description = kinds.describe();
    let first = format!("File {} — {description}.", usize::MAX);
    let rest = format!("{} more verified file(s) — {description}.", usize::MAX);
    let largest_possible = [first.clone(), first.clone(), first, rest].join("\n");
    assert!(largest_possible.len() <= 1500);
}

#[test]
fn empty_report_has_no_removed_claim() {
    assert!(confirmed_summary(&ScrubReport::default()).is_empty());
}
