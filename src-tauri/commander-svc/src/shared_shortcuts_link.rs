// SPDX-License-Identifier: AGPL-3.0-or-later
//! Resolve a Shell Link from immutable bytes, rather than reopening a mutable public path.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR};
use windows::Win32::{
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistStream, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED, STREAM_SEEK_SET,
    },
    UI::Shell::{IShellLinkW, SHCreateMemStream, ShellLink, SLGP_RAWPATH},
};

pub(super) fn calculator_bytes(target: &Path) -> Option<Vec<u8>> {
    if !target.is_absolute() || !target.is_file() {
        return None;
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
        let result = (|| {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
            link.SetPath(PCWSTR(wide.as_ptr())).ok()?;
            link.SetIconLocation(PCWSTR(wide.as_ptr()), 0).ok()?;
            let persistence: IPersistStream = link.cast().ok()?;
            let stream = SHCreateMemStream(None)?;
            persistence.Save(&stream, true).ok()?;
            stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
            let mut bytes = vec![0u8; 64 * 1024];
            let mut read = 0u32;
            stream
                .Read(
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    Some(&mut read),
                )
                .ok()
                .ok()?;
            bytes.truncate(read as usize);
            (!bytes.is_empty() && read < 64 * 1024).then_some(bytes)
        })();
        CoUninitialize();
        result
    }
}

pub(super) fn target_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return None;
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
        let result = (|| {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persistence: IPersistStream = link.cast().ok()?;
            let stream = SHCreateMemStream(Some(bytes))?;
            persistence.Load(&stream).ok()?;
            let mut arguments = [0u16; 256];
            link.GetArguments(&mut arguments).ok()?;
            if arguments[0] != 0 {
                return None;
            }
            let mut path = [0u16; 1024];
            link.GetPath(&mut path, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                .ok()?;
            let end = path.iter().position(|unit| *unit == 0)?;
            let path = PathBuf::from(String::from_utf16(&path[..end]).ok()?);
            path.is_absolute().then_some(path)
        })();
        CoUninitialize();
        result
    }
}

pub(super) fn calculator_icon_matches(bytes: &[u8], target: &Path) -> bool {
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return false;
    }
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).ok().is_err() {
            return false;
        }
        let result = (|| {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persistence: IPersistStream = link.cast().ok()?;
            let stream = SHCreateMemStream(Some(bytes))?;
            persistence.Load(&stream).ok()?;
            let mut icon = [0u16; 1024];
            let mut index = -1;
            link.GetIconLocation(&mut icon, &mut index).ok()?;
            let end = icon.iter().position(|unit| *unit == 0)?;
            let icon = String::from_utf16(&icon[..end]).ok()?;
            Some(index == 0 && icon.eq_ignore_ascii_case(&target.to_string_lossy()))
        })();
        CoUninitialize();
        result.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::STREAM_SEEK_SET;

    #[test]
    fn malformed_shortcut_cannot_claim_a_target() {
        assert!(target_from_bytes(&[]).is_none());
        assert!(target_from_bytes(&[0u8; 512]).is_none());
    }

    #[test]
    fn calculator_alias_targets_the_fixed_executable() {
        let target = std::env::current_exe().unwrap();
        let bytes = calculator_bytes(&target).unwrap();
        assert_eq!(target_from_bytes(&bytes), Some(target.clone()));
        assert!(calculator_icon_matches(&bytes, &target));
    }

    #[test]
    fn target_is_resolved_from_memory_without_opening_a_public_path() {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let target = "C:\\Program Files\\WinCommander\\wincommander-free.exe";
            let wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
            link.SetPath(PCWSTR(wide.as_ptr())).unwrap();
            let persistence: IPersistStream = link.cast().unwrap();
            let stream = SHCreateMemStream(None).unwrap();
            persistence.Save(&stream, true).unwrap();
            let mut size = 0u64;
            stream.Seek(0, STREAM_SEEK_SET, Some(&mut size)).unwrap();
            stream.Seek(0, STREAM_SEEK_SET, None).unwrap();
            let mut bytes = vec![0u8; 4096];
            let mut read = 0u32;
            stream
                .Read(
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    Some(&mut read),
                )
                .ok()
                .unwrap();
            bytes.truncate(read as usize);
            CoUninitialize();
            assert_eq!(target_from_bytes(&bytes), Some(PathBuf::from(target)));
        }
    }
}
