// SPDX-License-Identifier: AGPL-3.0-or-later

use std::time::Duration;

// Includes cold filesystem/key access and the transport's handshake retries.
const PRELOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StartupSettings {
    pub start_silently_at_sign_in: bool,
    pub calculator_lock_active: bool,
}

impl StartupSettings {
    fn from_settings(settings: &super::AppSettings) -> Self {
        Self {
            start_silently_at_sign_in: settings.app.start_silently_at_sign_in,
            calculator_lock_active: crate::startup_auth::gate_enabled(&settings.ideal.privacy.startup_pin),
        }
    }
}

/// The GUI must not open without the snapshot that carries its PIN and policy.
/// A stalled worker causes a closed startup, never a second read or defaults.
pub(crate) fn preload_settings() -> Result<StartupSettings, String> {
    preload_with(|| super::read_settings().map(|settings| StartupSettings::from_settings(&settings)))
}

fn preload_with<T: Send + 'static>(
    load: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    preload_with_timeout(load, PRELOAD_TIMEOUT)
}

fn preload_with_timeout<T: Send + 'static>(
    load: impl FnOnce() -> Result<T, String> + Send + 'static,
    timeout: Duration,
) -> Result<T, String> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("settings-preload".into())
        .spawn(move || {
            let _ = send.send(load());
        })
        .map_err(|_| "Settings initialization could not start".to_string())?;
    match receive.recv_timeout(timeout) {
        Ok(Ok(start_silently)) => Ok(start_silently),
        // Keep filesystem and account details out of the pre-window dialog.
        Ok(Err(_)) => Err("Settings could not be loaded safely".into()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(
            "Settings took too long to load. WinCommander could not confirm its security settings and has not opened. Try opening it again.".into(),
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err("Settings initialization stopped unexpectedly".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_snapshot_uses_the_resolved_pin_policy() {
        let mut settings = super::super::create_default_settings();
        assert!(!StartupSettings::from_settings(&settings).calculator_lock_active);
        settings.ideal.privacy.startup_pin.enabled = Some(true);
        assert!(!StartupSettings::from_settings(&settings).calculator_lock_active);
        settings.ideal.privacy.startup_pin.real_hash = Some("configured".into());
        settings.app.start_silently_at_sign_in = false;
        let startup = StartupSettings::from_settings(&settings);
        assert!(startup.calculator_lock_active);
        assert!(!startup.start_silently_at_sign_in);
        settings.ideal.privacy.startup_pin.enabled = Some(false);
        assert!(!StartupSettings::from_settings(&settings).calculator_lock_active);
    }

    #[test]
    fn preload_reports_success_and_sanitizes_failure() {
        for silent in [false, true] {
            assert_eq!(preload_with(move || Ok(silent)), Ok(silent));
        }
        assert_eq!(
            preload_with::<bool>(|| Err("private-path-and-account".into())),
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
                Ok(false)
            })
        });
        assert!(complete
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err());
        release.send(()).unwrap();
        complete
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(caller.join().unwrap(), Ok(false));
    }

    #[test]
    fn stalled_preload_fails_closed_without_starting_another_load() {
        let (release, blocked) = std::sync::mpsc::channel();
        let (finished, complete) = std::sync::mpsc::channel();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker_calls = calls.clone();
        let result = preload_with_timeout(
            move || {
                worker_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                blocked.recv_timeout(Duration::from_secs(2)).unwrap();
                finished.send(()).unwrap();
                Ok(false)
            },
            Duration::from_millis(25),
        );
        assert!(result.unwrap_err().contains("has not opened"));
        release.send(()).unwrap();
        complete.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
