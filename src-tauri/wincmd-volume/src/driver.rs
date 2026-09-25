// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::native::{wide, Handle};
use windows_sys::Win32::{Storage::FileSystem::*, System::IO::DeviceIoControl};
use zeroize::Zeroizing;

// VeraCrypt 1.26 Apidrvr.h, packed VOLUME_PROPERTIES_STRUCT (TC_MAX_PATH=260).
// https://github.com/veracrypt/VeraCrypt/blob/VeraCrypt_1.26.24/src/Common/Apidrvr.h
const DRIVER_VERSION: u32 = 0x0126;
const PROPERTIES_SIZE: usize = 706;
const ID_OFFSET: usize = 670;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Properties {
    pub stable_id: String,
    pub generation: u32,
    pub read_only: bool,
}

pub(crate) fn properties(slot: u8) -> Result<Properties, String> {
    let name = wide(r"\\.\VeraCrypt")?;
    let driver = Handle::new(unsafe {
        CreateFileW(
            name.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    })?;
    let mut version = [0u8; 4];
    query(&driver, 0x0022_2004, &mut version)?;
    if u32::from_le_bytes(version) != DRIVER_VERSION {
        return Err("Unsupported VeraCrypt driver version".into());
    }
    // The driver returns a container path; wipe it rather than retaining or logging it.
    let mut buffer = Zeroizing::new([0u8; PROPERTIES_SIZE]);
    buffer[..4].copy_from_slice(&(slot as u32).to_le_bytes());
    query(&driver, 0x0022_201c, &mut buffer[..])?;
    decode(&buffer[..], slot)
}

fn query(driver: &Handle, code: u32, buffer: &mut [u8]) -> Result<(), String> {
    let mut returned = 0;
    let length = buffer.len() as u32;
    let success = unsafe {
        DeviceIoControl(
            driver.0,
            code,
            buffer.as_ptr().cast(),
            length,
            buffer.as_mut_ptr().cast(),
            length,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if success == 0 || returned != length {
        return Err("VeraCrypt identity query unavailable".into());
    }
    Ok(())
}

fn decode(buffer: &[u8], slot: u8) -> Result<Properties, String> {
    if buffer.len() != PROPERTIES_SIZE || slot > 25 {
        return Err("Invalid VeraCrypt identity response".into());
    }
    let word = |offset| u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
    let id = &buffer[ID_OFFSET..ID_OFFSET + 32];
    if word(0) != u32::from(slot) || id.iter().all(|byte| *byte == 0) || word(556) > 1 {
        return Err("Invalid VeraCrypt identity response".into());
    }
    let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Properties {
        stable_id: format!("veracrypt:{hex}"),
        generation: word(4),
        read_only: word(556) != 0 || word(588) == 2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> [u8; PROPERTIES_SIZE] {
        let mut data = [0; PROPERTIES_SIZE];
        data[..4].copy_from_slice(&12u32.to_le_bytes());
        data[4..8].copy_from_slice(&41u32.to_le_bytes());
        data[ID_OFFSET..ID_OFFSET + 32].fill(0xab);
        data
    }

    #[test]
    fn binds_mount_generation_separately_from_stable_header_identity() {
        let data = response();
        let first = decode(&data, 12).unwrap();
        let mut remounted = data;
        remounted[4..8].copy_from_slice(&42u32.to_le_bytes());
        let second = decode(&remounted, 12).unwrap();
        assert_eq!(first.stable_id, format!("veracrypt:{}", "ab".repeat(32)));
        assert_eq!(first.stable_id, second.stable_id);
        assert_ne!(first.generation, second.generation);
    }

    #[test]
    fn rejects_wrong_slot_truncated_response_and_missing_identity() {
        let mut data = response();
        assert!(decode(&data, 11).is_err());
        assert!(decode(&data[..702], 12).is_err());
        data[ID_OFFSET..ID_OFFSET + 32].fill(0);
        assert!(decode(&data, 12).is_err());
    }

    #[test]
    fn honors_driver_read_only_and_hidden_volume_write_protection() {
        let mut data = response();
        assert!(!decode(&data, 12).unwrap().read_only);
        data[556..560].copy_from_slice(&1u32.to_le_bytes());
        assert!(decode(&data, 12).unwrap().read_only);
        data[556..560].fill(0);
        data[588..592].copy_from_slice(&2u32.to_le_bytes());
        assert!(decode(&data, 12).unwrap().read_only);
    }
}
