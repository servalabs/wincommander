// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in experimental cleanup of a disposable, flat legacy Tantivy cache.
//! Removing files cannot guarantee secure erasure on SSDs or backups.
use std::{collections::HashSet, io, path::{Component, Path}};
use std::os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, GetFileInformationByHandle,
    SetFileInformationByHandle,
};
use crate::read_only::{AccessMode, CheckedDirectory};
const MAX_FILES: usize = 8192;
const MAX_MANIFEST: usize = 4 * 1024 * 1024;
fn refused() -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, "legacy search cleanup refused") }
/// Resolve the current token's trusted local-app-data folder. No environment
/// override or caller-supplied path is used for the destructive operation.
pub(crate) fn authorized_parent() -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath}};
    let app = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let mut parts = Path::new(app).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() { return Err(refused()); }
    let mut raw = std::ptr::null_mut();
    // SAFETY: API allocates a null-terminated path, released below.
    let result = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        if !raw.is_null() { unsafe { CoTaskMemFree(raw.cast()) }; }
        return Err(refused());
    }
    let base = unsafe {
        let mut len = 0usize; while *raw.add(len) != 0 { len += 1; }
        let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast()); std::path::PathBuf::from(path)
    };
    if !base.is_absolute() { return Err(refused()); }
    Ok(base.join(app).join("file-search"))
}

