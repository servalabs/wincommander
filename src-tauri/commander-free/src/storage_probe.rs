// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/storage_probe.rs
//
// READ-ONLY storage capability probe.
//
// ────────────────────────────────────────────────────────────────────────────
// PURPOSE
// ────────────────────────────────────────────────────────────────────────────
// For each fixed physical drive, DETECT which hardware secure-erase paths the
// drive advertises (ATA Security ERASE UNIT, ATA Sanitize, NVMe Sanitize, NVMe
// Format NVM crypto-erase) and whether the drive is in a state that makes a
// hardware erase impossible right now (external USB bridge, ATA security-frozen).
//
// This feeds a LATER, separate, currently-held secure-erase feature. This module
// is DETECTION ONLY.
//
// ────────────────────────────────────────────────────────────────────────────
// SAFETY CONTRACT — READ-ONLY
// ────────────────────────────────────────────────────────────────────────────
// The ONLY device commands issued here are:
//   - IOCTL_STORAGE_QUERY_PROPERTY  (pure metadata read)
//   - ATA IDENTIFY DEVICE (opcode 0xEC), DATA_IN, non-destructive
// NO destructive opcode (ATA SECURITY ERASE / SANITIZE, NVMe Sanitize / Format
// NVM) is ever assembled or issued. The drive handle is opened WITHOUT
// GENERIC_WRITE. Parsing is confined to pure functions so the flag derivation is
// unit-tested against fixture buffers with no hardware.
//
// SPEC OFFSETS (verified, not guessed):
//   NVMe Identify Controller (4096-byte) — per Microsoft NVME_IDENTIFY_CONTROLLER_DATA
//   (learn.microsoft.com/windows/win32/api/nvme/ns-nvme-nvme_identify_controller_data):
//     * OACS    @ byte 256 (u16 LE): bit1 = Format NVM command supported
//     * SANICAP @ byte 328 (u32 LE): bit0 = Crypto Erase, bit1 = Block Erase,
//                                    bit2 = Overwrite sanitize supported
//   ATA IDENTIFY DEVICE (512-byte, 256 words) — per smartmontools ataidentify.cpp
//   (github.com/mirror/smartmontools ataidentify.cpp) and ATA8-ACS:
//     * word 82 bit1  = Security feature set supported
//     * word 128 bit3 = Security frozen (SEC2/SEC6)
//     * word 128 bit5 = Enhanced security erase supported
//     * word 59 bit12 = Sanitize Device feature set supported

use serde::Serialize;

