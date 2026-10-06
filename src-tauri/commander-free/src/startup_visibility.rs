// SPDX-License-Identifier: AGPL-3.0-or-later
//! Establish native visibility before startup work or tray callbacks can run.

pub(crate) fn enforce_hidden_before_setup(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
        SWP_NOZORDER,
    };

    // Tao can skip hide when its cached visibility differs from the native window.
    let hidden = unsafe {
        SetWindowPos(
            window.hwnd()?.0 as _,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_HIDEWINDOW
                | SWP_NOACTIVATE
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER,
        )
    };
    if hidden == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
