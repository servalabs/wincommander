// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::file_metadata::{ScrubError, StrippedField};
use std::path::PathBuf;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\0\x10JFIF\0";

fn result(path: &str, categories: &[&str], residual: bool) -> ScrubResult {
    let field = |c: &str| StrippedField {
        category: c.into(),
        label: c.into(),
        details: vec![],
        bytes: 1,
        is_identifying: true,
    };
    ScrubResult {
        input_path: path.into(),
        output_path: format!("{path}.out"),
        file_type: "jpg".into(),
        bytes_in: 1,
        bytes_out: 1,
        fields_stripped: categories.iter().map(|c| field(c)).collect(),
        gps_coords: None,
        sample_values: vec![],
        dry_run: false,
        residual_fields: if residual {
            vec![field("exif")]
        } else {
            vec![]
        },
        cleaned_as: None,
    }
}

#[test]
fn every_file_in_every_root_is_reported_once_even_when_the_engine_drops_it() {
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("Documents");
    let pics = dir.path().join("Pictures");
    std::fs::create_dir_all(docs.join("_scrubbed")).unwrap();
    std::fs::create_dir_all(pics.join("Trip ✈ 東京")).unwrap();
    let put = |p: PathBuf, b: &[u8]| {
        std::fs::write(&p, b).unwrap();
        p.to_string_lossy().to_string()
    };
    let photo = put(pics.join("Trip ✈ 東京").join("café 1.jpg"), JPEG);
    let renamed = put(pics.join("WhatsApp Image.jpg"), PNG);
    let empty = put(pics.join("empty.jpg"), b"");
    let notes = put(docs.join("notes.txt"), b"plain\r\n");
    let clean = put(docs.join("clean.jpg"), JPEG);
    let dropped = put(docs.join("dropped.odt"), b"PK\x03\x04");
    put(docs.join("_scrubbed").join("old.jpg"), JPEG);
    let roots = vec![
        docs.to_string_lossy().to_string(),
        pics.to_string_lossy().to_string(),
        dir.path().join("missing").to_string_lossy().to_string(),
    ];
    let report = ScrubReport {
        scrubbed: vec![
            result(&photo, &["exif", "xmp"], false),
            result(&clean, &[], false),
        ],
        errors: vec![ScrubError {
            input_path: empty.clone(),
            message: "Error: Format error in file".into(),
        }],
        skipped_count: 2,
        skipped_files: vec![renamed.clone(), notes.clone()],
        ..Default::default()
    };
    let (files, summary) = build(&roots, true, &report);
    let by = |p: &str| {
        files
            .iter()
            .find(|f| f.path == p)
            .unwrap_or_else(|| panic!("missing {p}"))
    };
    assert_eq!(files.len(), 7, "{files:#?}");
    assert_eq!(
        (
            by(&photo).outcome,
            by(&photo).scrubbed_index,
            by(&photo).root.as_str()
        ),
        (ScrubOutcome::Cleaned, Some(0), roots[1].as_str())
    );
    assert_eq!(by(&photo).removed_categories, vec!["exif", "xmp"]);
    assert_eq!(
        (
            by(&renamed).reason,
            by(&renamed).detected_type.as_deref(),
            by(&renamed).type_mismatch
        ),
        (Some(ScrubReason::ContentMismatch), Some("png"), true)
    );
    assert_eq!(by(&empty).reason, Some(ScrubReason::EmptyFile));
    assert_eq!(
        (by(&notes).outcome, by(&notes).reason),
        (
            ScrubOutcome::AlreadyClean,
            Some(ScrubReason::NothingToRemove)
        )
    );
    assert_eq!(by(&clean).reason, Some(ScrubReason::NothingToRemove));
    assert_eq!(
        (by(&dropped).reason, by(&dropped).message.as_deref()),
        (Some(ScrubReason::UnsupportedFormat), Some(NOT_REPORTED))
    );
    assert_eq!(by(&roots[2]).reason, Some(ScrubReason::ReadFailed));
    assert!(files.iter().all(|f| !f.path.contains("_scrubbed")));
    assert_eq!(
        (
            summary[0].total,
            summary[0].already_clean,
            summary[0].not_cleaned
        ),
        (3, 2, 1)
    );
    assert_eq!(
        (
            summary[1].total,
            summary[1].cleaned,
            summary[1].not_cleaned,
            summary[1].already_clean
        ),
        (3, 1, 1, 1)
    );
    assert!(!summary[2].exists && summary[2].total == 1);
}

