// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const UPDATE_BUSY: &str = "PRO_UPDATE_IN_PROGRESS:WinCommander Pro is being updated. Wait for the update to finish, then try again.";

fn require_available(marker: &Path) -> Result<(), String> {
    match marker.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(UPDATE_BUSY.into()),
        Err(error) => Err(format!("Pro update status could not be checked: {error}")),
    }
}

pub(crate) struct ImageLease(File);

#[cfg(windows)]
fn open_image(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
}

#[cfg(windows)]
fn lock_image(file: File, exclusive: bool) -> Result<ImageLease, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    let mut offset: OVERLAPPED = unsafe { std::mem::zeroed() };
    // Coordinate on a byte far beyond EOF so hashing and image loading still work.
    offset.Anonymous.Anonymous.OffsetHigh = 0x7fff_ffff;
    let flags = LOCKFILE_FAIL_IMMEDIATELY
        | if exclusive {
            LOCKFILE_EXCLUSIVE_LOCK
        } else {
            0
        };
    if unsafe { LockFileEx(file.as_raw_handle(), flags, 0, 1, 0, &mut offset) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(33) {
            UPDATE_BUSY.into()
        } else {
            format!("Pro operation lock could not be acquired: {error}")
        });
    }
    Ok(ImageLease(file))
}

impl Drop for ImageLease {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
            let mut offset: windows_sys::Win32::System::IO::OVERLAPPED =
                unsafe { std::mem::zeroed() };
            offset.Anonymous.Anonymous.OffsetHigh = 0x7fff_ffff;
            unsafe {
                UnlockFileEx(self.0.as_raw_handle(), 0, 1, 0, &mut offset);
            }
        }
    }
}

pub(crate) fn operation_lease(
    path: Option<&Path>,
    marker: &Path,
) -> Result<Option<ImageLease>, String> {
    #[cfg(windows)]
    {
        require_available(marker)?;
        let Some(path) = path else {
            return Ok(None);
        };
        let file =
            open_image(&path).map_err(|error| format!("Pro image could not be opened: {error}"))?;
        let lease = lock_image(file, false)?;
        // Closes the race with a maintenance request arriving between probe and lease.
        require_available(marker)?;
        Ok(Some(lease))
    }
    #[cfg(not(windows))]
    {
        let _ = (path, marker);
        Ok(None)
    }
}

pub(crate) struct Maintenance {
    _marker: File,
    _image: Option<ImageLease>,
}

#[cfg(windows)]
fn create_marker(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    };
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                UPDATE_BUSY.into()
            } else {
                format!("Pro update could not reserve maintenance: {error}")
            }
        })
}

pub(crate) fn reserve_install(path: &Path) -> Result<File, String> {
    #[cfg(windows)]
    {
        create_marker(&path.with_extension("installing"))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Pro updates require Windows".into())
    }
}

impl Maintenance {
    pub(crate) async fn begin(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        {
            let marker = create_marker(&path.with_extension("maintenance"))?;
            let deadline = Instant::now() + Duration::from_secs(15);
            let image = loop {
                let file = match open_image(path) {
                    Ok(file) => file,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break None,
                    Err(error) => {
                        return Err(format!(
                            "Pro update could not open the installed image: {error}"
                        ))
                    }
                };
                match lock_image(file, true) {
                    Ok(lease) => break Some(lease),
                    Err(error) if error == UPDATE_BUSY && Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(error) if error == UPDATE_BUSY => return Err("Pro is still completing an operation. Wait for it to finish before updating. The installed version was preserved.".into()),
                    Err(error) => return Err(error),
                }
            };
            Ok(Self {
                _marker: marker,
                _image: image,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err("Pro maintenance requires Windows".into())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn shared_operations_drain_before_exclusive_update_and_resume_afterwards() {
        let path = temporary_path("pro-lease");
        std::fs::write(&path, b"unchanged image").unwrap();
        let first = lock_image(open_image(&path).unwrap(), false).unwrap();
        let second = lock_image(open_image(&path).unwrap(), false).unwrap();
        assert!(lock_image(open_image(&path).unwrap(), true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"unchanged image");
        drop(first);
        assert!(lock_image(open_image(&path).unwrap(), true).is_err());
        drop(second);
        let update = lock_image(open_image(&path).unwrap(), true).unwrap();
        assert!(lock_image(open_image(&path).unwrap(), false).is_err());
        drop(update);
        drop(lock_image(open_image(&path).unwrap(), false).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn maintenance_marker_blocks_other_clients_and_disappears_on_handle_close() {
        let path = temporary_path("pro-maintenance");
        let guard = create_marker(&path).unwrap();
        assert_eq!(require_available(&path).unwrap_err(), UPDATE_BUSY);
        assert!(create_marker(&path).is_err());
        drop(guard);
        require_available(&path).unwrap();
    }

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{label}-{}-{stamp}", std::process::id()))
    }
}
