// SPDX-License-Identifier: AGPL-3.0-or-later

/// Hydrate before constructing the GUI.  The known settings snapshot carries
/// the startup-PIN and close-window policy, so the shell must not be created
/// until this succeeds.  There is deliberately no artificial receiver
/// deadline: timing out cannot cancel the filesystem or service call and used
/// to leave that worker holding the cold-read transaction while a second
/// renderer request timed out too.
pub(crate) fn preload_settings() -> Result<(), String> {
    preload_with(|| super::read_settings().map(|_| ()))
}

fn preload_with(load: impl FnOnce() -> Result<(), String> + Send + 'static) -> Result<(), String> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("settings-preload".into())
        .spawn(move || {
            let _ = send.send(load());
        })
        .map_err(|_| "Settings initialization could not start".to_string())?;
    match receive.recv() {
        Ok(Ok(())) => Ok(()),
        // Keep filesystem and account details out of the pre-window dialog.
        Ok(Err(_)) => Err("Settings could not be loaded safely".into()),
        Err(_) => Err("Settings initialization stopped unexpectedly".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preload_reports_success_and_sanitizes_failure() {
        assert!(preload_with(|| Ok(())).is_ok());
        assert_eq!(
            preload_with(|| Err("private-path-and-account".into())),
            Err("Settings could not be loaded safely".into())
        );
    }

    #[test]
    fn preload_waits_for_one_successful_load_instead_of_issuing_a_second_cold_read() {
        let (release, blocked) = std::sync::mpsc::channel();
        let (finished, complete) = std::sync::mpsc::channel();
        let caller = std::thread::spawn(move || {
            preload_with(move || {
                blocked
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                finished.send(()).unwrap();
                Ok(())
            })
        });
        assert!(complete
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err());
        release.send(()).unwrap();
        complete
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(caller.join().unwrap().is_ok());
    }
}
