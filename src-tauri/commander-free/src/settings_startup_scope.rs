// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sign-in presentation is one machine choice, never a personal override.
use serde_json::Value;
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::{Duration, Instant},
};

pub(super) const PATH: &str = "app.startSilentlyAtSignIn";

pub(super) fn acquire_writer(elevated: bool, user_install: bool) -> Result<Option<File>, String> {
    acquire_writer_with(elevated, user_install, crate::paths::datastore_data_dir)
}

fn acquire_writer_with(
    elevated: bool,
    user_install: bool,
    root: impl FnOnce() -> Result<std::path::PathBuf, String>,
) -> Result<Option<File>, String> {
    if !elevated && !user_install {
        return Ok(None);
    }
    // The installation's existing protected root supplies the ACL. Never delete
    // this persistent lockfile: the OS lock, not its presence, owns the lease.
    let path = root()?.join(".settings-write.lock");
    lock_writer(&path, Duration::from_secs(15)).map(Some)
}

fn lock_writer(path: &Path, timeout: Duration) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|_| "Machine settings could not be locked safely")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Machine settings lock could not be checked")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Machine settings lock is not a regular file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Machine settings lock is redirected".into());
        }
    }
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if started.elapsed() < timeout => {
                std::thread::sleep(
                    Duration::from_millis(25).min(timeout.saturating_sub(started.elapsed())),
                );
            }
            Err(TryLockError::WouldBlock) => {
                return Err("Another session is saving machine settings. Try again.".into())
            }
            Err(TryLockError::Error(_)) => {
                return Err("Machine settings could not be locked safely".into())
            }
        }
    }
}

pub(super) fn remove_personal_choice(overlay: &mut Value) {
    if let Some(app) = overlay.get_mut("app").and_then(Value::as_object_mut) {
        app.remove("startSilentlyAtSignIn");
    }
}

