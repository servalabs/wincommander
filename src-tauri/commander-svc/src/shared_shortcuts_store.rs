// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protected shared-shortcut journal and fixed Windows locations.

use super::{Journal, MAX_JOURNAL_BYTES, UNAVAILABLE};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use windows_sys::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{
        FOLDERID_CommonPrograms, FOLDERID_ProgramData, FOLDERID_ProgramFiles,
        FOLDERID_PublicDesktop, SHGetKnownFolderPath,
    },
};

pub(super) struct LinkSpec {
    pub(super) original: PathBuf,
    pub(super) backup: PathBuf,
    pub(super) target: PathBuf,
    pub(super) calculator: PathBuf,
    pub(super) calculator_backup: PathBuf,
    pub(super) calculator_target: PathBuf,
}

fn known_folder(id: &windows_sys::core::GUID) -> Result<PathBuf, &'static str> {
    let mut ptr = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut ptr) } < 0 || ptr.is_null() {
        return Err(UNAVAILABLE);
    }
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16(std::slice::from_raw_parts(ptr, len));
        CoTaskMemFree(ptr.cast());
        value.map(PathBuf::from).map_err(|_| UNAVAILABLE)
    }
}

pub(super) fn paths() -> Result<(PathBuf, [LinkSpec; 2]), &'static str> {
    let root = known_folder(&FOLDERID_ProgramData)?.join("WinCommanderSharedShortcuts");
    let target = known_folder(&FOLDERID_ProgramFiles)?
        .join("WinCommander")
        .join("wincommander-free.exe");
    let calculator_target = known_folder(&FOLDERID_ProgramFiles)?
        .join("WinCommander")
        .join("Calculator.exe");
    let desktop = known_folder(&FOLDERID_PublicDesktop)?;
    let start = known_folder(&FOLDERID_CommonPrograms)?
        .join("ServaLabs")
        .join("WinCommander");
    Ok((
        root.clone(),
        [
            LinkSpec {
                original: desktop.join("WinCommander.lnk"),
                backup: root.join("desktop.lnk"),
                target: target.clone(),
                calculator: desktop.join("calculator..lnk"),
                calculator_backup: root.join("desktop-calculator.lnk"),
                calculator_target: calculator_target.clone(),
            },
            LinkSpec {
                original: start.join("WinCommander.lnk"),
                backup: root.join("start.lnk"),
                target,
                calculator: start.join("calculator..lnk"),
                calculator_backup: root.join("start-calculator.lnk"),
                calculator_target,
            },
        ],
    ))
}

pub(super) fn read_journal(path: &Path) -> Result<Option<Journal>, &'static str> {
    let mut file = match crate::personal_settings::platform::open_record(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(UNAVAILABLE),
    };
    if file.metadata().map_err(|_| UNAVAILABLE)?.len() > MAX_JOURNAL_BYTES {
        return Err(UNAVAILABLE);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNAVAILABLE)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(UNAVAILABLE);
    }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)?;
    if journal.version != 3 || journal.claims.len() > 128 {
        return Err(UNAVAILABLE);
    }
    Ok(Some(journal))
}

pub(super) fn save_journal(path: &Path, journal: &Journal) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(journal).map_err(|_| UNAVAILABLE)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(UNAVAILABLE);
    }
    let parent = path.parent().ok_or(UNAVAILABLE)?;
    let temporary = parent.join(format!("state-{:032x}.tmp", rand::random::<u128>()));
    let mut file = crate::personal_settings::platform::open_record(&temporary, true)
        .map_err(|_| UNAVAILABLE)?;
    file.write_all(&bytes).map_err(|_| UNAVAILABLE)?;
    file.sync_all().map_err(|_| UNAVAILABLE)?;
    drop(file);
    crate::personal_settings::platform::replace(&temporary, path)
}

pub(super) fn entry_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

pub(super) fn clean_old_alias_temps(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.take(32).flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(hex) = name
            .strip_prefix("shortcut-")
            .and_then(|rest| rest.strip_suffix(".tmp"))
        else {
            continue;
        };
        if hex.len() != 32 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let path = entry.path();
        let Ok(file) = crate::personal_settings::platform::open_record(&path, false) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if metadata.len() > 64 * 1024 {
            continue;
        }
        let old = metadata
            .modified()
            .ok()
            .and_then(|when| now.duration_since(when).ok())
            .is_some_and(|elapsed| elapsed >= Duration::from_secs(600));
        drop(file);
        if old {
            let _ = std::fs::remove_file(&path);
        }
    }
}
