// SPDX-License-Identifier: AGPL-3.0-or-later
//! Windows publication without replacing an existing destination.
use std::{io, path::Path};

#[cfg(windows)]
pub(super) fn move_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut result: Vec<u16> = path.as_os_str().encode_wide().collect();
        if result.contains(&0) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL")); }
        result.push(0);
        Ok(result)
    }
    let source = wide(source)?;
    let target = wide(target)?;
    // MoveFileW fails if the target exists, including a concurrent creator.
    if unsafe { MoveFileW(source.as_ptr(), target.as_ptr()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// Safe Paste is a Windows product feature. Preserve the existing non-Windows
// test behavior; callers must not rely on this fallback for concurrency safety.
#[cfg(not(windows))]
pub(super) fn move_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(target).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination exists"));
    }
    std::fs::rename(source, target)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn competing_publishers_never_replace_the_winner() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = [first.clone(), second.clone()].into_iter().map(|source| {
            let target = target.clone(); let barrier = barrier.clone();
            std::thread::spawn(move || { barrier.wait(); move_no_replace(&source, &target) })
        }).collect();
        barrier.wait();
        assert_eq!(workers.into_iter().map(|w| w.join().unwrap()).filter(Result::is_ok).count(), 1);
        assert_eq!(first.exists() as u8 + second.exists() as u8, 1);
        assert!([b"first".as_slice(), b"second".as_slice()].contains(&std::fs::read(target).unwrap().as_slice()));
    }

    #[test]
    fn publishes_into_long_windows_path() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("a".repeat(90)).join("b".repeat(90)).join("c".repeat(90));
        std::fs::create_dir_all(&parent).unwrap();
        let source = root.path().join("source");
        let target = parent.join("target");
        std::fs::write(&source, b"fixture").unwrap();
        move_no_replace(&source, &target).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"fixture");
    }

    #[test]
    fn existing_directory_and_its_contents_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source"); let target = root.path().join("target");
        std::fs::create_dir(&source).unwrap(); std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"keep").unwrap();
        assert!(move_no_replace(&source, &target).is_err());
        assert!(source.is_dir());
        assert_eq!(std::fs::read(target.join("keep")).unwrap(), b"keep");
    }
}
