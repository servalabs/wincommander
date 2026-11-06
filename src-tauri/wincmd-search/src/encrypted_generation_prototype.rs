// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental immutable creation-proof record; not an active selector. It does not fence old writers or prove
//! completeness: callers must retain an exclusive migration lease and verify
//! the entire generation and source state before publishing or using it.
use std::io;
const MAGIC: &[u8; 8] = b"WCSEL001";
const RECORD_LEN: usize = 8 + 16 + 32;
const MAX_PROTECTED: usize = 65536;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Selection { pub(crate) index_id: [u8; 16], creation_digest: [u8; 32] }
pub(crate) trait Protect {
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>>;
    fn open(&self, protected: &[u8]) -> io::Result<Vec<u8>>;
}
pub(crate) trait Store {
    fn read(&mut self) -> io::Result<Option<Vec<u8>>>;
    // Atomic no-replace attempt. Directory-entry durability is NOT established;
    // an error has an unknown outcome.
    fn create_once(&mut self, bytes: &[u8]) -> io::Result<()>;
}
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "invalid index selection") }
fn encode(selection: &Selection) -> [u8; RECORD_LEN] {
    let mut out = [0; RECORD_LEN];
    out[..8].copy_from_slice(MAGIC);
    out[8..24].copy_from_slice(&selection.index_id);
    out[24..].copy_from_slice(&selection.creation_digest);
    out
}
fn decode(value: &[u8]) -> io::Result<Selection> {
    if value.len() != RECORD_LEN || &value[..8] != MAGIC { return Err(invalid()); }
    let index_id = value[8..24].try_into().map_err(|_| invalid())?;
    let creation_digest = value[24..].try_into().map_err(|_| invalid())?;
    if index_id == [0; 16] { return Err(invalid()); }
    Ok(Selection { index_id, creation_digest })
}
pub(crate) fn inspect(store: &mut impl Store, protector: &impl Protect) -> io::Result<Option<Selection>> {
    let Some(record) = store.read()? else { return Ok(None); };
    if record.is_empty() || record.len() > MAX_PROTECTED { return Err(invalid()); }
    Ok(Some(decode(&protector.open(&record)?)?))
}
// The callback authenticates the candidate metadata under the migration lease.
// The digest is historical creation evidence, not current mutable metadata.
// Selection alone does not prove that source files stayed unchanged.
pub(crate) fn publish(store: &mut impl Store, protector: &impl Protect, index_id: [u8; 16], verify: impl FnOnce() -> io::Result<[u8; 32]>) -> io::Result<Selection> {
    if index_id == [0; 16] { return Err(invalid()); }
    let selection = Selection { index_id, creation_digest: verify()? };
    if let Some(current) = inspect(store, protector)? {
        return if current == selection { Ok(current) } else { Err(invalid()) };
    }
    let sealed = protector.seal(&encode(&selection))?;
    if sealed.is_empty() || sealed.len() > MAX_PROTECTED { return Err(invalid()); }
    store.create_once(&sealed)?;
    // No blind success after an ambiguous or substituted publication.
    if inspect(store, protector)? != Some(Selection { index_id, creation_digest: selection.creation_digest }) { return Err(invalid()); }
    Ok(selection)
}

#[cfg(windows)]
pub(crate) struct UserProtector;
#[cfg(windows)]
impl Protect for UserProtector {
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
        use crate::encrypted_key_prototype::{Protector, UserDpapi};
        UserDpapi.protect(plain)
    }
    fn open(&self, value: &[u8]) -> io::Result<Vec<u8>> {
        use crate::encrypted_key_prototype::{Protector, UserDpapi};
        Ok(UserDpapi.unprotect(value)?.to_vec())
    }
}

