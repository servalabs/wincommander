// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in experimental cleanup of a disposable, flat legacy Tantivy cache.
//! Removing files cannot guarantee secure erasure on SSDs or backups.
use std::{collections::HashSet, io, path::{Component, Path}};
use std::os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, GetFileInformationByHandle,
    SetFileInformationByHandle,
};
use crate::read_only::{AccessMode, CheckedDirectory};
const MAX_FILES: usize = 8192;
const MAX_MANIFEST: usize = 4 * 1024 * 1024;
fn refused() -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, "legacy search cleanup refused") }
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
/// Caller has verified the selected encrypted cache using the authoritative
/// user configuration, stopped old processes, and holds the exclusive fence.
pub(crate) fn remove_managed(old: &Path, fence: &crate::encrypted_search_fence::SearchOperationFence) -> io::Result<usize> {
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
        if preserved(&name) { continue; }
        if !cache_file(&name) { return Err(refused()); }
        names.push(name);
    }
    if names.is_empty() { return Ok(0); }
    if names.iter().any(|name| name == "scope-v1.json") {
        use crate::encrypted_key_prototype::{Protector, UserDpapi};
        let protected = directory.atomic_read_bounded(Path::new("scope-v2.dpapi"), crate::encrypted_scope::MAX_PROTECTED_SCOPE)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let clear = UserDpapi.unprotect(&protected)?;
        if clear.len() < 8 || !crate::encrypted_scope::same_scope(&clear, &clear[8..]).unwrap_or(false) {
            return Err(refused());
        }
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
    for name in names { let (_, id) = open_checked(&old.join(&name))?; candidates.push((name, id)); }
    candidates.sort_by_key(|(name, _)| name == ".managed.json"); // manifest last enables retry.
    let mut removed = 0;
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
        std::fs::write(old.join("scope-v2.dpapi"), b"protected scope").unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        std::fs::write(old.join("unrelated.txt"), b"keep").unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert!(old.join(segment).exists());
        std::fs::remove_file(old.join("unrelated.txt")).unwrap();
        assert_eq!(remove_managed(&old, &fence).unwrap(), 3);
        assert_eq!(std::fs::read(old.join("scope-v2.dpapi")).unwrap(), b"protected scope");
        assert!(!old.join(segment).exists());
        assert_eq!(remove_managed(&old, &fence).unwrap(), 0);
    }
    #[test]
    fn plaintext_scope_survives_when_protected_scope_is_missing_or_invalid() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("scope-v1.json"), b"private roots").unwrap();
        std::fs::write(old.join("meta.json"), b"old metadata").unwrap();
        std::fs::write(old.join(".managed.json"), b"[]").unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert_eq!(std::fs::read(old.join("scope-v1.json")).unwrap(), b"private roots");
        std::fs::write(old.join("scope-v2.dpapi"), b"invalid").unwrap();
        assert!(remove_managed(&old, &fence).is_err());
        assert_eq!(std::fs::read(old.join("meta.json")).unwrap(), b"old metadata");
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
