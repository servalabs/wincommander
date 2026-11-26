// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn readable(len: u64) -> Probe {
    Probe {
        len: Some(len),
        open_os_error: None,
        path_chars: 40,
    }
}

#[test]
fn open_errors_map_to_precise_reasons() {
    assert_eq!(access_reason(Some(32), 40), Some(ScrubReason::Locked));
    assert_eq!(access_reason(Some(5), 40), Some(ScrubReason::AccessDenied));
    assert_eq!(access_reason(Some(206), 40), Some(ScrubReason::TooLongPath));
    assert_eq!(access_reason(Some(3), 300), Some(ScrubReason::TooLongPath));
    assert_eq!(access_reason(Some(2), 40), Some(ScrubReason::ReadFailed));
    assert_eq!(access_reason(None, 400), None);
}

#[test]
fn plain_text_and_empty_files_count_as_clean_not_failures() {
    assert_eq!(
        classify_unprocessed("txt", Some("text"), readable(12)),
        (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove)
    );
    assert_eq!(
        classify_unprocessed("csv", Some("text"), readable(9)),
        (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove)
    );
    assert_eq!(
        classify_unprocessed("jpg", None, readable(0)),
        (ScrubOutcome::AlreadyClean, ScrubReason::EmptyFile)
    );
    assert_eq!(
        classify_unprocessed("txt", Some("jpeg"), readable(10)),
        (ScrubOutcome::NotCleaned, ScrubReason::ContentMismatch)
    );
    assert_eq!(
        classify_unprocessed("jpg", Some("png"), readable(10)),
        (ScrubOutcome::NotCleaned, ScrubReason::ContentMismatch)
    );
    assert_eq!(
        classify_unprocessed("odt", Some("odt"), readable(10)),
        (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat)
    );
    let locked = Probe {
        open_os_error: Some(32),
        ..readable(10)
    };
    assert_eq!(
        classify_unprocessed("jpg", None, locked),
        (ScrubOutcome::Failed, ScrubReason::Locked)
    );
}

#[test]
fn engine_error_messages_map_to_reason_codes() {
    let open = "Hidden Data Remover didn't produce an output file. stderr: Error: Error opening file - C:/x.jpg";
    assert_eq!(
        classify_error(
            open,
            "jpg",
            Probe {
                open_os_error: Some(32),
                ..readable(5)
            }
        )
        .1,
        ScrubReason::Locked
    );
    assert_eq!(
        classify_error(open, "jpg", readable(5)).1,
        ScrubReason::ReadFailed
    );
    assert_eq!(
        classify_error("Error: Format error in file", "jpg", readable(0)),
        (ScrubOutcome::AlreadyClean, ScrubReason::EmptyFile)
    );
    assert_eq!(
        classify_error("stderr: Can't write MP3 files", "mp3", readable(5)),
        (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat)
    );
    assert_eq!(
        classify_error("stderr: Can't write BMP files", "bmp", readable(5)),
        (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove)
    );
    assert_eq!(
        classify_error(
            "Failed to replace original: Access is denied. (os error 5)",
            "jpg",
            readable(5)
        )
        .1,
        ScrubReason::AccessDenied
    );
    assert_eq!(
        classify_error("x (os error 32)", "jpg", readable(5)).1,
        ScrubReason::Locked
    );
    assert_eq!(
        classify_error(
            "Create output dir failed: (os error 206)",
            "jpg",
            readable(5)
        )
        .1,
        ScrubReason::TooLongPath
    );
    assert_eq!(
        classify_error(
            "stderr: Wildcards don't work in the directory specification",
            "jpg",
            readable(5)
        )
        .1,
        ScrubReason::ReadFailed
    );
    assert_eq!(
        classify_error("PDF full rewrite failed: qpdf exit 2", "pdf", readable(5)).1,
        ScrubReason::WriteFailed
    );
}
