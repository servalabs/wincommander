// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(windows)]

mod elevation;
mod path_trust;

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    UnexpectedArguments,
    UnsafeLocation,
    InspectionFailed,
    StartFailed,
}

fn child_path(
    current: &Path,
    installation: &Path,
    argument_count: usize,
) -> Result<PathBuf, LaunchError> {
    if argument_count != 0 {
        return Err(LaunchError::UnexpectedArguments);
    }
    let expected = installation.join("Calculator.exe");
    if !current
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy())
    {
        return Err(LaunchError::UnsafeLocation);
    }
    Ok(installation.join("wincommander-free.exe"))
}

fn protected_installation() -> Result<PathBuf, LaunchError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
    };
    let mut raw = std::ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut raw) };
    if result != 0 || raw.is_null() {
        if !raw.is_null() {
            unsafe {
                CoTaskMemFree(raw.cast());
            }
        }
        return Err(LaunchError::InspectionFailed);
    }
    let mut length = 0;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let directory =
        PathBuf::from(unsafe { OsString::from_wide(std::slice::from_raw_parts(raw, length)) });
    unsafe {
        CoTaskMemFree(raw.cast());
    }
    if !directory.is_absolute() {
        return Err(LaunchError::UnsafeLocation);
    }
    Ok(directory.join("WinCommander"))
}

pub fn launch(arguments: impl Iterator<Item = OsString>) -> Result<(), LaunchError> {
    let current = std::env::current_exe().map_err(|_| LaunchError::InspectionFailed)?;
    let installation = protected_installation()?;
    let child = child_path(&current, &installation, arguments.count())?;
    // Retain checked handles until the child is created. The canonical Free
    // process remains the service peer; the launcher gains no IPC privilege.
    let _pins = path_trust::pin_installation(&current, &child, &installation)?;
    let token = elevation::offer(&current, &installation)?;
    let elevated = match token {
        elevation::LaunchToken::ElevatedCopyStarted => return Ok(()),
        elevation::LaunchToken::Current { elevated } => elevated,
    };
    child_command(&child, &installation, elevated)
        .spawn()
        .map_err(|_| LaunchError::StartFailed)?;
    Ok(())
}

fn child_command(child: &Path, installation: &Path, elevated: bool) -> std::process::Command {
    let mut command = std::process::Command::new(child);
    command.current_dir(installation);
    if elevated {
        command.arg("--elevated-relaunch");
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_fixed_installed_launcher_and_no_arguments_are_allowed() {
        let root = Path::new(r"C:\Program Files\WinCommander");
        assert_eq!(
            child_path(&root.join("Calculator.exe"), root, 0).unwrap(),
            root.join("wincommander-free.exe")
        );
        for current in [
            r"C:\Users\Admin\Calculator.exe",
            r"C:\Program Files\Other\Calculator.exe",
            r"C:\Program Files\WinCommander\other.exe",
        ] {
            assert_eq!(
                child_path(Path::new(current), root, 0),
                Err(LaunchError::UnsafeLocation)
            );
        }
        assert_eq!(
            child_path(&root.join("Calculator.exe"), root, 1),
            Err(LaunchError::UnexpectedArguments)
        );
    }

    #[test]
    fn known_folder_resolution_does_not_use_inherited_program_files_variables() {
        let expected = protected_installation().unwrap();
        let original = std::env::var_os("ProgramFiles");
        std::env::set_var("ProgramFiles", r"C:\Users\Public\fake-program-files");
        let actual = protected_installation();
        match original {
            Some(value) => std::env::set_var("ProgramFiles", value),
            None => std::env::remove_var("ProgramFiles"),
        }
        assert_eq!(actual.unwrap(), expected);
    }

    #[test]
    fn an_uninstalled_copy_is_rejected_before_requesting_elevation() {
        assert_eq!(launch(std::iter::empty()), Err(LaunchError::UnsafeLocation));
        assert_eq!(
            launch(std::iter::once("--arbitrary-path".into())),
            Err(LaunchError::UnexpectedArguments)
        );
    }

    #[test]
    fn only_the_fixed_handoff_flag_is_sent_to_the_canonical_child() {
        let root = Path::new(r"C:\Program Files\WinCommander");
        let child = root.join("wincommander-free.exe");
        let ordinary = child_command(&child, root, false);
        assert_eq!(ordinary.get_program(), child.as_os_str());
        assert_eq!(ordinary.get_current_dir(), Some(root));
        assert_eq!(ordinary.get_args().count(), 0);
        let elevated = child_command(&child, root, true);
        assert_eq!(
            elevated.get_args().collect::<Vec<_>>(),
            vec!["--elevated-relaunch"]
        );
    }
}
