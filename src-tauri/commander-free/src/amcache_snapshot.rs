//! Scoped, non-persistent VSS snapshot for reading the Amcache hive and its
//! sibling transaction logs. No live hive is loaded or modified.
use std::path::Path;

#[cfg(windows)]
pub(crate) fn with_amcache_snapshot<T>(read: impl FnOnce(&Path) -> Result<T, String>) -> Result<T, String> {
    use std::{ffi::{c_void, OsString}, os::windows::ffi::OsStringExt, path::PathBuf, ptr};
    unsafe extern "C" {
        fn wc_amcache_snapshot_open(handle: *mut *mut c_void, path: *mut u16, capacity: u32, stage: *mut u32) -> i32;
        fn wc_amcache_snapshot_close(handle: *mut c_void) -> i32;
    }
    struct Snapshot(*mut c_void);
    impl Drop for Snapshot {
        fn drop(&mut self) { if !self.0.is_null() { unsafe { wc_amcache_snapshot_close(self.0); } } }
    }
    let mut handle = ptr::null_mut();
    let mut path = vec![0u16; 32768];
    let mut stage = 0;
    let hr = unsafe { wc_amcache_snapshot_open(&mut handle, path.as_mut_ptr(), path.len() as u32, &mut stage) };
    if hr < 0 || handle.is_null() {
        if hr as u32 == 0x8007_0070 {
            return Err("At least 1 GiB of free space is required on the Windows drive before creating a temporary snapshot.".into());
        }
        return Err(format!("Unable to create a temporary volume snapshot (stage {stage}, Windows error 0x{:08X}). Administrator access and VSS are required.", hr as u32));
    }
    let mut snapshot = Snapshot(handle);
    let len = path.iter().position(|c| *c == 0).ok_or("Invalid snapshot path")?;
    let hive = PathBuf::from(OsString::from_wide(&path[..len]));
    let result = read(&hive);
    let close_status = unsafe { wc_amcache_snapshot_close(snapshot.0) };
    snapshot.0 = ptr::null_mut();
    if close_status < 0 {
        return Err(format!("The temporary snapshot could not be confirmed removed (Windows error 0x{:08X}).", close_status as u32));
    }
    result
}

#[cfg(not(windows))]
pub(crate) fn with_amcache_snapshot<T>(_read: impl FnOnce(&Path) -> Result<T, String>) -> Result<T, String> {
    Err("Temporary Windows volume snapshots are unavailable on this system.".into())
}