#[test]
fn documents_summary_skips_git_internals_and_counts_text_as_nothing_to_remove() {
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("Documents");
    let hooks = docs.join("Obsidian Vault").join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("pre-commit.sample"), b"#!/bin/sh\n").unwrap();
    std::fs::write(hooks.parent().unwrap().join("config"), b"[core]\n").unwrap();
    let rdp = docs.join("Default.rdp");
    std::fs::write(&rdp, b"\xFF\xFEs\0c\0r\0e\0e\0n\0\r\0\n\0").unwrap();
    let photo = docs.join("photo.jpg");
    std::fs::write(&photo, JPEG).unwrap();
    let roots = vec![docs.to_string_lossy().to_string()];
    let photo = photo.to_string_lossy().to_string();
    let rdp = rdp.to_string_lossy().to_string();
    // An engine that predates text detection still names the .rdp as skipped.
    let report = ScrubReport {
        scrubbed: vec![result(&photo, &["exif"], false)],
        skipped_count: 1,
        skipped_files: vec![rdp.clone()],
        ..Default::default()
    };
    let (files, summary) = build(&roots, true, &report);
    assert_eq!(files.len(), 2, "{files:#?}");
    assert!(files.iter().all(|f| !f.path.contains(".git")));
    let rdp_item = files.iter().find(|f| f.path == rdp).unwrap();
    assert_eq!(
        (rdp_item.outcome, rdp_item.reason),
        (ScrubOutcome::AlreadyClean, Some(ScrubReason::NothingToRemove))
    );
    assert_eq!(
        (summary[0].total, summary[0].cleaned, summary[0].already_clean, summary[0].not_cleaned),
        (2, 1, 1, 0)
    );
}

#[test]
fn engine_paths_outside_the_listing_are_still_reported_and_residuals_kept() {
    let roots = vec![r"C:\nowhere-root".to_string()];
    let ghost = r"C:\nowhere-root\sub\photo.jpg";
    let report = ScrubReport {
        scrubbed: vec![result(ghost, &["exif"], true)],
        ..Default::default()
    };
    let (files, summary) = build(&roots, true, &report);
    assert_eq!(files.len(), 2);
    let item = files.iter().find(|f| f.path == ghost).unwrap();
    assert_eq!(
        (item.outcome, item.root.as_str()),
        (ScrubOutcome::Residual, roots[0].as_str())
    );
    assert_eq!(summary[0].residual, 1);
}

/// Corpus check driven by tools/test-scrub-formats.ps1 (WC_SCRUB_CORPUS).
#[test]
#[ignore]
fn corpus_every_file_has_an_expected_outcome() {
    let base = PathBuf::from(std::env::var("WC_SCRUB_CORPUS").expect("WC_SCRUB_CORPUS"));
    let expected: Vec<(String, String, String)> =
        std::fs::read_to_string(base.join("expected.tsv"))
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                (c[0].to_string(), c[1].to_string(), c[2].to_string())
            })
            .collect();
    let roots = vec![
        base.join("Documents").to_string_lossy().to_string(),
        base.join("Pictures").to_string_lossy().to_string(),
    ];
    let (files, summary) = build(&roots, true, &ScrubReport::default());
    assert_eq!(files.len(), expected.len(), "{files:#?}");
    for (rel, reason, detected) in expected {
        let path = base.join(&rel).to_string_lossy().to_string();
        let item = files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("not reported: {rel}"));
        let got = serde_json::to_value(item.reason).unwrap();
        assert_eq!(got.as_str().unwrap_or(""), reason, "{rel}");
        assert_eq!(
            item.detected_type.as_deref().unwrap_or("-"),
            detected,
            "{rel}"
        );
    }
    assert_eq!(
        summary.iter().map(|s| s.total as usize).sum::<usize>(),
        files.len()
    );
}

/// Applies a real engine report (WC_SCRUB_ENGINE_REPORT) to a corpus and writes outcomes.json.
#[test]
#[ignore]
fn corpus_engine_report_names_every_file() {
    let base = PathBuf::from(std::env::var("WC_SCRUB_CORPUS").expect("WC_SCRUB_CORPUS"));
    let raw = std::fs::read_to_string(
        std::env::var("WC_SCRUB_ENGINE_REPORT").expect("WC_SCRUB_ENGINE_REPORT"),
    )
    .unwrap();
    let report: ScrubReport = serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap();
    let roots = vec![
        base.join("Documents").to_string_lossy().to_string(),
        base.join("Pictures").to_string_lossy().to_string(),
    ];
    let (files, roots) = build(&roots, true, &report);
    let reported = report.scrubbed.len() + report.errors.len() + report.skipped_files.len();
    assert!(files.len() >= reported);
    assert!(files.iter().all(|f| f.outcome == ScrubOutcome::Cleaned
        || f.outcome == ScrubOutcome::Residual
        || f.reason.is_some()));
    let out = serde_json::json!({ "files": files, "roots": roots });
    std::fs::write(
        base.join("outcomes.json"),
        serde_json::to_string_pretty(&out).unwrap(),
    )
    .unwrap();
}
