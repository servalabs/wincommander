#[cfg(windows)]
pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub(super) fn read_wide(value: &[u16]) -> String {
    String::from_utf16_lossy(value)
}

#[cfg(windows)]
pub(super) fn read_default(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }
        let mut ty = 0;
        let mut data = [0u8; 4096];
        let mut length = data.len() as u32;
        let rc = RegQueryValueExW(
            key,
            wide("").as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            data.as_mut_ptr(),
            &mut length,
        );
        RegCloseKey(key);
        if rc != ERROR_SUCCESS
            || !(ty == REG_SZ || ty == REG_EXPAND_SZ)
            || !length.is_multiple_of(2)
        {
            return None;
        }
        let units = std::slice::from_raw_parts(data.as_ptr() as *const u16, length as usize / 2);
        Some(
            String::from_utf16_lossy(units)
                .trim_end_matches('\0')
                .to_string(),
        )
    }
}
