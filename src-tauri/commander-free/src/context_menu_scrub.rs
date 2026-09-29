//! Explorer's metadata-scrub verb is a one-shot operation.  It deliberately
//! uses the same paid dispatcher as the Share Safely screen, but does not
//! require a webview to be ready before a right-click can safely finish.

use std::collections::BTreeSet;
use std::path::Path;

use crate::file_metadata::{ScrubOptions, ScrubReport, StrippedField};

const OUTPUT_FOLDER_NAME: &str = "_scrubbed";

/// Execute `--scrub <path>...` before the GUI single-instance hand-off.
///
/// The Pro scrubber's default non-replace output is intentionally explicit
/// here: a file selected on the Desktop is written to
/// `Desktop\\_scrubbed\\<name>`, and a selected folder writes its tree to that
/// folder's `_scrubbed` child. Originals are never modified by this verb.
pub(crate) fn execute_cli(raw_paths: Vec<String>) -> Result<ContextScrubSuccess, String> {
    let paths = selected_paths(raw_paths)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Scrub could not start its processing runtime: {error}"))?;

    runtime.block_on(async move {
        let result = crate::file_metadata::scrub_metadata_paths_headless(
            paths,
            Some(ScrubOptions {
                output_dir: None,
                dry_run: false,
                recursive: true,
                paranoid: Default::default(),
                replace_originals: false,
            }),
        )
        .await;
        // A short-lived Explorer invocation has no later work for the helper.
        // Close it before the process exits, just as Safe Copy/Paste does.
        crate::sidecar::close_pro_session().await;
        let report = result?;
        clean_output_summary(&report)
    })
}

fn selected_paths(raw_paths: Vec<String>) -> Result<Vec<String>, String> {
    let paths: Vec<String> = raw_paths
        .into_iter()
        .filter(|path| !path.starts_with("--"))
        .collect();
    if paths.is_empty() {
        return Err("Scrub did not receive a file or folder. Refresh the right-click integration and try again.".into());
    }
    if let Some(missing) = paths
        .iter()
        .find(|path| !Path::new(path).is_file() && !Path::new(path).is_dir())
    {
        return Err(format!("Scrub could not find the selected item: {missing}"));
    }
    Ok(paths)
}

/// A written output is shareable only when every queued item was processed and
/// its post-scrub verification reported no surviving identifying metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextScrubFile {
    name: String,
    removed_kinds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextScrubSuccess {
    output_dirs: Vec<String>,
    files: Vec<ContextScrubFile>,
}

/// The native Explorer popup must be useful without re-disclosing the source
/// metadata. It reports only property *kinds*, never values such as a GPS
/// coordinate, device serial, author name, timestamp, or full input path.
fn sanitized_removed_kinds(fields: &[StrippedField]) -> Vec<String> {
    let mut kinds = BTreeSet::new();
    for field in fields {
        if field.details.is_empty() {
            kinds.insert(field.label.clone());
        } else {
            kinds.extend(field.details.iter().cloned());
        }
    }
    kinds.into_iter().collect()
}

/// A written output is shareable only when every queued item was processed and
/// its post-scrub verification reported no surviving identifying metadata.
fn clean_output_summary(report: &ScrubReport) -> Result<ContextScrubSuccess, String> {
    if !report.errors.is_empty() {
        return Err(format!(
            "Scrub could not complete for {} item(s): {}",
            report.errors.len(),
            report.errors[0].message
        ));
    }
    if report.skipped_count != 0 {
        return Err(format!(
            "Scrub left {} unsupported or mismatched item(s) unchanged. No item is being reported as safe to share.",
            report.skipped_count
        ));
    }
    if report.residual_count != 0 {
        return Err(format!(
            "Post-scrub checking found identifying metadata in {} output item(s). Those files are not safe to share.",
            report.residual_count
        ));
    }
    if report.scrubbed.is_empty() {
        return Err("Scrub did not produce any supported output files.".into());
    }

    let dirs: BTreeSet<String> = report
        .scrubbed
        .iter()
        .filter_map(|result| Path::new(&result.output_path).parent())
        .map(|parent| parent.to_string_lossy().to_string())
        .collect();
    if dirs.is_empty() {
        return Err("Scrub completed without an output folder.".into());
    }
    Ok(ContextScrubSuccess {
        output_dirs: dirs.into_iter().collect(),
        files: report
            .scrubbed
            .iter()
            .map(|result| ContextScrubFile {
                name: Path::new(&result.output_path)
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("scrubbed file"))
                    .to_string_lossy()
                    .to_string(),
                removed_kinds: sanitized_removed_kinds(&result.fields_stripped),
            })
            .collect(),
    })
}

