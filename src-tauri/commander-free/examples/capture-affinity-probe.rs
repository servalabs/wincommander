// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disposable RDP comparison: a redundant affinity write versus production reconciliation.
#![windows_subsystem = "windows"]

#[path = "../src/capture_protection.rs"]
mod capture_protection;

use std::{
    io::Write,
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .ok_or("Supply a new evidence file")?;
    let mut receipt = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    let class = wide("STATIC");
    let titles = [
        "Capture probe: redundant write (negative control)",
        "Capture probe: production fix (must stay hidden)",
        "Capture probe: untouched control (must stay hidden)",
    ];
    let mut windows = Vec::new();
    for (index, title) in titles.iter().enumerate() {
        // These are fresh process-owned disposable windows, never application HWNDs.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                wide(title).as_ptr(),
                WS_OVERLAPPEDWINDOW,
                30 + (index as i32 * 450),
                300,
                430,
                220,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        windows.push(hwnd);
    }
    let started = Instant::now();
    let mut applied = false;
    while started.elapsed() < Duration::from_secs(45) {
        let mut message = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if !applied && started.elapsed() >= Duration::from_secs(2) {
            let baseline = unsafe { SetWindowDisplayAffinity(windows[0], WDA_NONE) };
            let fixed = capture_protection::apply(windows[1], false);
            writeln!(
                receipt,
                "baseline_set_success={} production_result={fixed:?}",
                baseline != 0
            )?;
            for (index, hwnd) in windows.iter().enumerate() {
                writeln!(receipt, "window={index} native_visible={}", unsafe {
                    IsWindowVisible(*hwnd) != 0
                })?;
            }
            receipt.flush()?;
            applied = true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for hwnd in windows {
        unsafe {
            DestroyWindow(hwnd);
        }
    }
    writeln!(receipt, "completed; all disposable windows destroyed")?;
    Ok(())
}
