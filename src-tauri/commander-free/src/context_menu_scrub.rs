// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explorer metadata scrub remains a one-shot paid operation before GUI startup.
//! Only a verified result can be published; owned cleanup still finishes before exit.
use crate::context_scrub_outcome::ScrubOutcome;
use crate::file_metadata::{ScrubOptions, ScrubReport};
use std::path::Path;

/// Preserve originals and place verified copies in the adjacent `_scrubbed`
/// directory without depending on a running main-window listener.
pub(crate) fn execute_cli(raw_paths: Vec<String>, completed: impl FnOnce(ScrubOutcome)) -> i32 {
    let paths = match selected_paths(raw_paths) {
        Ok(paths) => paths,
        Err(error) => return publish(Err(error), completed),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return publish(Err("scrub_start_failed".into()), completed),
    };
    runtime.block_on(complete_then_cleanup(
        crate::file_metadata::scrub_metadata_paths_headless(
            paths,
            Some(ScrubOptions {
                output_dir: None,
                dry_run: false,
                recursive: true,
                paranoid: Default::default(),
                replace_originals: false,
            }),
        ),
        crate::sidecar::close_pro_session(),
        completed,
    ))
}

fn publish(result: Result<ScrubReport, String>, completed: impl FnOnce(ScrubOutcome)) -> i32 {
    let outcome = ScrubOutcome::from_result(result);
    let code = outcome.exit_code();
    completed(outcome);
    code
}

async fn complete_then_cleanup(
    operation: impl std::future::Future<Output = Result<ScrubReport, String>>,
    cleanup: impl std::future::Future<Output = ()>,
    completed: impl FnOnce(ScrubOutcome),
) -> i32 {
    let code = publish(operation.await, completed);
    cleanup.await;
    code
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
                .find("context_scrub_notification::run(paths)")
                .unwrap()
                < source.find("session_instance::acquire(&cli_args)").unwrap()
        );
        assert!(!include_str!("context_menu_scrub.rs").contains(concat!("Message", "BoxW(")));
    }

    #[tokio::test]
    async fn verified_failure_is_published_before_cleanup_but_exit_waits_for_cleanup() {
        let (ready, result) = tokio::sync::oneshot::channel();
        let (release, cleanup) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(complete_then_cleanup(
            async { Err("scrub_selected_item_missing".into()) },
            async {
                cleanup.await.unwrap();
            },
            move |outcome| {
                ready.send(outcome).unwrap();
            },
        ));
        assert_eq!(result.await.unwrap().exit_code(), 1);
        assert!(!task.is_finished());
        release.send(()).unwrap();
        assert_eq!(task.await.unwrap(), 1);
    }

    #[tokio::test]
    async fn no_notification_is_published_while_the_engine_is_still_checking() {
        let (finish, checking) = tokio::sync::oneshot::channel();
        let (send, mut notification) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(complete_then_cleanup(
            async {
                checking.await.unwrap();
                Err("scrub_start_failed".into())
            },
            async {},
            move |outcome| {
                send.send(outcome).unwrap();
            },
        ));
        tokio::task::yield_now().await;
        assert!(matches!(
            notification.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        finish.send(()).unwrap();
        assert_eq!(notification.await.unwrap().exit_code(), 1);
        assert_eq!(task.await.unwrap(), 1);
    }
}