fn success_message(success: &ContextScrubSuccess) -> String {
    let mut message = String::from(
        "Scrubbed file(s) passed post-scrub checks.\r\n\r\nVerified metadata removed:\r\n",
    );
    for file in &success.files {
        message.push_str(&format!("{}\r\n", file.name));
        if file.removed_kinds.is_empty() {
            message.push_str("  • No removable metadata was detected (normal file structure was retained).\r\n");
        } else {
            for kind in &file.removed_kinds {
                message.push_str(&format!("  • {kind}\r\n"));
            }
        }
    }
    message.push_str(&format!(
        "\r\nSaved in {OUTPUT_FOLDER_NAME} folder(s):\r\n{}",
        success.output_dirs.join("\r\n"),
    ));
    message
}

pub(crate) fn show_result(result: Result<ContextScrubSuccess, String>) {
    match result {
        Ok(success) => {
            crate::log_message_src("info", "core", "[ContextScrub] completed clean output");
            show_message(
                "WinCommander Scrub",
                &success_message(&success),
                false,
            );
        }
        Err(error) => {
            crate::log_message_src(
                "warn",
                "core",
                "[ContextScrub] did not produce share-safe output",
            );
            show_message("WinCommander Scrub", &error, true);
        }
    }
}

#[cfg(windows)]
fn show_message(title: &str, message: &str, is_error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
    };
    let text: Vec<u16> = format!("{message}\0").encode_utf16().collect();
    let title: Vec<u16> = format!("{title}\0").encode_utf16().collect();
    let icon = if is_error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | icon,
        );
    }
}

#[cfg(not(windows))]
fn show_message(_title: &str, _message: &str, _is_error: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_metadata::{GpsCoords, ScrubError, ScrubResult};

    fn clean_report() -> ScrubReport {
        ScrubReport {
            scrubbed: vec![ScrubResult {
                input_path: "C:\\Users\\Ada\\Desktop\\a.pdf".into(),
                output_path: "C:\\Users\\Ada\\Desktop\\_scrubbed\\a.pdf".into(),
                file_type: "pdf".into(),
                bytes_in: 1,
                bytes_out: 1,
                fields_stripped: vec![],
                gps_coords: None,
                sample_values: vec![],
                dry_run: false,
                residual_fields: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn clean_context_scrub_reports_the_adjacent_scrubbed_folder() {
        assert_eq!(
            clean_output_summary(&clean_report()).unwrap().output_dirs,
            vec!["C:\\Users\\Ada\\Desktop\\_scrubbed"]
        );
    }

    #[test]
    fn context_scrub_never_reports_residual_metadata_as_safe() {
        let mut report = clean_report();
        report.residual_count = 1;
        assert!(clean_output_summary(&report)
            .unwrap_err()
            .contains("not safe to share"));
    }

    #[test]
    fn context_scrub_never_reports_failed_files_as_safe() {
        let mut report = clean_report();
        report.errors.push(ScrubError {
            input_path: "C:\\Users\\Ada\\Desktop\\a.pdf".into(),
            message: "test failure".into(),
        });
        assert!(clean_output_summary(&report).is_err());
    }

    #[test]
    fn context_scrub_popup_lists_sanitized_verified_removals_not_original_values() {
        let mut report = clean_report();
        report.scrubbed[0].fields_stripped = vec![StrippedField {
            category: "exif".into(),
            label: "Camera & GPS".into(),
            details: vec!["GPS location".into(), "Device make / model".into()],
            bytes: 12,
            is_identifying: true,
        }];
        report.scrubbed[0].sample_values = vec!["GPSLatitude: 37.7749".into()];
        report.scrubbed[0].gps_coords = Some(GpsCoords {
            lat: 37.7749,
            lon: -122.4194,
            label: "37.7749° N, 122.4194° W".into(),
        });

        let message = success_message(&clean_output_summary(&report).unwrap());
        assert!(message.contains("a.pdf"));
        assert!(message.contains("GPS location"));
        assert!(message.contains("Device make / model"));
        assert!(!message.contains("37.7749"));
        assert!(!message.contains("GPSLatitude"));
    }

    #[test]
    fn output_folder_contract_is_stable() {
        assert_eq!(OUTPUT_FOLDER_NAME, "_scrubbed");
    }

    #[test]
    fn explorer_scrub_is_handled_before_single_instance_forwarding() {
        let app_startup = include_str!("lib.rs");
        let direct_runner = app_startup
            .find("context_menu_scrub::execute_cli(paths)")
            .expect("Explorer scrub must have a standalone runner");
        let instance_guard = app_startup
            .find("session_instance::acquire(&cli_args)")
            .expect("single-instance guard must remain present");
        assert!(direct_runner < instance_guard);
    }
}
