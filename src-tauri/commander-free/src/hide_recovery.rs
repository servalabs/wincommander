// SPDX-License-Identifier: AGPL-3.0-or-later
//! Per-session recovery listener; it never records or forwards keyboard input.

#[cfg(windows)]
mod windows {
    use std::{
        os::windows::process::CommandExt,
        process::Command,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::{CreateMutexW, OpenMutexW},
        UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VkKeyScanW},
    };
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "WinCommander Hide Recovery";
    const INTERVAL: Duration = Duration::from_millis(40);
    const DOUBLE_PRESS: Duration = Duration::from_secs(2);
    const MUTEX: &str = "Local\\WinCommanderHideRecoveryListener\0";

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Chord {
        key: i32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    }

    fn parse_chord(input: &str) -> Result<Chord, String> {
        use std::str::FromStr;
        tauri_plugin_global_shortcut::Shortcut::from_str(input)
            .map_err(|_| "Invalid hide shortcut".to_string())?;
        let mut chord = Chord {
            key: 0,
            ctrl: false,
            shift: false,
            alt: false,
        };
        for part in input.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                key => {
                    chord.key = match key {
                        "space" => 0x20,
                        "enter" => 0x0d,
                        "tab" => 0x09,
                        "escape" | "esc" => 0x1b,
                        "backspace" => 0x08,
                        "delete" => 0x2e,
                        "insert" => 0x2d,
                        "home" => 0x24,
                        "end" => 0x23,
                        "pageup" => 0x21,
                        "pagedown" => 0x22,
                        "arrowleft" | "left" => 0x25,
                        "arrowup" | "up" => 0x26,
                        "arrowright" | "right" => 0x27,
                        "arrowdown" | "down" => 0x28,
                        k if k.starts_with('f')
                            && k[1..].parse::<i32>().is_ok_and(|v| (1..=24).contains(&v)) =>
                        {
                            0x6f + k[1..].parse::<i32>().unwrap()
                        }
                        k if k.len() == 1 && k.is_ascii() => {
                            let code = unsafe { VkKeyScanW(k.as_bytes()[0] as u16) };
                            if code == -1 {
                                return Err("Hide shortcut key is unsupported for recovery".into());
                            }
                            i32::from(code & 0xff)
                        }
                        _ => return Err("Hide shortcut key is unsupported for recovery".into()),
                    };
                }
            }
        }
        if chord.key == 0 {
            return Err("Hide shortcut has no key".into());
        }
        Ok(chord)
    }

    pub fn validate(shortcut: &str) -> Result<(), String> {
        if shortcut.len() > 128 || shortcut.contains('"') {
            return Err("Invalid hide shortcut".into());
        }
        parse_chord(shortcut).map(|_| ())
    }

    fn config_path() -> Result<std::path::PathBuf, String> {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("WinCommander").join("hide-recovery-shortcut.txt"))
            .ok_or_else(|| "The user profile is unavailable".into())
    }

    fn write_shortcut(path: &std::path::Path, shortcut: &str) -> Result<(), String> {
        use std::io::Write;
        validate(shortcut)?;
        let directory = path.parent().ok_or("The user profile is unavailable")?;
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
        temp.write_all(shortcut.as_bytes())
            .map_err(|e| e.to_string())?;
        temp.persist(path).map_err(|e| e.error.to_string())?;
        Ok(())
    }

    pub fn update(shortcut: &str) -> Result<(), String> {
        if crate::startup_elevation::is_current_process_elevated() {
            return Err("Recovery setup requires a standard token".into());
        }
        write_shortcut(&config_path()?, shortcut)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let run_command = format!("\"{}\" --hide-recovery-listener", exe.display());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let is_hidden = crate::paths::hide_flag_path()
            .map_err(|e| e.to_string())?
            .try_exists()
            .map_err(|e| e.to_string())?;
        if !is_hidden {
            match hkcu
                .open_subkey_with_flags(RUN, winreg::enums::KEY_READ | winreg::enums::KEY_SET_VALUE)
            {
                Ok(run) => {
                    if run
                        .get_value::<String, _>(VALUE)
                        .ok()
                        .is_some_and(|value| value.eq_ignore_ascii_case(&run_command))
                    {
                        run.delete_value(VALUE).map_err(|e| e.to_string())?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.to_string()),
            }
        } else {
            if !cfg!(debug_assertions) {
                let (run, _) = hkcu.create_subkey(RUN).map_err(|e| e.to_string())?;
                if let Ok(existing) = run.get_value::<String, _>(VALUE) {
                    if !existing.eq_ignore_ascii_case(&run_command) {
                        return Err(
                            "Another startup entry already uses the recovery listener name".into(),
                        );
                    }
                }
                run.set_value(VALUE, &run_command)
                    .map_err(|e| e.to_string())?;
            }
            Command::new(exe)
                .arg("--hide-recovery-listener")
                .creation_flags(0x0800_0000)
                .spawn()
                .map_err(|e| format!("Could not start recovery listener: {e}"))?;
        }
        Ok(())
    }

    fn down(key: i32) -> bool {
        unsafe { GetAsyncKeyState(key) < 0 }
    }
    fn pressed(chord: Chord) -> bool {
        down(chord.key)
            && down(0x11) == chord.ctrl
            && down(0x10) == chord.shift
            && down(0x12) == chord.alt
            && !down(0x5b)
            && !down(0x5c)
    }

    #[derive(Default)]
    struct DoublePress {
        held: bool,
        previous: Option<Instant>,
    }
    impl DoublePress {
        fn update(&mut self, is_pressed: bool, app_running: bool, now: Instant) -> bool {
            let triggered = if is_pressed && !self.held {
                if app_running {
                    self.previous = None;
                    false
                } else if self
                    .previous
                    .is_some_and(|time| now.duration_since(time) <= DOUBLE_PRESS)
                {
                    self.previous = None;
                    true
                } else {
                    self.previous = Some(now);
                    false
                }
            } else {
                false
            };
            self.held = is_pressed;
            triggered
        }
    }

    pub fn configure(shortcut: &str) -> Result<(), String> {
        validate(shortcut)?;
        if crate::startup_elevation::is_current_process_elevated() {
            crate::hide_recovery_launch::verify_interactive_user()?;
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let is_hidden = crate::paths::hide_flag_path()
            .map_err(|e| e.to_string())?
            .try_exists()
            .map_err(|e| e.to_string())?;
        if crate::startup_elevation::is_current_process_elevated() {
            let name: Vec<u16> = MUTEX.encode_utf16().collect();
            crate::hide_recovery_launch::launch(&exe, shortcut)?;
            if !is_hidden {
                return Ok(());
            }
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(50));
                let listener = unsafe { OpenMutexW(0x0010_0000, 0, name.as_ptr()) };
                if !listener.is_null() {
                    unsafe { CloseHandle(listener) };
                    return Ok(());
                }
            }
            return Err("The standard-token recovery listener did not start".into());
        }
        update(shortcut)
    }

    pub fn run() {
        if crate::startup_elevation::is_current_process_elevated() {
            return;
        }
        let name: Vec<u16> = MUTEX.encode_utf16().collect();
        unsafe { SetLastError(0) };
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if mutex.is_null() {
            return;
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(mutex) };
            return;
        }
        let mut last_refresh = Instant::now() - Duration::from_secs(2);
        let mut chord = None;
        let mut gesture = DoublePress::default();
        loop {
            if last_refresh.elapsed() >= Duration::from_secs(1) {
                if !crate::paths::hide_flag_path().is_ok_and(|path| path.exists()) {
                    break;
                }
                let updated = config_path()
                    .ok()
                    .and_then(|path| {
                        let metadata = std::fs::metadata(&path).ok()?;
                        if metadata.len() > 128 {
                            return None;
                        }
                        std::fs::read_to_string(path).ok()
                    })
                    .and_then(|value| parse_chord(&value).ok());
                if chord != updated {
                    chord = updated;
                    gesture = DoublePress::default();
                }
                last_refresh = Instant::now();
            }
            let is_pressed = chord.is_some_and(pressed);
            if is_pressed && !gesture.held {
                let app_running =
                    crate::session_instance::is_background_duplicate(&["--autostart".into()]);
                if gesture.update(true, app_running, Instant::now()) {
                    if let Ok(exe) = std::env::current_exe() {
                        if let Err(error) = Command::new(exe)
                            .args(["--focus", "--recovery-reveal"])
                            .creation_flags(0x0800_0000)
                            .spawn()
                        {
                            crate::log_message_src(
                                "warn",
                                "core",
                                &format!("Recovery launch failed: {error}"),
                            );
                        }
                    }
                }
            } else {
                gesture.update(is_pressed, false, Instant::now());
            }
            std::thread::sleep(INTERVAL);
        }
        unsafe { CloseHandle(mutex) };
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn invalid_shortcut_does_not_replace_saved_recovery_configuration() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("shortcut.txt");
            write_shortcut(&path, "Ctrl+Shift+G").unwrap();
            assert!(write_shortcut(&path, "Ctrl+BadKey").is_err());
            assert_eq!(std::fs::read_to_string(path).unwrap(), "Ctrl+Shift+G");
        }
        #[test]
        fn configured_shortcut_requires_exact_modifiers() {
            assert_eq!(
                parse_chord("Ctrl+Shift+G").unwrap(),
                Chord {
                    key: 0x47,
                    ctrl: true,
                    shift: true,
                    alt: false
                }
            );
            assert_eq!(parse_chord("Alt+F10").unwrap().key, 0x79);
            assert!(parse_chord("Ctrl+BadKey").is_err());
        }
        #[test]
        fn only_two_distinct_quick_presses_without_running_app_launch() {
            let t = Instant::now();
            let mut gesture = DoublePress::default();
            assert!(!gesture.update(true, false, t));
            assert!(!gesture.update(true, false, t + Duration::from_millis(100)));
            assert!(!gesture.update(false, false, t + Duration::from_millis(200)));
            assert!(gesture.update(true, false, t + Duration::from_millis(300)));
            assert!(!gesture.update(false, false, t + Duration::from_millis(400)));
            assert!(!gesture.update(true, false, t + Duration::from_secs(3)));
            assert!(!gesture.update(false, false, t + Duration::from_secs(6)));
            assert!(!gesture.update(true, true, t + Duration::from_secs(6)));
            assert!(!gesture.update(false, false, t + Duration::from_secs(6)));
            assert!(!gesture.update(true, false, t + Duration::from_secs(6)));
        }
    }
}

#[cfg(windows)]
pub(crate) use windows::{configure, run, update, validate};
#[cfg(not(windows))]
pub(crate) fn configure(_: &str) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub(crate) fn run() {}
#[cfg(not(windows))]
pub(crate) fn update(_: &str) -> Result<(), String> {
    Err("Recovery is unavailable".into())
}
#[cfg(not(windows))]
pub(crate) fn validate(_: &str) -> Result<(), String> {
    Ok(())
}
