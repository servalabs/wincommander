// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn readable(len: u64) -> Probe {
    Probe {
        len: Some(len),
        open_os_error: None,
        path_chars: 40,
        plain_text: false,
    }
}

fn text(len: u64) -> Probe {
    Probe {
        plain_text: true,
        ..readable(len)
    }
}

#[test]
fn text_files_without_a_known_type_have_nothing_to_remove() {
    for ext in ["", "rdp", "sample", "cfg", "conf", "gitignore"] {
        assert_eq!(
            classify_unprocessed(ext, None, text(12)),
            (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove),
            "{ext:?}"
        );
    }
    // Text formats with their own author or location fields are not assumed clean.
    for ext in ["html", "svg", "xmp", "gpx"] {
        assert_eq!(
            classify_unprocessed(ext, None, text(12)),
            (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat),
            "{ext}"
        );
    }
    assert_eq!(
        classify_unprocessed("", None, readable(12)),
        (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat)
    );
}

#[test]
fn documents_folder_git_and_rdp_files_are_text_but_binary_blobs_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let classify = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let ext = path
            .extension()
            .map_or(String::new(), |e| e.to_string_lossy().to_lowercase());
        classify_unprocessed(&ext, content::detect_file(&path), Probe::of(&path))
    };
    let clean = (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove);
    let unsupported = (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat);
    let rdp = b"\xFF\xFEs\0c\0r\0e\0e\0n\0 \0m\0o\0d\0e\0\r\0\n\0";
    assert_eq!(classify("config", b"[core]\n\tbare = false\n"), clean);
    assert_eq!(classify("COMMIT_EDITMSG", "Café ✈\n".as_bytes()), clean);
    assert_eq!(classify("Default.rdp", rdp), clean);
    assert_eq!(classify("pre-commit.sample", b"#!/bin/sh\n"), clean);
    assert_eq!(classify("index", b"DIRC\0\0\0\x02\0\0\0\x01"), unsupported);
    assert_eq!(classify("blob", b"x\x01\xab\xcd\xef\x90\xfe"), unsupported);
    assert_eq!(classify("pack.idx", b"\xFFtOc\0\0\0\x02"), unsupported);
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
