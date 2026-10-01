// SPDX-License-Identifier: AGPL-3.0-or-later
use std::time::Duration;

/// Hydrate before constructing the GUI; the caller must refuse launch on failure.
pub(crate) fn preload_settings(deadline: Duration) -> Result<(), String> {
    preload_with(deadline, || super::read_settings().map(|_| ()))
}

fn preload_with(
    deadline: Duration,
    load: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("settings-preload".into())
        .spawn(move || {
            let _ = send.send(load());
        })
        .map_err(|_| "Settings initialization could not start".to_string())?;
    // A deadline does not cancel OS work or authorize a second initialization.
    match receive.recv_timeout(deadline) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err("Settings could not be loaded safely".into()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err("Settings initialization did not finish in time".into())
        }
        Err(_) => Err("Settings initialization stopped unexpectedly".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preload_reports_success_and_sanitizes_failure() {
        assert!(preload_with(Duration::from_secs(2), || Ok(())).is_ok());
        assert_eq!(
            preload_with(Duration::from_secs(2), || Err(
                "private-path-and-account".into()
            )),
            Err("Settings could not be loaded safely".into())
        );
    }

    #[test]
    fn preload_deadline_returns_without_waiting_for_storage() {
        let (release, blocked) = std::sync::mpsc::channel();
        let (finished, complete) = std::sync::mpsc::channel();
        let result = preload_with(Duration::from_millis(20), move || {
            let _ = blocked.recv_timeout(Duration::from_secs(5));
            let _ = finished.send(());
            Ok(())
        });
        let completed_early = complete.try_recv().is_ok();
        let _ = release.send(());
        complete.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!completed_early);
        assert_eq!(
            result,
            Err("Settings initialization did not finish in time".into())
        );
    }
}
