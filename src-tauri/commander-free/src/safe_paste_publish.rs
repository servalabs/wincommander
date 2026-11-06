// SPDX-License-Identifier: AGPL-3.0-or-later
// Publish without replacing an existing destination. The source and target
// must reside on the same filesystem; failure leaves the source in place.
#[cfg(windows)]
pub(crate) fn move_no_replace(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileW(existing: *const u16, new: *const u16) -> i32;
    }
    let wide = |path: &std::path::Path| -> std::io::Result<Vec<u16>> {
        let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let source = wide(source)?;
    let target = wide(target)?;
    if unsafe { MoveFileW(source.as_ptr(), target.as_ptr()) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::move_no_replace;
    #[test]
    fn concurrent_publish_never_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let handles: Vec<_> = [first.clone(), second.clone()]
            .into_iter()
            .map(|source| {
                let barrier = barrier.clone();
                let target = target.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    move_no_replace(&source, &target)
                })
            })
            .collect();
        barrier.wait();
        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(Result::is_ok)
            .count();
        assert_eq!(successes, 1);
        assert_eq!(first.exists() as u8 + second.exists() as u8, 1);
        let contents = std::fs::read(target).unwrap();
        assert!(contents == b"first" || contents == b"second");
    }
}
