//! Explorer's metadata-scrub verb is a one-shot operation.  It deliberately
//! uses the same paid dispatcher as the Share Safely screen, but does not
//! require a webview to be ready before a right-click can safely finish.

use std::collections::BTreeSet;
use std::path::Path;

use crate::file_metadata::{ScrubOptions, ScrubReport};

const OUTPUT_FOLDER_NAME: &str = "_scrubbed";

/// Execute `--scrub <path>...` before the GUI single-instance hand-off.
///
/// The Pro scrubber's default non-replace output is intentionally explicit
/// here: a file selected on the Desktop is written to
/// `Desktop\\_scrubbed\\<name>`, and a selected folder writes its tree to that
/// folder's `_scrubbed` child. Originals are never modified by this verb.
pub(crate) fn execute_cli(raw_paths: Vec<String>) -> Result<Vec<String>, String> {
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
        clean_output_dirs(&report)
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
fn clean_output_dirs(report: &ScrubReport) -> Result<Vec<String>, String> {
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
    Ok(dirs.into_iter().collect())
}

pub(crate) fn show_result(result: Result<Vec<String>, String>) {
    match result {
        Ok(output_dirs) => {
            crate::log_message_src("info", "core", "[ContextScrub] completed clean output");
            show_message(
                "WinCommander Scrub",
                &format!(
                    "Scrubbed file(s) passed post-scrub checks.\r\n\r\nSaved in {OUTPUT_FOLDER_NAME} folder(s):\r\n{}",
                    output_dirs.join("\r\n"),
                ),
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
    use crate::file_metadata::{ScrubError, ScrubResult};

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
            clean_output_dirs(&clean_report()).unwrap(),
            vec!["C:\\Users\\Ada\\Desktop\\_scrubbed"]
        );
    }

    #[test]
    fn context_scrub_never_reports_residual_metadata_as_safe() {
        let mut report = clean_report();
        report.residual_count = 1;
        assert!(clean_output_dirs(&report)
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
        assert!(clean_output_dirs(&report).is_err());
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
