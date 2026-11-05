// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only lifecycle experiment. No production store or cutover is wired.
use rand::{rngs::OsRng, RngCore};
use std::io;
use zeroize::Zeroizing;

const MAX_PROTECTED: usize = 65536;
const KEY_MAGIC: &[u8; 8] = b"WCIDXK01";
const KEY_RECORD_LEN: usize = 8 + 16 + 32;
trait Protector {
    fn protect(&self, value: &[u8]) -> io::Result<Vec<u8>>;
    fn unprotect(&self, value: &[u8]) -> io::Result<Zeroizing<Vec<u8>>>;
}
trait KeyStore {
    // Errors (including access denied or corrupted file) are not absence.
    fn read(&self) -> io::Result<Option<Vec<u8>>>;
    // Must atomically create only when absent and never overwrite.
    fn create_new(&mut self, value: &[u8]) -> io::Result<()>;
}
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "protected search key invalid") }

// Caller must determine has_index under a cross-process exclusion covering both
// index state and key creation. A boolean alone is NOT a production fence.
// The expected index identity must come from independently trusted durable state;
// a record beside the key is not a trusted identity or rollback anchor.
fn load_or_create(store: &mut impl KeyStore, protector: &impl Protector, index_id: &[u8; 16], has_index: bool) -> io::Result<Zeroizing<[u8; 32]>> {
    match store.read()? {
        Some(protected) => {
            if protected.is_empty() || protected.len() > MAX_PROTECTED { return Err(invalid()); }
            let plain = protector.unprotect(&protected)?;
            let mut key = Zeroizing::new([0; 32]);
            if plain.len() != KEY_RECORD_LEN || &plain[..8] != KEY_MAGIC || &plain[8..24] != index_id { return Err(invalid()); }
            key.copy_from_slice(&plain[24..]);
            Ok(key)
        }
        None if has_index => Err(io::Error::new(io::ErrorKind::NotFound, "existing search index key missing")),
        None => {
            let mut key = Zeroizing::new([0; 32]);
            OsRng.fill_bytes(key.as_mut());
            let mut record = Zeroizing::new([0u8; KEY_RECORD_LEN]);
            record[..8].copy_from_slice(KEY_MAGIC);
            record[8..24].copy_from_slice(index_id);
            record[24..].copy_from_slice(key.as_ref());
            let protected = protector.protect(record.as_ref())?;
            if protected.is_empty() || protected.len() > MAX_PROTECTED { return Err(invalid()); }
            store.create_new(&protected)?;
            // Treat missing, unreadable, or changed publication as an error.
            // The caller must inspect an ambiguous outcome before retrying.
            if store.read()?.as_deref() != Some(protected.as_slice()) { return Err(invalid()); }
            Ok(key)
        }
    }
}

