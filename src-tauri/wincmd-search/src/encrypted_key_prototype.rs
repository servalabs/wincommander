// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only lifecycle experiment. No production store or cutover is wired.
use rand::{rngs::OsRng, RngCore};
use std::io;
use zeroize::Zeroizing;

const MAX_PROTECTED: usize = 65536;
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
fn load_or_create(store: &mut impl KeyStore, protector: &impl Protector, has_index: bool) -> io::Result<Zeroizing<[u8; 32]>> {
    match store.read()? {
        Some(protected) => {
            if protected.is_empty() || protected.len() > MAX_PROTECTED { return Err(invalid()); }
            let plain = protector.unprotect(&protected)?;
            let mut key = Zeroizing::new([0; 32]);
            if plain.len() != key.len() { return Err(invalid()); }
            key.copy_from_slice(&plain);
            Ok(key)
        }
        None if has_index => Err(io::Error::new(io::ErrorKind::NotFound, "existing search index key missing")),
        None => {
            let mut key = Zeroizing::new([0; 32]);
            OsRng.fill_bytes(key.as_mut());
            let protected = protector.protect(key.as_ref())?;
            if protected.is_empty() || protected.len() > MAX_PROTECTED { return Err(invalid()); }
            store.create_new(&protected)?;
            Ok(key)
        }
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
    struct Memory { value: Option<Vec<u8>>, fail_read: bool, conflict: bool }
    impl KeyStore for Memory {
        fn read(&self) -> io::Result<Option<Vec<u8>>> {
            if self.fail_read { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")); }
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
        let first = load_or_create(&mut store, &Stub, false).unwrap();
        let second = load_or_create(&mut store, &Stub, true).unwrap();
        assert_eq!(&*first, &*second);
    }
    #[test]
    fn missing_existing_key_and_permission_errors_never_create_key() {
        let mut store = Memory::default();
        assert_eq!(load_or_create(&mut store, &Stub, true).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert!(store.value.is_none());
        store.fail_read = true;
        assert_eq!(load_or_create(&mut store, &Stub, false).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(store.value.is_none());
    }
    #[test]
    fn corrupt_key_and_creation_race_are_errors_not_replacements() {
        let mut store = Memory { value: Some(vec![3; 31]), ..Default::default() };
        assert!(load_or_create(&mut store, &Stub, true).is_err());
        assert_eq!(store.value, Some(vec![3; 31]));
        let mut racing = Memory { conflict: true, ..Default::default() };
        assert_eq!(load_or_create(&mut racing, &Stub, false).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(racing.value.is_none());
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
