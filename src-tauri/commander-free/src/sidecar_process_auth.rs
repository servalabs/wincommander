// SPDX-License-Identifier: AGPL-3.0-or-later

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedProProcess {
    pub(crate) image_hash: String,
}

fn require_expected_pid(spawned_pid: Option<u32>, connected_pid: u32) -> Result<u32, String> {
    let spawned_pid = spawned_pid.ok_or_else(|| "spawned Pro PID is unavailable".to_string())?;
    if connected_pid == 0 || connected_pid != spawned_pid {
        return Err("connected pipe client is not the spawned Pro process".to_string());
    }
    Ok(spawned_pid)
}

fn hash_open_file(file: &std::fs::File) -> Result<String, String> {
    let mut reader = file;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("cannot read connected Pro image: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

#[cfg(windows)]
pub(crate) fn verify_connected_pro(
    pipe: &tokio::net::windows::named_pipe::NamedPipeServer,
    spawned_pid: Option<u32>,
    expected_image: &Path,
) -> Result<VerifiedProProcess, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    let mut connected_pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut connected_pid) }
        == 0
    {
        return Err(format!(
            "cannot identify connected Pro process: {}",
            std::io::Error::last_os_error()
        ));
    }
    let pid = require_expected_pid(spawned_pid, connected_pid)?;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(format!(
            "cannot open connected Pro process: {}",
            std::io::Error::last_os_error()
        ));
    }
    let _process = HandleGuard(process);
    let mut image_buffer = vec![0u16; 32_768];
    let mut image_length = image_buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process, 0, image_buffer.as_mut_ptr(), &mut image_length)
    } == 0
    {
        return Err(format!(
            "cannot resolve connected Pro image: {}",
            std::io::Error::last_os_error()
        ));
    }
    let image_path = PathBuf::from(String::from_utf16_lossy(
        &image_buffer[..image_length as usize],
    ));
    let expected = std::fs::canonicalize(expected_image)
        .map_err(|error| format!("cannot resolve expected Pro image: {error}"))?;
    let actual = std::fs::canonicalize(&image_path)
        .map_err(|error| format!("cannot resolve connected Pro image: {error}"))?;
    if !expected
        .to_string_lossy()
        .eq_ignore_ascii_case(&actual.to_string_lossy())
    {
        return Err("connected Pro image path does not match the spawned image".to_string());
    }
    use std::os::windows::fs::OpenOptionsExt;
    let image = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(&actual)
        .map_err(|error| format!("cannot open connected Pro image: {error}"))?;
    Ok(VerifiedProProcess {
        image_hash: hash_open_file(&image)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_or_missing_pipe_client_pid_is_rejected() {
        assert!(require_expected_pid(None, 7).is_err());
        assert!(require_expected_pid(Some(7), 0).is_err());
        assert!(require_expected_pid(Some(7), 8).is_err());
        assert_eq!(require_expected_pid(Some(7), 7).unwrap(), 7);
    }
}
