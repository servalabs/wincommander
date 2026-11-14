// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disabled experiment: cooperating per-generation Windows leases.
//!
//! A verified same-user protected parent OUTSIDE every generation is required.
//! The caller must verify owner and DACL on the supplied opened handles. No
//! production verifier, selector, trusted authority or legacy exclusion is
//! provided here. This lock alone never authorizes deletion.
use std::{fs::File, io, path::Path};
use std::os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle};

/// Hold until after the reader has finished publishing; an exclusive lease
/// coordinates only other participants using this exact generation lock.
pub struct GenerationLease { _pins: Vec<File>, _lock: File }

impl GenerationLease {
    pub fn shared(parent: &Path, generation: [u8;16], verify: impl Fn(&File) -> io::Result<()>) -> io::Result<Self> {
        Self::acquire(parent, generation, false, verify)
    }
    pub fn exclusive(parent: &Path, generation: [u8;16], verify: impl Fn(&File) -> io::Result<()>) -> io::Result<Self> {
        Self::acquire(parent, generation, true, verify)
    }
    fn acquire(parent: &Path, generation: [u8;16], exclusive: bool, verify: impl Fn(&File) -> io::Result<()>) -> io::Result<Self> {
        let denied=||io::Error::new(io::ErrorKind::PermissionDenied,"search generation lease unavailable");
        if generation == [0;16] { return Err(denied()); }
        let pins=crate::read_only::CheckedDirectory::pin_root_path(parent)?;
        verify(pins.last().ok_or_else(denied)?)?;
        let path=parent.join(format!(".ordinary-search-generation-{}.lock",hex::encode(generation)));
        let file=std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
        let meta=file.metadata()?;
        let mut info=BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the open file handle and output buffer remain valid.
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileInformationByHandle(file.as_raw_handle(),&mut info) } == 0
            || info.nNumberOfLinks != 1 { return Err(denied()); }
        verify(&file)?;
        let lock=if exclusive { file.try_lock() } else { file.try_lock_shared() };
        lock.map_err(|error| match error {
            std::fs::TryLockError::WouldBlock=>io::Error::new(io::ErrorKind::WouldBlock,"search generation is in use"),
            std::fs::TryLockError::Error(error)=>error,
        })?;
        Ok(Self { _pins:pins,_lock:file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration,Instant};
    fn synthetic(_: &File)->io::Result<()> { Ok(()) }
    #[test]
    fn rejects_unverified_parent_and_lock() {
        let dir=tempfile::tempdir().unwrap();
        assert!(GenerationLease::shared(dir.path(),[0;16],synthetic).is_err());
        assert!(GenerationLease::shared(dir.path(),[1;16],|_|Err(io::ErrorKind::PermissionDenied.into())).is_err());
        let calls=std::cell::Cell::new(0);
        assert!(GenerationLease::shared(dir.path(),[1;16],|_| {
            calls.set(calls.get()+1);
            if calls.get()==2 { Err(io::ErrorKind::PermissionDenied.into()) } else { Ok(()) }
        }).is_err());
        assert_eq!(calls.get(),2);
    }
    #[test]
    fn shared_leases_block_exclusive_until_crashed_child_exits() {
        const ENV:&str="WINCMD_TEST_ORDINARY_GENERATION_LEASE";
        if let Some(parent)=std::env::var_os(ENV) {
            let parent=std::path::PathBuf::from(parent);
            let _lease=GenerationLease::shared(&parent,[1;16],synthetic).unwrap();
            std::fs::write(parent.join("child-ready"),b"ready").unwrap();
            std::thread::sleep(Duration::from_secs(20));
            return;
        }
        struct Child(std::process::Child);
        impl Drop for Child { fn drop(&mut self) { let _=self.0.kill();let _=self.0.wait(); } }
        let dir=tempfile::tempdir().unwrap();
        let first=GenerationLease::shared(dir.path(),[1;16],synthetic).unwrap();
        let mut child=Child(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","generation_lease::tests::shared_leases_block_exclusive_until_crashed_child_exits"])
            .env(ENV,dir.path()).spawn().unwrap());
        let deadline=Instant::now()+Duration::from_secs(10);
        while !dir.path().join("child-ready").exists() {
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(Instant::now()<deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(GenerationLease::exclusive(dir.path(),[1;16],synthetic).is_err());
        drop(first);
        assert!(GenerationLease::exclusive(dir.path(),[1;16],synthetic).is_err());
        child.0.kill().unwrap();child.0.wait().unwrap();
        assert!(GenerationLease::exclusive(dir.path(),[1;16],synthetic).is_ok());
    }
}