/// Non-destructive ATA IDENTIFY DEVICE opcode. This is the ONLY ATA command
/// this module ever issues — it reads the 512-byte identify block, no writes.
#[cfg(windows)]
const ATA_IDENTIFY_DEVICE: u8 = 0xEC;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DriveCapabilities {
    /// Physical drive index (\\.\PhysicalDriveN).
    pub drive_index: u32,
    /// "Nvme", "Sata", "Ata", "Usb", "Raid", "Sas", "Scsi", "Unknown", …
    pub bus_type: String,
    /// External USB bridge — hardware secure-erase is usually unavailable
    /// because the bridge chip does not pass ATA/NVMe admin commands through.
    pub is_usb_bridge: bool,
    pub ata_security_supported: bool,
    pub ata_enhanced_erase_supported: bool,
    /// Drive in SEC2/SEC6 frozen state — hardware erase impossible until a
    /// power-cycle clears the freeze.
    pub ata_frozen: bool,
    pub ata_sanitize_supported: bool,
    pub nvme_sanitize_supported: bool,
    pub nvme_crypto_erase_supported: bool,
    pub nvme_format_supported: bool,
    /// Per-drive error so one unreadable drive never fails the whole call.
    pub probe_error: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// PURE PARSERS (unit-tested; no hardware)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse the NVMe Identify Controller buffer.
/// Returns `(sanitize_supported, crypto_erase_supported, format_supported)`.
///
/// A short buffer is treated as "nothing supported" rather than panicking —
/// a truncated read must never claim a capability.
pub fn parse_nvme_identify(buf: &[u8]) -> (bool, bool, bool) {
    // OACS @ byte 256 (u16 LE); SANICAP @ byte 328 (u32 LE). Need through 331.
    if buf.len() < 332 {
        return (false, false, false);
    }
    let oacs = u16::from_le_bytes([buf[256], buf[257]]);
    let sanicap = u32::from_le_bytes([buf[328], buf[329], buf[330], buf[331]]);

    let format_supported = oacs & (1 << 1) != 0; // OACS bit1 = Format NVM
    let crypto_erase = sanicap & (1 << 0) != 0; // SANICAP bit0 = Crypto Erase
    let block_erase = sanicap & (1 << 1) != 0; // SANICAP bit1 = Block Erase
    let overwrite = sanicap & (1 << 2) != 0; // SANICAP bit2 = Overwrite
    let sanitize_supported = crypto_erase || block_erase || overwrite;

    (sanitize_supported, crypto_erase, format_supported)
}

/// Parse the ATA IDENTIFY DEVICE buffer (512 bytes / 256 little-endian words).
/// Returns `(security_supported, enhanced_erase_supported, frozen, sanitize_supported)`.
///
/// A short buffer is treated as "nothing supported / not frozen" rather than
/// panicking.
pub fn parse_ata_identify(buf: &[u8]) -> (bool, bool, bool, bool) {
    // Word 128 is the highest word we read → need 129 words = 258 bytes.
    if buf.len() < 258 {
        return (false, false, false, false);
    }
    let word = |i: usize| -> u16 { u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]) };

    let security_supported = word(82) & (1 << 1) != 0; // word 82 bit1
    let w128 = word(128);
    let frozen = w128 & (1 << 3) != 0; // word 128 bit3
    let enhanced_erase = w128 & (1 << 5) != 0; // word 128 bit5
    let sanitize_supported = word(59) & (1 << 12) != 0; // word 59 bit12

    (
        security_supported,
        enhanced_erase,
        frozen,
        sanitize_supported,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// TAURI COMMAND
// ─────────────────────────────────────────────────────────────────────────────

/// Probe fixed physical drives for hardware secure-erase capabilities.
///
/// If `drive_index` is `Some(n)`, probes only `\\.\PhysicalDriveN`; otherwise
/// enumerates fixed drives 0..=31 and returns every one that opens.
///
/// FREE tier — capability DETECTION only, issues no destructive command.
#[tauri::command]
pub fn probe_drive_capabilities(drive_index: Option<u32>) -> Vec<DriveCapabilities> {
    #[cfg(windows)]
    {
        windows_impl::probe(drive_index)
    }
    #[cfg(not(windows))]
    {
        let _ = drive_index;
        vec![DriveCapabilities {
            drive_index: 0,
            bus_type: "Unknown".to_string(),
            probe_error: Some(
                "storage capability probe is unsupported on this platform".to_string(),
            ),
            ..Default::default()
        }]
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::{parse_ata_identify, parse_nvme_identify, DriveCapabilities, ATA_IDENTIFY_DEVICE};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::Storage::IscsiDisc::{
        ATA_FLAGS_DATA_IN, ATA_FLAGS_DRDY_REQUIRED, ATA_PASS_THROUGH_EX, IOCTL_ATA_PASS_THROUGH,
    };
    use windows_sys::Win32::System::Ioctl::{
        NVMeDataTypeIdentify, PropertyStandardQuery, ProtocolTypeNvme,
        StorageAdapterProtocolSpecificProperty, StorageDeviceProperty,
        IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
        STORAGE_PROTOCOL_DATA_DESCRIPTOR, STORAGE_PROTOCOL_SPECIFIC_DATA,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    // STORAGE_BUS_TYPE values (Win32_Storage_FileSystem).
    use windows_sys::Win32::Storage::FileSystem::{
        BusTypeAta, BusTypeNvme, BusTypeRAID, BusTypeSas, BusTypeSata, BusTypeScsi, BusTypeUsb,
    };

    /// GENERIC_READ only — deliberately NOT GENERIC_WRITE. A read handle is all
    /// IOCTL_STORAGE_QUERY_PROPERTY and ATA IDENTIFY (DATA_IN) require, and it
    /// structurally prevents any accidental destructive IOCTL.
    const GENERIC_READ: u32 = 0x8000_0000;
    const NVME_IDENTIFY_LEN: usize = 4096;
    const ATA_IDENTIFY_LEN: usize = 512;

    /// RAII wrapper so every early-return path closes the device handle.
    struct DeviceHandle(HANDLE);
    impl Drop for DeviceHandle {
        fn drop(&mut self) {
            // SAFETY: self.0 is a valid, non-INVALID handle (checked at open).
            unsafe { CloseHandle(self.0) };
        }
    }

    fn bus_type_name(bus: i32) -> &'static str {
        match bus {
            x if x == BusTypeNvme => "Nvme",
            x if x == BusTypeSata => "Sata",
            x if x == BusTypeAta => "Ata",
            x if x == BusTypeUsb => "Usb",
            x if x == BusTypeRAID => "Raid",
            x if x == BusTypeSas => "Sas",
            x if x == BusTypeScsi => "Scsi",
            _ => "Unknown",
        }
    }

    fn open_drive(index: u32) -> Result<DeviceHandle, u32> {
        let path: Vec<u16> = OsStr::new(&format!("\\\\.\\PhysicalDrive{index}"))
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: path is a valid NUL-terminated wide string; all other args are
        // plain scalars / null. Read-only share so we never lock the disk.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(unsafe { GetLastError() });
        }
        Ok(DeviceHandle(handle))
    }

    /// IOCTL_STORAGE_QUERY_PROPERTY(StorageDeviceProperty) → BusType.
    fn query_bus_type(dev: HANDLE) -> Result<i32, String> {
        let mut query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceProperty,
            QueryType: PropertyStandardQuery,
            AdditionalParameters: [0u8; 1],
        };
        // The descriptor has a variable-length tail; a fixed 1 KiB scratch is
        // plenty for the fixed header we read (BusType).
        let mut out = [0u8; 1024];
        let mut returned: u32 = 0;
        // SAFETY: query/out are stack buffers valid for the call; sizes match.
        let ok = unsafe {
            DeviceIoControl(
                dev,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &mut query as *mut _ as *const core::ffi::c_void,
                std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                out.as_mut_ptr() as *mut core::ffi::c_void,
                out.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "IOCTL_STORAGE_QUERY_PROPERTY failed: error {}",
                unsafe { GetLastError() }
            ));
        }
        if (returned as usize) < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
            return Err("STORAGE_DEVICE_DESCRIPTOR truncated".to_string());
        }
        // SAFETY: `out` is aligned to 4 (u8 array, but we only read a repr(C)
        // struct via read_unaligned) and holds at least the descriptor header.
        let desc = unsafe { ptr::read_unaligned(out.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
        Ok(desc.BusType)
    }

    /// IOCTL_STORAGE_QUERY_PROPERTY(StorageAdapterProtocolSpecificProperty,
    /// ProtocolTypeNvme / NVMeDataTypeIdentify, RequestValue=1 = Identify
    /// Controller) → 4096-byte Identify Controller buffer.
    fn query_nvme_identify(dev: HANDLE) -> Result<Vec<u8>, String> {
        const HDR: usize = std::mem::size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>();
        let mut buf = vec![0u8; HDR + NVME_IDENTIFY_LEN];

        // Build the request header in-place at the front of `buf`.
        {
            let query = STORAGE_PROPERTY_QUERY {
                PropertyId: StorageAdapterProtocolSpecificProperty,
                QueryType: PropertyStandardQuery,
                AdditionalParameters: [0u8; 1],
            };
            // The AdditionalParameters area is actually a
            // STORAGE_PROTOCOL_SPECIFIC_DATA; write both pieces via unaligned
            // pointer writes so the on-wire layout matches the driver's expectation.
            // SAFETY: buf is large enough (HDR + payload) and the writes stay in bounds.
            unsafe {
                ptr::write_unaligned(buf.as_mut_ptr() as *mut STORAGE_PROPERTY_QUERY, query);
                let proto_off = std::mem::offset_of!(STORAGE_PROPERTY_QUERY, AdditionalParameters);
                let proto = STORAGE_PROTOCOL_SPECIFIC_DATA {
                    ProtocolType: ProtocolTypeNvme,
                    DataType: NVMeDataTypeIdentify as u32,
                    ProtocolDataRequestValue: 1, // 1 = Identify Controller (CNS 01h)
                    ProtocolDataRequestSubValue: 0,
                    ProtocolDataOffset: HDR as u32,
                    ProtocolDataLength: NVME_IDENTIFY_LEN as u32,
                    FixedProtocolReturnData: 0,
                    ProtocolDataRequestSubValue2: 0,
                    ProtocolDataRequestSubValue3: 0,
                    ProtocolDataRequestSubValue4: 0,
                };
                ptr::write_unaligned(
                    buf.as_mut_ptr().add(proto_off) as *mut STORAGE_PROTOCOL_SPECIFIC_DATA,
                    proto,
                );
            }
        }

        let mut out = vec![0u8; HDR + NVME_IDENTIFY_LEN];
        let mut returned: u32 = 0;
        // SAFETY: buf/out are heap buffers valid for the call; sizes passed match lengths.
        let ok = unsafe {
            DeviceIoControl(
                dev,
                IOCTL_STORAGE_QUERY_PROPERTY,
                buf.as_ptr() as *const core::ffi::c_void,
                buf.len() as u32,
                out.as_mut_ptr() as *mut core::ffi::c_void,
                out.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!("NVMe Identify query failed: error {}", unsafe {
                GetLastError()
            }));
        }
        if (returned as usize) < HDR + 332 {
            return Err("NVMe Identify Controller data truncated".to_string());
        }
        Ok(out[HDR..HDR + NVME_IDENTIFY_LEN].to_vec())
    }

    /// IOCTL_ATA_PASS_THROUGH issuing IDENTIFY DEVICE (0xEC, DATA_IN, non-destructive)
    /// → 512-byte IDENTIFY buffer.
    fn query_ata_identify(dev: HANDLE) -> Result<Vec<u8>, String> {
        const HDR: usize = std::mem::size_of::<ATA_PASS_THROUGH_EX>();
        // One contiguous buffer: [ATA_PASS_THROUGH_EX header][512-byte data].
        let mut buf = vec![0u8; HDR + ATA_IDENTIFY_LEN];

        let mut apt = ATA_PASS_THROUGH_EX {
            Length: HDR as u16,
            AtaFlags: (ATA_FLAGS_DATA_IN | ATA_FLAGS_DRDY_REQUIRED) as u16,
            PathId: 0,
            TargetId: 0,
            Lun: 0,
            ReservedAsUchar: 0,
            DataTransferLength: ATA_IDENTIFY_LEN as u32,
            TimeOutValue: 10,
            ReservedAsUlong: 0,
            DataBufferOffset: HDR,
            PreviousTaskFile: [0u8; 8],
            CurrentTaskFile: [0u8; 8],
        };
        // CurrentTaskFile is the IDE register block; index 6 is the Command
        // register. IDENTIFY DEVICE (0xEC) reads only — no LBA/count needed.
        apt.CurrentTaskFile[6] = ATA_IDENTIFY_DEVICE;

        // SAFETY: buf is HDR + payload; the header write stays within bounds.
        unsafe {
            ptr::write_unaligned(buf.as_mut_ptr() as *mut ATA_PASS_THROUGH_EX, apt);
        }

        let mut returned: u32 = 0;
        // SAFETY: single in/out buffer valid for the call; size matches buf.len().
        let ok = unsafe {
            DeviceIoControl(
                dev,
                IOCTL_ATA_PASS_THROUGH,
                buf.as_ptr() as *const core::ffi::c_void,
                buf.len() as u32,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                buf.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "ATA IDENTIFY pass-through failed: error {}",
                unsafe { GetLastError() }
            ));
        }
        if (returned as usize) < HDR + 258 {
            return Err("ATA IDENTIFY data truncated".to_string());
        }
        Ok(buf[HDR..HDR + ATA_IDENTIFY_LEN].to_vec())
    }

    fn probe_one(index: u32) -> DriveCapabilities {
        let mut caps = DriveCapabilities {
            drive_index: index,
            bus_type: "Unknown".to_string(),
            ..Default::default()
        };

        let dev = match open_drive(index) {
            Ok(d) => d,
            Err(err) => {
                caps.probe_error = Some(format!("open PhysicalDrive{index} failed: error {err}"));
                return caps;
            }
        };

        let bus = match query_bus_type(dev.0) {
            Ok(b) => b,
            Err(e) => {
                caps.probe_error = Some(e);
                return caps;
            }
        };
        caps.bus_type = bus_type_name(bus).to_string();
        caps.is_usb_bridge = bus == BusTypeUsb;

        if bus == BusTypeNvme {
            match query_nvme_identify(dev.0) {
                Ok(id) => {
                    let (sanitize, crypto, format) = parse_nvme_identify(&id);
                    caps.nvme_sanitize_supported = sanitize;
                    caps.nvme_crypto_erase_supported = crypto;
                    caps.nvme_format_supported = format;
                }
                Err(e) => caps.probe_error = Some(e),
            }
        } else if bus == BusTypeSata || bus == BusTypeAta {
            match query_ata_identify(dev.0) {
                Ok(id) => {
                    let (security, enhanced, frozen, sanitize) = parse_ata_identify(&id);
                    caps.ata_security_supported = security;
                    caps.ata_enhanced_erase_supported = enhanced;
                    caps.ata_frozen = frozen;
                    caps.ata_sanitize_supported = sanitize;
                }
                Err(e) => caps.probe_error = Some(e),
            }
        }
        // USB / RAID / SAS / SCSI / Unknown: bus type reported; no ATA/NVMe
        // admin path is trustworthy through those bridges, so capability flags
        // stay false (fail-safe: never claim an erase path we can't verify).

        caps
    }

    pub fn probe(drive_index: Option<u32>) -> Vec<DriveCapabilities> {
        if let Some(idx) = drive_index {
            return vec![probe_one(idx)];
        }
        // Enumerate fixed drives. A drive that fails to open (index gap) is
        // skipped silently; a drive that opens but errors mid-probe is kept with
        // its per-drive probe_error set.
        let mut drives = Vec::new();
        for idx in 0u32..32 {
            match open_drive(idx) {
                Ok(_) => drives.push(probe_one(idx)),
                Err(_) => continue,
            }
        }
        drives
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a zeroed 4096-byte NVMe Identify Controller buffer, then let the
    /// caller set specific bytes.
    fn nvme_buf() -> Vec<u8> {
        vec![0u8; 4096]
    }
    fn ata_buf() -> Vec<u8> {
        vec![0u8; 512]
    }

    #[test]
    fn nvme_all_flags_off_when_zeroed() {
        assert_eq!(parse_nvme_identify(&nvme_buf()), (false, false, false));
    }

    #[test]
    fn nvme_format_supported_is_oacs_bit1() {
        let mut b = nvme_buf();
        b[256] = 0b0000_0010; // OACS bit1
        let (sanitize, crypto, format) = parse_nvme_identify(&b);
        assert!(format);
        assert!(!sanitize);
        assert!(!crypto);
    }

    #[test]
    fn nvme_oacs_bit0_security_does_not_set_format() {
        let mut b = nvme_buf();
        b[256] = 0b0000_0001; // OACS bit0 = Security cmds, NOT Format
        let (_, _, format) = parse_nvme_identify(&b);
        assert!(!format);
    }

    #[test]
    fn nvme_crypto_erase_is_sanicap_bit0_and_implies_sanitize() {
        let mut b = nvme_buf();
        b[328] = 0b0000_0001; // SANICAP bit0 = Crypto Erase
        let (sanitize, crypto, _) = parse_nvme_identify(&b);
        assert!(crypto);
        assert!(sanitize);
    }

    #[test]
    fn nvme_block_erase_sets_sanitize_but_not_crypto() {
        let mut b = nvme_buf();
        b[328] = 0b0000_0010; // SANICAP bit1 = Block Erase
        let (sanitize, crypto, _) = parse_nvme_identify(&b);
        assert!(sanitize);
        assert!(!crypto);
    }

    #[test]
    fn nvme_overwrite_sets_sanitize_but_not_crypto() {
        let mut b = nvme_buf();
        b[328] = 0b0000_0100; // SANICAP bit2 = Overwrite
        let (sanitize, crypto, _) = parse_nvme_identify(&b);
        assert!(sanitize);
        assert!(!crypto);
    }

    #[test]
    fn nvme_short_buffer_reports_nothing() {
        assert_eq!(parse_nvme_identify(&[0u8; 100]), (false, false, false));
    }

    #[test]
    fn ata_all_flags_off_when_zeroed() {
        assert_eq!(parse_ata_identify(&ata_buf()), (false, false, false, false));
    }

    /// Helper: set word `w` (little-endian) in an ATA buffer.
    fn set_word(buf: &mut [u8], w: usize, val: u16) {
        buf[w * 2..w * 2 + 2].copy_from_slice(&val.to_le_bytes());
    }

    #[test]
    fn ata_security_supported_is_word82_bit1() {
        let mut b = ata_buf();
        set_word(&mut b, 82, 1 << 1);
        let (security, enhanced, frozen, sanitize) = parse_ata_identify(&b);
        assert!(security);
        assert!(!enhanced);
        assert!(!frozen);
        assert!(!sanitize);
    }

    #[test]
    fn ata_frozen_is_word128_bit3() {
        let mut b = ata_buf();
        set_word(&mut b, 128, 1 << 3);
        let (_, _, frozen, _) = parse_ata_identify(&b);
        assert!(frozen);
    }

    #[test]
    fn ata_enhanced_erase_is_word128_bit5() {
        let mut b = ata_buf();
        set_word(&mut b, 128, 1 << 5);
        let (_, enhanced, frozen, _) = parse_ata_identify(&b);
        assert!(enhanced);
        assert!(!frozen);
    }

    #[test]
    fn ata_frozen_and_enhanced_are_independent_bits() {
        let mut b = ata_buf();
        set_word(&mut b, 128, (1 << 3) | (1 << 5));
        let (_, enhanced, frozen, _) = parse_ata_identify(&b);
        assert!(enhanced);
        assert!(frozen);
    }

    #[test]
    fn ata_sanitize_supported_is_word59_bit12() {
        let mut b = ata_buf();
        set_word(&mut b, 59, 1 << 12);
        let (_, _, _, sanitize) = parse_ata_identify(&b);
        assert!(sanitize);
    }

    #[test]
    fn ata_word59_low_bits_do_not_set_sanitize() {
        let mut b = ata_buf();
        set_word(&mut b, 59, 0x0FFF); // bits 0..11 set, bit12 clear
        let (_, _, _, sanitize) = parse_ata_identify(&b);
        assert!(!sanitize);
    }

    #[test]
    fn ata_short_buffer_reports_nothing() {
        assert_eq!(
            parse_ata_identify(&[0u8; 100]),
            (false, false, false, false)
        );
    }
}