pub(super) fn authorize_write(
    stored: &Value,
    candidate: bool,
    cached: Option<bool>,
    elevated: impl FnOnce() -> Result<bool, String>,
) -> Result<(), String> {
    let current = stored
        .pointer("/app/startSilentlyAtSignIn")
        .map(|value| value.as_bool().ok_or("Invalid machine startup preference"))
        .transpose()?
        .unwrap_or(true);
    if current == candidate {
        return Ok(());
    }
    // An unrelated save from an older session must not undo another admin's choice.
    if cached == Some(candidate) {
        return Err(
            "The startup preference changed in another session. Reload settings and try again."
                .into(),
        );
    }
    if !elevated()? {
        return Err(
            "Administrator privileges are required to change startup behavior for all users."
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        create_default_settings, merge_user_overlay, parse_and_migrate_json_val,
        split_settings_value,
    };
    use super::*;
    use serde_json::json;

    #[test]
    fn ordinary_personal_writer_needs_no_protected_lock() {
        assert!(acquire_writer_with(false, false, || panic!(
            "standard users must not touch the machine lock"
        ))
        .unwrap()
        .is_none());
    }

    #[test]
    fn personal_install_writes_take_the_owned_local_lock_without_elevation() {
        let directory = tempfile::tempdir().unwrap();
        let guard =
            acquire_writer_with(false, true, || Ok(directory.path().to_path_buf())).unwrap();
        assert!(guard.is_some());
        let path = directory.path().join(".settings-write.lock");
        assert!(lock_writer(&path, Duration::ZERO).is_err());
        assert!(authorize_write(&json!({}), false, Some(true), || Ok(false)).is_err());
    }

    #[test]
    fn competing_handles_wait_boundedly_and_drop_releases_the_lock() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("writer.lock");
        let first = lock_writer(&path, Duration::ZERO).unwrap();
        let start = Instant::now();
        assert!(lock_writer(&path, Duration::from_millis(50))
            .unwrap_err()
            .contains("Another session"));
        assert!(start.elapsed() >= Duration::from_millis(50));
        assert!(start.elapsed() < Duration::from_secs(2));
        #[cfg(windows)]
        assert!(
            std::fs::remove_file(&path).is_err(),
            "a held lock cannot be replaced"
        );
        drop(first);
        let next = lock_writer(&path, Duration::ZERO).unwrap();
        drop(next);
        assert!(path.exists(), "the persistent lockfile must not be removed");
    }

    #[test]
    fn writer_lock_child() {
        let Some(directory) = std::env::var_os("WINCOMMANDER_TEST_SETTINGS_LOCK_DIRECTORY") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        let _guard = lock_writer(&directory.join("writer.lock"), Duration::from_secs(2)).unwrap();
        std::fs::write(directory.join("ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(20));
    }

    #[test]
    fn another_process_is_excluded_and_crash_releases_its_lock() {
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "settings::startup_scope::tests::writer_lock_child",
                "--nocapture",
            ])
            .env(
                "WINCOMMANDER_TEST_SETTINGS_LOCK_DIRECTORY",
                directory.path(),
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = ChildGuard(command.spawn().unwrap());
        let start = Instant::now();
        while !directory.path().join("ready").exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "child lock acquisition timed out"
            );
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before acquiring its lock"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let path = directory.path().join("writer.lock");
        assert!(lock_writer(&path, Duration::from_millis(50)).is_err());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        lock_writer(&path, Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn machine_choice_wins_over_conflicting_old_personal_profiles() {
        for silent in [true, false] {
            let mut settings = create_default_settings();
            settings.app.start_silently_at_sign_in = silent;
            let (machine, personal) =
                split_settings_value(serde_json::to_value(settings).unwrap()).unwrap();
            assert_eq!(
                machine.pointer("/app/startSilentlyAtSignIn"),
                Some(&json!(silent))
            );
            assert!(personal.pointer("/app/startSilentlyAtSignIn").is_none());
            assert!(machine.pointer("/app/autoHeal").is_none());
            for old_personal in [true, false] {
                let restored = merge_user_overlay(
                    parse_and_migrate_json_val(machine.clone()).unwrap(),
                    json!({"app":{"startSilentlyAtSignIn":old_personal,"autoHeal":old_personal}}),
                )
                .unwrap();
                assert_eq!(restored.app.start_silently_at_sign_in, silent);
                assert_eq!(restored.app.auto_heal, old_personal);
            }
        }
    }

    #[test]
    fn absent_machine_choice_uses_silent_default_without_elevated_migration() {
        let (mut machine, _) =
            split_settings_value(serde_json::to_value(create_default_settings()).unwrap()).unwrap();
        machine.as_object_mut().unwrap().remove("app");
        let restored = merge_user_overlay(
            parse_and_migrate_json_val(machine.clone()).unwrap(),
            json!({"app":{"startSilentlyAtSignIn":false}}),
        )
        .unwrap();
        assert!(restored.app.start_silently_at_sign_in);
        authorize_write(&machine, true, Some(true), || {
            panic!("unchanged defaults need no elevation")
        })
        .unwrap();
        let (candidate, _) = split_settings_value(serde_json::to_value(restored).unwrap()).unwrap();
        let mut existing = candidate.clone();
        existing["app"]
            .as_object_mut()
            .unwrap()
            .remove("startSilentlyAtSignIn");
        assert!(!super::super::machine_settings_changed(&existing, &candidate).unwrap());
    }

    #[test]
    fn changed_choice_requires_confirmed_elevation_and_stale_saves_are_denied() {
        let stored = json!({"app":{"startSilentlyAtSignIn":false}});
        assert!(authorize_write(&stored, true, Some(false), || Ok(false))
            .unwrap_err()
            .contains("Administrator"));
        assert!(authorize_write(&stored, true, Some(false), || Err(
            "token unavailable".into()
        ))
        .is_err());
        authorize_write(&stored, true, Some(false), || Ok(true)).unwrap();
        assert!(authorize_write(&stored, true, Some(true), || panic!(
            "stale save checked first"
        ))
        .unwrap_err()
        .contains("another session"));
        // A direct full replacement, including imports, still needs elevation.
        assert!(authorize_write(&stored, true, None, || Ok(false)).is_err());
        assert!(authorize_write(
            &json!({"app":{"startSilentlyAtSignIn":"false"}}),
            false,
            None,
            || Ok(true)
        )
        .is_err());
    }
}