// Windows experiment only. The caller must independently authorize the root,
// hold a cross-process migration lease and provide a trusted index identity.
// Pins deliberately remain alive for this store's lifetime.
#[cfg(windows)]
struct PinnedKeyFile {
    directory: crate::read_only::CheckedDirectory,
    _pins: Vec<std::fs::File>,
}
#[cfg(windows)]
impl PinnedKeyFile {
    fn open(root: &std::path::Path) -> io::Result<Self> {
        use crate::read_only::{AccessMode, CheckedDirectory};
        let pins = CheckedDirectory::pin_root_path(root)?;
        let directory = CheckedDirectory::open(root, AccessMode::Writable)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { directory, _pins: pins })
    }
}
#[cfg(windows)]
impl KeyStore for PinnedKeyFile {
    fn read(&self) -> io::Result<Option<Vec<u8>>> {
        use std::{io::Read, os::windows::io::AsRawHandle, path::Path};
        use tantivy::directory::error::OpenReadError;
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        let path = Path::new("protected-search-key.v1");
        let file = match self.directory.open_read_pinned(path) {
            Ok(file) => file,
            Err(OpenReadError::FileDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(io::Error::other(error.to_string())),
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file and output struct remain valid for the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.nNumberOfLinks != 1 { return Err(invalid()); }
        let mut bytes = Vec::new();
        file.take(MAX_PROTECTED as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > MAX_PROTECTED { return Err(invalid()); }
        Ok(Some(bytes))
    }
    fn create_new(&mut self, value: &[u8]) -> io::Result<()> {
        if value.is_empty() || value.len() > MAX_PROTECTED { return Err(invalid()); }
        self.directory.atomic_create_pinned(std::path::Path::new("protected-search-key.v1"), value)
    }
}
// DPAPI user scope: do not set CRYPTPROTECT_LOCAL_MACHINE. Not yet wired to a
// real key store. No runtime fallback or new-key creation on unprotect failure.
#[cfg(windows)]
struct UserDpapi;
#[cfg(windows)]
impl UserDpapi {
    fn transform(&self, input: &[u8], protect: bool) -> io::Result<Zeroizing<Vec<u8>>> {
        use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
        }};
        use zeroize::Zeroize;
        if input.is_empty() || input.len() > MAX_PROTECTED { return Err(invalid()); }
        let source = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: buffers live for the call; output is Windows-allocated and is
        // zeroed and freed below. No machine-wide flag or UI is enabled.
        let ok = unsafe { if protect {
            CryptProtectData(&source, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        } else {
            CryptUnprotectData(&source, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        }};
        if ok == 0 { return Err(io::Error::last_os_error()); }
        if output.pbData.is_null() { return Err(invalid()); }
        // SAFETY: successful DPAPI returned this allocation with this size.
        let allocated = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let result = if allocated.len() <= MAX_PROTECTED { Ok(Zeroizing::new(allocated.to_vec())) } else { Err(invalid()) };
        allocated.zeroize();
        unsafe { LocalFree(output.pbData as _); }
        result
    }
}
#[cfg(windows)]
impl Protector for UserDpapi {
    fn protect(&self, value: &[u8]) -> io::Result<Vec<u8>> { Ok(self.transform(value, true)?.to_vec()) }
    fn unprotect(&self, value: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> { self.transform(value, false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Identity protector is only a portable test double, never a security provider.
    struct Stub;
    impl Protector for Stub {
        fn protect(&self, value: &[u8]) -> io::Result<Vec<u8>> { Ok(value.to_vec()) }
        fn unprotect(&self, value: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> { Ok(Zeroizing::new(value.to_vec())) }
    }
    #[derive(Default)]
    struct Memory { value: Option<Vec<u8>>, fail_read: bool, conflict: bool, bad_readback: bool }
    impl KeyStore for Memory {
        fn read(&self) -> io::Result<Option<Vec<u8>>> {
            if self.fail_read { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")); }
            if self.bad_readback && self.value.is_some() { return Ok(Some(vec![0])); }
            Ok(self.value.clone())
        }
        fn create_new(&mut self, value: &[u8]) -> io::Result<()> {
            if self.conflict || self.value.is_some() { return Err(io::Error::new(io::ErrorKind::AlreadyExists, "key exists")); }
            self.value = Some(value.to_vec()); Ok(())
        }
    }
    #[test]
    fn creates_once_and_reopens_same_key() {
        let mut store = Memory::default();
        let first = load_or_create(&mut store, &Stub, &[1; 16], false).unwrap();
        let second = load_or_create(&mut store, &Stub, &[1; 16], true).unwrap();
        assert_eq!(&*first, &*second);
    }
    #[test]
    fn missing_existing_key_and_permission_errors_never_create_key() {
        let mut store = Memory::default();
        assert_eq!(load_or_create(&mut store, &Stub, &[1; 16], true).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert!(store.value.is_none());
        store.fail_read = true;
        assert_eq!(load_or_create(&mut store, &Stub, &[1; 16], false).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(store.value.is_none());
    }
    #[test]
    fn corrupt_key_and_creation_race_are_errors_not_replacements() {
        let mut store = Memory { value: Some(vec![3; 31]), ..Default::default() };
        assert!(load_or_create(&mut store, &Stub, &[1; 16], true).is_err());
        assert_eq!(store.value, Some(vec![3; 31]));
        let mut racing = Memory { conflict: true, ..Default::default() };
        assert_eq!(load_or_create(&mut racing, &Stub, &[1; 16], false).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(racing.value.is_none());
    }
    #[test]
    fn readback_mismatch_does_not_report_creation_success() {
        let mut store = Memory { bad_readback: true, ..Default::default() };
        assert!(load_or_create(&mut store, &Stub, &[1; 16], false).is_err());
        assert!(store.value.is_some()); // ambiguous publication is preserved for inspection
    }
    #[test]
    fn keys_are_bound_to_the_expected_index_and_bad_records_fail_closed() {
        let mut store = Memory::default();
        let key = load_or_create(&mut store, &Stub, &[1; 16], false).unwrap();
        let saved = store.value.clone().unwrap();
        assert_eq!(load_or_create(&mut store, &Stub, &[2; 16], true).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(store.value.as_ref().unwrap(), &saved);
        assert_eq!(&*key, &*load_or_create(&mut store, &Stub, &[1; 16], true).unwrap());
        for length in [0, 7, 8, 23, 24, KEY_RECORD_LEN - 1] {
            store.value = Some(saved[..length].to_vec());
            assert!(load_or_create(&mut store, &Stub, &[1; 16], true).is_err());
        }
        let mut wrong_format = saved;
        wrong_format[0] ^= 1;
        store.value = Some(wrong_format);
        assert!(load_or_create(&mut store, &Stub, &[1; 16], true).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn pinned_user_key_roundtrips_and_missing_existing_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let mut file = PinnedKeyFile::open(root.path()).unwrap();
        assert!(load_or_create(&mut file, &UserDpapi, &[1; 16], true).is_err());
        let first = load_or_create(&mut file, &UserDpapi, &[1; 16], false).unwrap();
        assert_eq!(&*first, &*load_or_create(&mut file, &UserDpapi, &[1; 16], true).unwrap());
        assert!(load_or_create(&mut file, &UserDpapi, &[2; 16], true).is_err());
        let path = root.path().join("protected-search-key.v1");
        let protected = std::fs::read(&path).unwrap();
        assert!(!protected.windows(32).any(|part| part == first.as_ref()));
        assert!(file.create_new(b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), protected);
    }
    #[cfg(windows)]
    #[test]
    fn pinned_user_key_rejects_hardlinks_without_modifying_them() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("source");
        std::fs::write(&source, b"outside key").unwrap();
        std::fs::hard_link(&source, root.path().join("protected-search-key.v1")).unwrap();
        let mut file = PinnedKeyFile::open(root.path()).unwrap();
        assert!(load_or_create(&mut file, &UserDpapi, &[1; 16], true).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"outside key");
    }
    #[cfg(windows)]
    #[test]
    fn current_user_dpapi_roundtrip_rejects_corruption() {
        let protector = UserDpapi;
        let protected = protector.protect(&[7; 32]).unwrap();
        assert_eq!(protector.unprotect(&protected).unwrap().as_slice(), &[7; 32]);
        let mut corrupt = protected; let i = corrupt.len() / 2; corrupt[i] ^= 1;
        assert!(protector.unprotect(&corrupt).is_err());
    }
}
