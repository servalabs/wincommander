// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if commander_calculator_launcher::launch(std::env::args_os().skip(1)).is_err() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let title: Vec<u16> = "Calculator\0".encode_utf16().collect();
        let message: Vec<u16> =
            "Calculator could not open. Repair the installed application and try again.\0"
                .encode_utf16()
                .collect();
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    std::process::exit(1);
}
