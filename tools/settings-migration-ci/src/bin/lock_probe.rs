use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
fn main() {
    let Some(path) = std::env::args_os().nth(1) else { std::process::exit(1) };
    let Ok(file) = OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).open(path) else { std::process::exit(1) };
    std::process::exit(if file.try_lock_shared().is_ok() { 0 } else { 2 });
}
