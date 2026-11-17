// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{ActivationRecord, ActivationStatus};
use std::ffi::c_void;
use zeroize::Zeroizing;

const TELEMETRY_CLASS: u32 = 64;
const TELEMETRY_HEADER_BYTES: usize = 96;
const MAX_TELEMETRY_BYTES: usize = 256 * 1024;
const LENGTH_MISMATCH: i32 = 0xc0000004u32 as i32;
const BUFFER_TOO_SMALL: i32 = 0xc0000023u32 as i32;

#[repr(C)]
#[derive(Default)]
struct BootEnvironment {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
    firmware_type: u32,
    boot_flags: u64,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        data: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
    fn NtQueryInformationProcess(
        process: *mut c_void,
        class: u32,
        data: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

fn unavailable() -> String {
    "Windows boot-session identity unavailable".into()
}

fn telemetry_sequence(bytes: &[u8], returned: u32, process_id: u32) -> Option<u32> {
    let returned = returned as usize;
    if returned < TELEMETRY_HEADER_BYTES || returned > bytes.len() {
        return None;
    }
    let number = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let header = number(0) as usize;
    if header < TELEMETRY_HEADER_BYTES || header > returned || number(4) != process_id {
        return None;
    }
    let sequence = number(60);
    (sequence != 0).then_some(sequence)
}

fn query_sequence(
    process_id: u32,
    mut query: impl FnMut(&mut [u8], &mut u32) -> i32,
) -> Result<u32, String> {
    let mut capacity = 4096;
    for _ in 0..4 {
        // Telemetry includes the process command line; never retain or report those bytes.
        let mut bytes = Zeroizing::new(vec![0u8; capacity]);
        let mut returned = 0;
        let status = query(&mut bytes, &mut returned);
        if status == 0 {
            return telemetry_sequence(&bytes, returned, process_id).ok_or_else(unavailable);
        }
        if !matches!(status, LENGTH_MISMATCH | BUFFER_TOO_SMALL) {
            return Err(unavailable());
        }
        capacity = if returned as usize > capacity {
            returned as usize
        } else {
            capacity.checked_mul(2).ok_or_else(unavailable)?
        };
        if capacity > MAX_TELEMETRY_BYTES {
            return Err(unavailable());
        }
    }
    Err(unavailable())
}

/// Returns the immutable loader namespace and kernel boot sequence, never wall-clock time.
pub fn current_boot_id() -> Result<String, String> {
    let sequence = query_sequence(std::process::id(), |bytes, returned| {
        // The -1 pseudo-handle selects only this process; the buffer lives through the call.
        unsafe {
            NtQueryInformationProcess(
                -1isize as *mut c_void,
                TELEMETRY_CLASS,
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                returned,
            )
        }
    })?;
    let mut data = BootEnvironment::default();
    let mut returned = 0;
    // Nt90 identifies the BCD entry; it cannot establish a later kernel boot by itself.
    let status = unsafe {
        NtQuerySystemInformation(
            90,
            (&mut data as *mut BootEnvironment).cast(),
            std::mem::size_of::<BootEnvironment>() as u32,
            &mut returned,
        )
    };
    if status != 0 || returned as usize != std::mem::size_of::<BootEnvironment>() {
        return Err(unavailable());
    }
    let boot = format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}/{sequence:08x}",
        data.data1,
        data.data2,
        data.data3,
        data.data4[0],
        data.data4[1],
        data.data4[2],
        data.data4[3],
        data.data4[4],
        data.data4[5],
        data.data4[6],
        data.data4[7]
    );
    ActivationRecord::new(&"0".repeat(64), &boot, ActivationStatus::Active)?;
    Ok(boot)
}

#[cfg(test)]
#[path = "boot_session_tests.rs"]
mod tests;