// root is an existing, independently authorized, flat selection directory.
// Its ancestor handles and the selected file remain held for this lifetime.
#[cfg(windows)]
pub(crate) struct PinnedSelectionStore {
    directory: crate::read_only::CheckedDirectory,
    root: std::path::PathBuf,
    _pins: Vec<std::fs::File>,
    held: Option<std::fs::File>,
}
#[cfg(windows)]
impl PinnedSelectionStore {
    #[cfg(feature = "experimental-encrypted-search")]
    pub(crate) fn root(&self) -> &std::path::Path { &self.root }
    pub(crate) fn open(root: &std::path::Path) -> io::Result<Self> {
        use crate::read_only::{AccessMode, CheckedDirectory};
        let pins = CheckedDirectory::pin_root_path(root)?;
        let directory = CheckedDirectory::open(root, AccessMode::Writable).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { directory, root: root.into(), _pins: pins, held: None })
    }
}
#[cfg(windows)]
impl Store for PinnedSelectionStore {
    fn read(&mut self) -> io::Result<Option<Vec<u8>>> {
        use std::{io::{Read, Seek, SeekFrom}, os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle}};
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
        if self.held.is_none() {
            let file = match std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(self.root.join("candidate-proof.v1")) {
                    Ok(file) => file,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => return Err(e),
                };
            let meta = file.metadata()?;
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: the file handle and information buffer remain valid.
            if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
                || info.nNumberOfLinks != 1 { return Err(invalid()); }
            self.held = Some(file);
        }
        let file = self.held.as_mut().ok_or_else(invalid)?;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(MAX_PROTECTED as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > MAX_PROTECTED { return Err(invalid()); }
        Ok(Some(bytes))
    }
    fn create_once(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > MAX_PROTECTED { return Err(invalid()); }
        self.directory.atomic_create_pinned(std::path::Path::new("candidate-proof.v1"), bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Explicitly non-secure test double; Windows DPAPI is tested separately.
    struct Stub;
    impl Protect for Stub {
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> { Ok(plain.to_vec()) }
        fn open(&self, value: &[u8]) -> io::Result<Vec<u8>> { Ok(value.to_vec()) }
    }
    #[derive(Default)]
    struct Fake { bytes: Option<Vec<u8>>, fail: bool, commit_before_error: bool }
    impl Store for Fake {
        fn read(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(self.bytes.clone()) }
        fn create_once(&mut self, value: &[u8]) -> io::Result<()> {
            if self.bytes.is_some() { return Err(io::ErrorKind::AlreadyExists.into()); }
            if !self.fail || self.commit_before_error { self.bytes = Some(value.to_vec()); }
            if self.fail { Err(io::Error::other("unknown outcome")) } else { Ok(()) }
        }
    }
    #[test]
    fn verified_selection_reopens_and_cannot_be_replaced() {
        let mut store = Fake::default();
        let result = publish(&mut store, &Stub, [1; 16], || Ok([2; 32])).unwrap();
        assert_eq!(inspect(&mut store, &Stub).unwrap(), Some(result));
        assert!(publish(&mut store, &Stub, [3; 16], || Ok([2; 32])).is_err());
        assert!(publish(&mut store, &Stub, [1; 16], || Ok([4; 32])).is_err());
    }
    #[test]
    fn invalid_records_and_failed_verification_never_select() {
        let mut store = Fake::default();
        assert!(publish(&mut store, &Stub, [1; 16], || Err(invalid())).is_err());
        assert!(store.bytes.is_none());
        assert!(publish(&mut store, &Stub, [0; 16], || Ok([2; 32])).is_err());
        store.bytes = Some(vec![0; 7]);
        assert!(inspect(&mut store, &Stub).is_err());
    }
    #[test]
    fn uncertain_publication_requires_inspection() {
        for written in [false, true] {
            let mut store = Fake { fail: true, commit_before_error: written, ..Default::default() };
            assert!(publish(&mut store, &Stub, [1; 16], || Ok([2; 32])).is_err());
            assert_eq!(inspect(&mut store, &Stub).unwrap().is_some(), written);
        }
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn dpapi_origin_is_bound_to_generation_and_cannot_be_reclassified() {
        let root = tempfile::tempdir().unwrap();
        record_origin(root.path(), &[7; 16], false, &[3; 32], root.path()).unwrap();
        assert_eq!(inspect_origin(root.path(), &[7; 16]).unwrap(), Some(false));
        assert!(inspect_origin(root.path(), &[8; 16]).is_err());
        assert!(record_origin(root.path(), &[7; 16], true, &[3; 32], root.path()).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_selection_reopens_without_replacing_and_keeps_file_pinned() {
        let root = tempfile::tempdir().unwrap();
        let mut store = PinnedSelectionStore::open(root.path()).unwrap();
        publish(&mut store, &UserProtector, [1; 16], || Ok([2; 32])).unwrap();
        let path = root.path().join("candidate-proof.v1");
        assert!(std::fs::remove_file(&path).is_err());
        drop(store);
        let mut reopened = PinnedSelectionStore::open(root.path()).unwrap();
        assert_eq!(inspect(&mut reopened, &UserProtector).unwrap(), Some(Selection { index_id: [1; 16], creation_digest: [2; 32] }));
        assert!(publish(&mut reopened, &UserProtector, [3; 16], || Ok([4; 32])).is_err());
    }
}

/// DPAPI-authenticated origin binds generation, approved creation manifest,
/// and the original index directory identity. Missing never proves fresh.
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Origin {
    pub id: [u8; 16], pub fresh: bool, pub digest: [u8; 32], pub source: (u32, u32, u32),
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(crate) fn record_origin(root: &std::path::Path, id: &[u8; 16], fresh: bool,
    digest: &[u8; 32], source: &std::path::Path) -> io::Result<()> {
    use crate::read_only::{AccessMode, CheckedDirectory};
    let _pins = CheckedDirectory::pin_root_path(root)?;
    let identity = crate::encrypted_migration_transaction::Intent::capture([1; 16], *id, source)?.identity();
    let origin = Origin { id: *id, fresh, digest: *digest, source: identity };
    if let Some(existing) = read_origin(root)? {
        return if existing == origin { Ok(()) } else { Err(invalid()) };
    }
    let dir = CheckedDirectory::open(root, AccessMode::Writable).map_err(|e| io::Error::other(e.to_string()))?;
    let mut plain = b"WCORIG02".to_vec(); plain.extend_from_slice(id); plain.push(u8::from(fresh));
    plain.extend_from_slice(digest);
    for part in [identity.0, identity.1, identity.2] { plain.extend_from_slice(&part.to_le_bytes()); }
    let protected = UserProtector.seal(&plain)?;
    dir.atomic_create_pinned(std::path::Path::new("origin-proof.v1"), &protected)?;
    if read_origin(root)?.as_ref() != Some(&origin) { return Err(invalid()); }
    Ok(())
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(crate) fn read_origin(root: &std::path::Path) -> io::Result<Option<Origin>> {
    use crate::read_only::{AccessMode, CheckedDirectory};
    use tantivy::directory::error::OpenReadError;
    let _pins = CheckedDirectory::pin_root_path(root)?;
    let dir = CheckedDirectory::open(root, AccessMode::GuardedReader).map_err(|e| io::Error::other(e.to_string()))?;
    let bytes = match dir.atomic_read_bounded(std::path::Path::new("origin-proof.v1"), MAX_PROTECTED) {
        Ok(v) => v,
        Err(OpenReadError::FileDoesNotExist(_)) => return Ok(None),
        Err(e) => return Err(io::Error::other(e.to_string())),
    };
    let plain = UserProtector.open(&bytes)?;
    if plain.len() != 69 || &plain[..8] != b"WCORIG02" { return Err(invalid()); }
    let id: [u8; 16] = plain[8..24].try_into().map_err(|_| invalid())?;
    if id == [0; 16] { return Err(invalid()); }
    let fresh = match plain[24] { 0 => false, 1 => true, _ => return Err(invalid()) };
    let digest = plain[25..57].try_into().map_err(|_| invalid())?;
    let part = |start| -> io::Result<u32> { Ok(u32::from_le_bytes(plain[start..start + 4].try_into().map_err(|_| invalid())?)) };
    Ok(Some(Origin { id, fresh, digest, source: (part(57)?, part(61)?, part(65)?) }))
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(crate) fn inspect_origin(root: &std::path::Path, id: &[u8; 16]) -> io::Result<Option<bool>> {
    match read_origin(root)? {
        None => Ok(None),
        Some(origin) if &origin.id == id => Ok(Some(origin.fresh)),
        Some(_) => Err(invalid()),
    }
}