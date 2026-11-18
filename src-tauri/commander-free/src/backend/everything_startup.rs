//! Start only the installed, signed Everything client in the interactive session.
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
        process::CommandExt,
    },
    path::{Component, Path, Prefix},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Security::{
        Cryptography::{CertGetNameStringW, CERT_NAME_ATTR_TYPE},
        WinTrust::*,
    },
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
    System::Threading::CREATE_NO_WINDOW,
    UI::Shell::{FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86},
};

static LAST_LAUNCH: Mutex<Option<Instant>> = Mutex::new(None);

pub(super) fn start() -> bool {
    let Ok(mut last_launch) = LAST_LAUNCH.try_lock() else {
        return true;
    };
    if last_launch.is_some_and(|time| time.elapsed() < Duration::from_secs(5)) {
        return true;
    }
    let Ok(elevated) = crate::process_privileges::current_process_elevation() else {
        return false;
    };
    for folder in [FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86] {
        let Some(root) = super::known_folder_path(&folder) else {
            continue;
        };
        for name in ["Everything", "Everything 1.5a"] {
            let path = root.join(name).join("Everything.exe");
            if launch(&path, elevated) {
                *last_launch = Some(Instant::now());
                return true;
            }
        }
    }
    false
}

fn launch(path: &Path, elevated: bool) -> bool {
    let Some(_locks) = verified_install(path) else {
        return false;
    };
    if elevated {
        crate::hide_recovery_launch::launch_everything(path).is_ok()
    } else {
        Command::new(path)
            .args(["-startup", "-first-instance"])
            .current_dir(path.parent().expect("verified path has a parent"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .is_ok()
    }
}

fn verified_install(path: &Path) -> Option<Vec<File>> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        || components.next() != Some(Component::RootDir)
        || !components.all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let parents: Vec<&Path> = path.ancestors().skip(1).collect();
    let mut locks = Vec::new();
    for parent in parents.into_iter().rev() {
        let directory = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)
            .ok()?;
        let metadata = directory.metadata().ok()?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return None;
        }
        locks.push(directory);
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || !signed_by_voidtools(path, &file)
    {
        return None;
    }
    locks.push(file);
    Some(locks)
}

fn signed_by_voidtools(path: &Path, file: &File) -> bool {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut subject = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide.as_ptr(),
        hFile: file.as_raw_handle(),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut subject,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // The file and its parent chain stay open through verification and process creation.
    unsafe {
        let trusted = WinVerifyTrustEx(std::ptr::null_mut(), &mut action, &mut data) == 0;
        let correct_publisher = trusted && publisher_is_voidtools(&data);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrustEx(std::ptr::null_mut(), &mut action, &mut data);
        correct_publisher
    }
}

unsafe fn publisher_is_voidtools(data: &WINTRUST_DATA) -> bool {
    unsafe {
        let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
        if provider.is_null() {
            return false;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() {
            return false;
        }
        let cert = WTHelperGetProvCertFromChain(signer, 0);
        let Some(cert) = cert.as_ref().filter(|cert| !cert.pCert.is_null()) else {
            return false;
        };
        let mut value = [0u16; 128];
        let length = CertGetNameStringW(
            cert.pCert,
            CERT_NAME_ATTR_TYPE,
            0,
            c"2.5.4.10".as_ptr().cast(),
            value.as_mut_ptr(),
            value.len() as u32,
        );
        length > 1
            && length as usize <= value.len()
            && String::from_utf16_lossy(&value[..length as usize - 1]) == "voidtools PTY LTD"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_unsigned_candidates() {
        assert!(verified_install(Path::new("Everything.exe")).is_none());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Everything.exe");
        std::fs::write(&path, b"unsigned fixture").unwrap();
        assert!(verified_install(&path).is_none());
    }
}