/// Hold both directory chains and require that they identify the same parent.
pub(crate) fn pin_authorized_parent(actual: &Path, authorized: &Path) -> io::Result<Vec<std::fs::File>> {
    let actual_pins = CheckedDirectory::pin_root_path(actual)?;
    let authorized_pins = CheckedDirectory::pin_root_path(authorized)?;
    let identity = |file: &std::fs::File| -> io::Result<(u32, u32, u32)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: handles and output remain valid.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 { return Err(io::Error::last_os_error()); }
        Ok((info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
    };
    if identity(actual_pins.last().ok_or_else(refused)?)? != identity(authorized_pins.last().ok_or_else(refused)?)? { return Err(refused()); }
    let mut pins = actual_pins; pins.extend(authorized_pins); Ok(pins)
}
fn cache_file(name: &str) -> bool {
    if matches!(name, "meta.json" | "schema.version" | ".managed.json" | "scope-v1.json") { return true; }
    let parts: Vec<&str> = name.split('.').collect();
    if !(parts.len() == 2 || parts.len() == 3) || parts[0].len() != 32
        || !parts[0].bytes().all(|b| b.is_ascii_hexdigit()) { return false; }
    let extension = parts[parts.len() - 1];
    if parts.len() == 3 && (extension != "del" || parts[1].is_empty() || !parts[1].bytes().all(|b| b.is_ascii_digit())) { return false; }
    matches!(extension, "idx" | "pos" | "term" | "store" | "temp" | "fast" | "fieldnorm" | "del")
}
fn preserved(name: &str) -> bool {
    matches!(name, "scope-v2.dpapi" | ".tantivy-writer.lock" | ".tantivy-meta.lock")
}
fn open_checked(path: &Path) -> io::Result<(std::fs::File, (u32, u32, u32))> {
    let file = std::fs::OpenOptions::new().read(true).access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let meta = file.metadata()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: live file and output buffer.
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.nNumberOfLinks != 1 { return Err(refused()); }
    Ok((file, (info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow)))
}
fn validate_empty_lock(path: &Path) -> io::Result<()> {
    let file = std::fs::OpenOptions::new().read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let meta = file.metadata()?; let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: live file and output buffer.
    if !meta.is_file() || meta.len() != 0 || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.nNumberOfLinks != 1 { return Err(refused()); }
    Ok(())
}
fn open_scope(path: &Path, delete: bool) -> io::Result<std::fs::File> {
    let access = FILE_GENERIC_READ | FILE_READ_ATTRIBUTES | if delete { DELETE } else { 0 };
    let file = std::fs::OpenOptions::new().read(true).access_mode(access)
        .share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let meta = file.metadata()?; let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: live file and information buffer.
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.nNumberOfLinks != 1 { return Err(refused()); }
    Ok(file)
}
fn read_bounded(file: &mut std::fs::File, limit: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new(); file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err(refused()); } Ok(bytes)
}
/// Caller has verified the selected encrypted cache using the authoritative
/// user configuration, stopped old processes, and holds the exclusive fence.
pub(crate) fn remove_managed(old: &Path, fence: &crate::encrypted_search_fence::SearchOperationFence) -> io::Result<usize> {
    remove_managed_with_hook(old, fence, || {})
}
fn remove_managed_with_hook(old: &Path, fence: &crate::encrypted_search_fence::SearchOperationFence,
    after_validation: impl FnOnce()) -> io::Result<usize> {
    let parent = old.parent().ok_or_else(refused)?;
    fence.require_exclusive_for(parent)?;
    crate::encrypted_search_maintenance::verify_known_writers_stopped().map_err(io::Error::other)?;
    let _pins = CheckedDirectory::pin_root_path(old)?;
    let directory = CheckedDirectory::open(old, AccessMode::Writable).map_err(|e| io::Error::other(e.to_string()))?;
    let _writer = directory.acquire_lock_pinned(&tantivy::directory::INDEX_WRITER_LOCK)
        .map_err(|e| io::Error::other(e.to_string()))?;
    let entries: Vec<_> = std::fs::read_dir(old)?.take(MAX_FILES + 1).collect::<Result<_, _>>()?;
    if entries.len() > MAX_FILES { return Err(refused()); }
    let mut names = Vec::new();
    for entry in entries {
        let name = entry.file_name().into_string().map_err(|_| refused())?;
        if preserved(&name) {
            if name == "scope-v2.dpapi" {
                use crate::encrypted_key_prototype::{Protector, UserDpapi};
                let mut file = open_scope(&old.join(&name), false)?;
                let bytes = read_bounded(&mut file, crate::encrypted_scope::MAX_PROTECTED_SCOPE)?;
                let clear = UserDpapi.unprotect(&bytes)?;
                if clear.len() < 8 || !crate::encrypted_scope::same_scope(&clear, &clear[8..]).unwrap_or(false) { return Err(refused()); }
            } else { validate_empty_lock(&old.join(&name))?; }
            continue;
        }
        if !cache_file(&name) { return Err(refused()); }
        names.push(name);
    }
    if names.is_empty() { return Ok(0); }
    let mut scope_v1 = None;
    let mut _scope_v2 = None;
    if names.iter().any(|name| name == "scope-v1.json") {
        use crate::encrypted_key_prototype::{Protector, UserDpapi};
        let mut protected_file = open_scope(&old.join("scope-v2.dpapi"), false)?;
        let protected = read_bounded(&mut protected_file, crate::encrypted_scope::MAX_PROTECTED_SCOPE)?;
        let clear = UserDpapi.unprotect(&protected)?;
        if clear.len() < 8 || !crate::encrypted_scope::same_scope(&clear, &clear[8..]).unwrap_or(false) {
            return Err(refused());
        }
        let mut original_file = open_scope(&old.join("scope-v1.json"), true)?;
        let original = read_bounded(&mut original_file, crate::encrypted_scope::MAX_SCOPE)?;
        let previous: serde_json::Value = serde_json::from_slice(&original).map_err(|_| refused())?;
        let protected: serde_json::Value = serde_json::from_slice(&clear[8..]).map_err(|_| refused())?;
        let valid = |value: &serde_json::Value| value.get("version").and_then(|v| v.as_u64()) == Some(1)
            && ["roots", "exclusions", "blocked_roots"].iter().all(|key|
                value.get(*key).and_then(|v| v.as_array()).is_some_and(|items| items.iter().all(|item| item.is_string())));
        // If the current protected scope changed, retain the old copy rather
        // than guessing whether it is safe to discard.
        if !valid(&previous) || !valid(&protected) || previous != protected { return Err(refused()); }
        scope_v1 = Some(original_file); _scope_v2 = Some(protected_file);
    }
    let manifest = directory.atomic_read_bounded(Path::new(".managed.json"), MAX_MANIFEST)
        .map_err(|e| io::Error::other(e.to_string()))?;
    let listed: Vec<String> = serde_json::from_slice(&manifest).map_err(|_| refused())?;
    if listed.len() > MAX_FILES { return Err(refused()); }
    let mut managed = HashSet::new();
    for name in listed {
        let mut parts = Path::new(&name).components();
        if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() || !cache_file(&name) { return Err(refused()); }
        managed.insert(name);
    }
    for name in &names {
        if !matches!(name.as_str(), "meta.json" | "schema.version" | ".managed.json" | "scope-v1.json") && !managed.contains(name) { return Err(refused()); }
    }
    // Preflight all candidates before any mutation, recording their identities.
    let mut candidates = Vec::new();
    for name in names {
        if name == "scope-v1.json" { continue; }
        let (_, id) = open_checked(&old.join(&name))?; candidates.push((name, id));
    }
    candidates.sort_by_key(|(name, _)| name == ".managed.json"); // manifest last enables retry.
    after_validation();
    let mut removed = 0;
    if let Some(file) = scope_v1.take() {
        let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: this is the same no-write/no-delete-shared handle whose
        // bounded plaintext was compared against the held protected scope.
        if unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo,
            std::ptr::from_mut(&mut disposition).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) } == 0 {
            return Err(io::Error::last_os_error());
        }
        drop(file); removed += 1;
    }
    for (name, expected) in candidates {
        let (file, id) = match open_checked(&old.join(&name)) {
            Ok(value) => value,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if id != expected { return Err(refused()); }
        let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: deletion is applied only to the checked file handle itself.
        if unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo,
            std::ptr::from_mut(&mut disposition).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) } == 0 {
            return Err(io::Error::last_os_error());
        }
        drop(file); removed += 1;
    }
    Ok(removed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleans_real_tantivy_cache_on_disposable_directory() {
        use tantivy::{doc, schema::{Schema, TEXT}};
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        std::fs::create_dir(&old).unwrap();
        let mut builder = Schema::builder(); let body = builder.add_text_field("body", TEXT);
        let index = tantivy::Index::create_in_dir(&old, builder.build()).unwrap();
        let mut writer = index.writer(50_000_000).unwrap();
        writer.add_document(doc!(body => "old cache content")).unwrap(); writer.commit().unwrap();
        drop(writer); drop(index);
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        assert!(remove_managed(&old, &fence).unwrap() > 0);
        assert!(!old.join("meta.json").exists());
        assert_eq!(remove_managed(&old, &fence).unwrap(), 0);
    }
    #[test]
    fn disposable_cache_cleanup_is_bounded_and_retryable() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        std::fs::create_dir(&old).unwrap();
        let segment = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.store";
        std::fs::write(old.join(segment), b"old document content").unwrap();
        std::fs::write(old.join("meta.json"), b"old names").unwrap();
        std::fs::write(old.join(".managed.json"), format!("[\"{segment}\"]")).unwrap();
        let scope = br#"{"version":1,"roots":[],"exclusions":[],"blocked_roots":[]}"#;
        crate::publish_experimental_scope(&old, scope, |_| Ok(())).unwrap();
        let protected_scope = std::fs::read(old.join("scope-v2.dpapi")).unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        std::fs::write(old.join("unrelated.txt"), b"keep").unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert!(old.join(segment).exists());
        std::fs::remove_file(old.join("unrelated.txt")).unwrap();
        assert_eq!(remove_managed(&old, &fence).unwrap(), 3);
        assert_eq!(std::fs::read(old.join("scope-v2.dpapi")).unwrap(), protected_scope);
        assert!(!old.join(segment).exists());
        assert_eq!(remove_managed(&old, &fence).unwrap(), 0);
    }
    #[test]
    fn plaintext_scope_survives_when_protected_scope_is_missing_or_invalid() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        std::fs::create_dir(&old).unwrap();
        let prior = br#"{"version":1,"roots":["private"],"exclusions":[],"blocked_roots":[]}"#;
        std::fs::write(old.join("scope-v1.json"), prior).unwrap();
        std::fs::write(old.join("meta.json"), b"old metadata").unwrap();
        std::fs::write(old.join(".managed.json"), b"[]").unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert_eq!(std::fs::read(old.join("scope-v1.json")).unwrap(), prior);
        std::fs::write(old.join("scope-v2.dpapi"), b"invalid").unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert_eq!(std::fs::read(old.join("meta.json")).unwrap(), b"old metadata");
        std::fs::remove_file(old.join("scope-v2.dpapi")).unwrap();
        let changed = br#"{"version":1,"roots":[],"exclusions":[],"blocked_roots":[]}"#;
        crate::publish_experimental_scope(&old, changed, |_| Ok(())).unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert_eq!(std::fs::read(old.join("scope-v1.json")).unwrap(), prior);
    }
    #[test]
    fn validated_scope_handles_block_swap_or_write_until_deletion() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        std::fs::create_dir(&old).unwrap();
        let scope = br#"{"version":1,"roots":[],"exclusions":[],"blocked_roots":[]}"#;
        std::fs::write(old.join("scope-v1.json"), scope).unwrap();
        std::fs::write(old.join("meta.json"), b"old").unwrap();
        std::fs::write(old.join(".managed.json"), b"[]").unwrap();
        crate::publish_experimental_scope(&old, scope, |_| Ok(())).unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        remove_managed_with_hook(&old, &fence, || {
            assert!(std::fs::write(old.join("scope-v1.json"), b"changed").is_err());
            assert!(std::fs::rename(old.join("scope-v2.dpapi"), old.join("moved")).is_err());
        }).unwrap();
        assert!(!old.join("scope-v1.json").exists());
        assert!(old.join("scope-v2.dpapi").is_file());
    }
    #[test]
    fn hardlinked_cache_candidate_is_never_deleted() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        std::fs::create_dir(&old).unwrap();
        let segment = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.store";
        let outside = root.path().join("outside"); std::fs::write(&outside, b"keep").unwrap();
        std::fs::hard_link(&outside, old.join(segment)).unwrap();
        std::fs::write(old.join(".managed.json"), format!("[\"{segment}\"]")).unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"keep");
        assert!(old.join(segment).exists());
    }
    #[test]
    fn allowlist_only_accepts_known_tantivy_names() {
        assert!(cache_file("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.store"));
        assert!(cache_file("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.12.del"));
        for name in ["document.txt", "../../meta.json", "scope-v2.dpapi", "a.store", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.exe"] {
            assert!(!cache_file(name));
        }
    }
}
