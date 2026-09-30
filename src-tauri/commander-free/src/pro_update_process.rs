// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(windows)]
pub(super) fn stop_running_pro_at_path(path: &std::path::Path) -> Result<bool, String> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::time::{Duration, Instant};
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    let expected = std::fs::canonicalize(path).map_err(|e| format!("Pro target identity: {e}"))?;
    let expected_name = path.file_name().ok_or("Pro target has no file name")?;
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut stopped = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    for (pid, process) in system.processes() {
        if !process.name().eq_ignore_ascii_case(expected_name) {
            continue;
        }
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid.as_u32(),
            )
        };
        if raw.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(87) {
                continue;
            }
            return Err(format!("Pro update could not inspect or stop a Pro process: {error}. Administrator permission is required."));
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(handle.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut length)
        } == 0
        {
            if unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
                continue;
            }
            return Err(format!(
                "Pro process identity could not be verified: {}",
                std::io::Error::last_os_error()
            ));
        }
        let image = std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
        let actual = std::fs::canonicalize(image)
            .map_err(|e| format!("Pro process image could not be resolved: {e}"))?;
        if !expected
            .as_os_str()
            .eq_ignore_ascii_case(actual.as_os_str())
        {
            continue;
        }
        // Identity and termination use the same pinned handle, never a recycled PID.
        if unsafe { TerminateProcess(handle.as_raw_handle(), 0) } == 0
            && unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_OBJECT_0
        {
            return Err(format!(
                "Pro process could not be stopped: {}",
                std::io::Error::last_os_error()
            ));
        }
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u32;
        if unsafe { WaitForSingleObject(handle.as_raw_handle(), wait) } != WAIT_OBJECT_0 {
            return Err(
                "Pro did not stop within ten seconds. Its installed file was preserved.".into(),
            );
        }
        stopped = true;
    }
    Ok(stopped)
}

#[cfg(not(windows))]
pub(super) fn stop_running_pro_at_path(_path: &std::path::Path) -> Result<bool, String> {
    Ok(false)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    struct Fixture {
        root: std::path::PathBuf,
        children: Vec<Child>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            for child in &mut self.children {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn stopping_verified_target_preserves_same_named_process_at_another_path() {
        let id = uuid::Uuid::new_v4();
        let root = std::env::temp_dir().join(format!("pro-update-process-{id}"));
        std::fs::create_dir(&root).unwrap();
        let mut fixture = Fixture {
            root,
            children: Vec::new(),
        };
        let source = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        let mut paths = Vec::new();
        for folder in ["target", "foreign"] {
            let directory = fixture.root.join(folder);
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join(format!("pro-test-{id}.exe"));
            std::fs::copy(&source, &path).unwrap();
            fixture.children.push(
                Command::new(&path)
                    .args(["/D", "/Q", "/K"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(0x08000000)
                    .spawn()
                    .unwrap(),
            );
            paths.push(path);
        }
        assert!(fixture
            .children
            .iter_mut()
            .all(|child| child.try_wait().unwrap().is_none()));
        assert!(stop_running_pro_at_path(&paths[0]).unwrap());
        assert!(fixture.children[0].try_wait().unwrap().is_some());
        assert!(fixture.children[1].try_wait().unwrap().is_none());
        assert!(!stop_running_pro_at_path(&paths[0]).unwrap());
    }
}
