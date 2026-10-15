// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explorer metadata scrub remains a one-shot paid operation before GUI startup.
//! Notification hosting happens only after its verified result is available.
use crate::file_metadata::{ScrubOptions, ScrubReport};
use std::path::Path;

/// Preserve originals and place verified copies in the adjacent `_scrubbed`
/// directory without depending on a running main-window listener.
pub(crate) fn execute_cli(raw_paths: Vec<String>) -> Result<ScrubReport, String> {
    let paths = selected_paths(raw_paths)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "scrub_start_failed".to_string())?;
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
        crate::sidecar::close_pro_session().await;
        result
    })
}

fn selected_paths(raw_paths: Vec<String>) -> Result<Vec<String>, String> {
    let paths: Vec<String> = raw_paths
        .into_iter()
        .filter(|path| !path.starts_with("--"))
        .collect();
    if paths.is_empty() {
        return Err("scrub_selection_missing".into());
    }
    if paths
        .iter()
        .any(|path| !Path::new(path).is_file() && !Path::new(path).is_dir())
    {
        return Err("scrub_selected_item_missing".into());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_selection_never_exposes_the_selected_path() {
        assert_eq!(
            selected_paths(vec![]).unwrap_err(),
            "scrub_selection_missing"
        );
        assert_eq!(
            selected_paths(vec!["Z:\\private\\missing-context-scrub-fixture.png".into()])
                .unwrap_err(),
            "scrub_selected_item_missing"
        );
    }
    #[test]
    fn explorer_scrub_stays_before_single_instance_and_never_calls_messagebox() {
        let source = include_str!("lib.rs");
        assert!(
            source
                .find("context_menu_scrub::execute_cli(paths)")
                .unwrap()
                < source.find("session_instance::acquire(&cli_args)").unwrap()
        );
        assert!(!include_str!("context_menu_scrub.rs").contains(concat!("Message", "BoxW(")));
    }
}
