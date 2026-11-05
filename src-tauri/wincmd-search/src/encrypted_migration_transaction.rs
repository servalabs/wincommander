// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental per-installer intent. Power-loss directory durability and
//! the installed commit/rollback protocol require native validation.
use std::{io, path::Path};
use serde::{Deserialize, Serialize};
use crate::{encrypted_key_prototype::{Protector, UserDpapi}, read_only::{AccessMode, CheckedDirectory}};

const NAME: &str = "migration-intent.v1";
const MAGIC: &str = "WINCOMMANDER-SEARCH-MIGRATION-1";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Intent {
    magic: String,
    pub transaction: [u8; 16],
    pub generation: [u8; 16],
    pub volume: u32,
    pub file_high: u32,
    pub file_low: u32,
    pub replacement_pending: bool,
}
impl Intent {
    pub(crate) fn capture(transaction: [u8; 16], generation: [u8; 16], old: &Path) -> io::Result<Self> {
        if transaction == [0; 16] || generation == [0; 16] { return Err(invalid()); }
        let (volume, file_high, file_low) = identity(old)?;
        Ok(Self { magic: MAGIC.into(), transaction, generation, volume, file_high, file_low, replacement_pending: false })
    }
    pub(crate) fn matches_path(&self, path: &Path) -> io::Result<bool> {
        Ok(identity(path)? == (self.volume, self.file_high, self.file_low))
    }
}
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "invalid search migration intent") }
fn identity(path: &Path) -> io::Result<(u32, u32, u32)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    let pins = CheckedDirectory::pin_root_path(path)?;
    let root = pins.last().ok_or_else(invalid)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: pinned directory and output struct remain valid.
    if unsafe { GetFileInformationByHandle(root.as_raw_handle(), &mut info) } == 0 { return Err(io::Error::last_os_error()); }
    Ok((info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
}
pub(crate) struct Store { dir: CheckedDirectory, _pins: Vec<std::fs::File> }
impl Store {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        let pins = CheckedDirectory::pin_root_path(root)?;
        let dir = CheckedDirectory::open(root, AccessMode::Writable).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { dir, _pins: pins })
    }
    pub(crate) fn load(&self) -> io::Result<Option<Intent>> {
        use tantivy::directory::error::OpenReadError;
        let data = match self.dir.atomic_read_bounded(Path::new(NAME), 8192) {
            Ok(bytes) => bytes,
            Err(OpenReadError::FileDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(io::Error::other(e.to_string())),
        };
        let plain = UserDpapi.unprotect(&data)?;
        let value: Intent = serde_json::from_slice(&plain).map_err(|_| invalid())?;
        if value.magic != MAGIC || value.transaction == [0; 16] || value.generation == [0; 16] { return Err(invalid()); }
        Ok(Some(value))
    }
    pub(crate) fn prepare(&self, intent: &Intent) -> io::Result<()> {
        if let Some(existing) = self.load()? {
            return if &existing == intent { Ok(()) } else { Err(invalid()) };
        }
        let plain = serde_json::to_vec(intent).map_err(|_| invalid())?;
        let protected = UserDpapi.protect(&plain)?;
        if protected.len() > 8192 { return Err(invalid()); }
        self.dir.atomic_create_pinned(Path::new(NAME), &protected)?;
        if self.load()?.as_ref() != Some(intent) { return Err(invalid()); }
        Ok(())
    }
    pub(crate) fn mark_replacement(&self, transaction: &[u8; 16]) -> io::Result<()> {
        let mut current = self.load()?.ok_or_else(invalid)?;
        if &current.transaction != transaction { return Err(invalid()); }
        if current.replacement_pending { return Ok(()); }
        current.replacement_pending = true;
        let plain = serde_json::to_vec(&current).map_err(|_| invalid())?;
        let protected = UserDpapi.protect(&plain)?;
        if protected.len() > 8192 { return Err(invalid()); }
        self.dir.atomic_write_pinned(Path::new(NAME), &protected)?;
        if self.load()?.as_ref() != Some(&current) { return Err(invalid()); }
        Ok(())
    }
    pub(crate) fn clear_matching(&self, transaction: &[u8; 16]) -> io::Result<()> {
        let current = self.load()?.ok_or_else(invalid)?;
        if &current.transaction != transaction { return Err(invalid()); }
        self.dir.delete_pinned(Path::new(NAME)).map_err(|e| io::Error::other(e.to_string()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_intent_rejects_other_transactions_and_tracks_directory_identity() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old"); let other = root.path().join("other");
        let selection = root.path().join("selection");
        std::fs::create_dir(&old).unwrap(); std::fs::create_dir(&other).unwrap(); std::fs::create_dir(&selection).unwrap();
        let intent = Intent::capture([1; 16], [2; 16], &old).unwrap();
        let store = Store::open(&selection).unwrap();
        store.prepare(&intent).unwrap(); store.prepare(&intent).unwrap();
        assert_eq!(store.load().unwrap(), Some(intent.clone()));
        assert!(intent.matches_path(&old).unwrap()); assert!(!intent.matches_path(&other).unwrap());
        assert!(store.clear_matching(&[3; 16]).is_err());
        assert!(store.mark_replacement(&[3; 16]).is_err());
        store.mark_replacement(&[1; 16]).unwrap();
        assert!(store.load().unwrap().unwrap().replacement_pending);
        let ciphertext = std::fs::read(selection.join(NAME)).unwrap();
        assert!(!ciphertext.windows(MAGIC.len()).any(|v| v == MAGIC.as_bytes()));
        store.clear_matching(&[1; 16]).unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
