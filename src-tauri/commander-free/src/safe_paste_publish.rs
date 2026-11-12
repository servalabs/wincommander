// SPDX-License-Identifier: AGPL-3.0-or-later
//! Windows publication without replacing an existing destination.
use std::{io, path::Path};

#[cfg(windows)]
fn native_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let raw: Vec<u16> = path.as_os_str().encode_wide().collect();
    if raw.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"));
    }
    let slash = |unit: u16| unit == b'\\' as u16 || unit == b'/' as u16;
    let verbatim = raw.starts_with(&[92, 92, 63, 92]) || raw.starts_with(&[92, 63, 63, 92]);
    let short_drive = raw.len() >= 2 && raw[1] == b':' as u16 && !slash(raw[0])
        && (raw.len() == 2 || raw.get(2).is_some_and(|unit| slash(*unit)));
    let short_network = raw.len() >= 2 && slash(raw[0]) && slash(raw[1]);
    let mut result = if raw.is_empty() || verbatim || (raw.len() + 1 < 248 && (short_drive || short_network)) {
        raw
    } else {
        // Lexical Windows full-path resolution, not filesystem canonicalization:
        // junctions are not opened or followed by this conversion.
        let absolute: Vec<u16> = std::path::absolute(path)?.as_os_str().encode_wide().collect();
        let mut prefix = Vec::new();
        let remainder = if absolute.starts_with(&[92, 92, 63, 92]) || absolute.starts_with(&[92, 63, 63, 92]) {
            absolute.as_slice()
        } else if absolute.starts_with(&[92, 92, 46, 92]) {
            prefix.extend([92, 92, 63, 92]);
            &absolute[4..]
        } else if absolute.starts_with(&[92, 92]) {
            prefix.extend([92, 92, 63, 92, 85, 78, 67, 92]);
            &absolute[2..]
        } else if absolute.len() >= 3 && absolute[1] == 58 && absolute[2] == 92 {
            prefix.extend([92, 92, 63, 92]);
            absolute.as_slice()
        } else {
            absolute.as_slice()
        };
        prefix.extend_from_slice(remainder);
        prefix
    };
    result.push(0);
    Ok(result)
}

#[cfg(windows)]
pub(super) fn move_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let source = native_path(source)?;
    let target = native_path(target)?;
    // With no flags, the destination cannot be replaced and cross-volume
    // copy/delete is forbidden; the operation remains a same-volume move.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } == 0 {
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
    fn path_encoding_preserves_short_and_verbatim_inputs() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        fn output(path: &str) -> String {
            let mut wide = native_path(Path::new(path)).unwrap();
            assert_eq!(wide.pop(), Some(0));
            String::from_utf16(&wide).unwrap()
        }
        assert_eq!(output(r"C:\short\item"), r"C:\short\item");
        assert_eq!(output(r"\\?\C:\name.\..\item"), r"\\?\C:\name.\..\item");
        assert_eq!(output(r"\??\C:\name.\..\item"), r"\??\C:\name.\..\item");
        assert_eq!(output(r"\\?\UNC\server\share\file"), r"\\?\UNC\server\share\file");
        let nul = OsString::from_wide(&[b'a' as u16, 0, b'b' as u16]);
        assert_eq!(native_path(Path::new(&nul)).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn long_drive_and_unc_names_are_prefixed_without_filesystem_io() {
        let drive = format!(r"C:\{}\item", "a".repeat(260));
        let unc = format!(r"\\server\share\{}\item", "b".repeat(260));
        for (input, expected_prefix) in [(&drive, r"\\?\C:\"), (&unc, r"\\?\UNC\server\share\")] {
            let mut output = native_path(Path::new(input)).unwrap();
            assert_eq!(output.pop(), Some(0));
            let output = String::from_utf16(&output).unwrap();
            assert!(output.starts_with(expected_prefix));
            assert!(output.ends_with(r"\item"));
        }
    }

    #[test]
    fn relative_paths_use_the_same_lexical_absolute_resolution() {
        use std::os::windows::ffi::OsStrExt;
        let input = Path::new(r"subdir\..\item");
        let absolute = std::path::absolute(input).unwrap();
        let output = native_path(input).unwrap();
        let absolute_wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        // The conversion adds a verbatim drive or UNC prefix, not a filesystem lookup.
        assert!(output.windows(absolute_wide.len().saturating_sub(2))
            .any(|window| window == &absolute_wide[2..]));
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
