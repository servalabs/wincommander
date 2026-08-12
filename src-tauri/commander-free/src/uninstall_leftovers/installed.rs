use std::collections::HashSet;
use std::path::Path;

#[cfg(windows)]
pub(super) fn installed_tokens() -> HashSet<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };
    let mut tokens = HashSet::new();
    for (root, parent) in [
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
    ] {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(root, wide(parent).as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
                continue;
            }
            for index in 0..4096u32 {
                let mut name = [0u16; 512];
                let mut length = name.len() as u32;
                if RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != ERROR_SUCCESS
                {
                    break;
                }
                let subkey = format!(
                    "{parent}\\{}",
                    String::from_utf16_lossy(&name[..length as usize])
                );
                if let Some(display_name) = read_value(root, &subkey, "DisplayName") {
                    add_tokens(&mut tokens, &display_name);
                }
                if let Some(location) = read_value(root, &subkey, "InstallLocation") {
                    if let Some(folder) = Path::new(&location).file_name() {
                        add_tokens(&mut tokens, &folder.to_string_lossy());
                    }
                }
            }
            RegCloseKey(key);
        }
    }
    tokens
}
#[cfg(not(windows))]
pub(super) fn installed_tokens() -> HashSet<String> {
    HashSet::new()
}

#[cfg(windows)]
fn read_value(
    root: windows_sys::Win32::System::Registry::HKEY,
    key_name: &str,
    name: &str,
) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(key_name).as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }
        let mut ty = 0;
        let mut bytes = 0;
        let rc = RegQueryValueExW(
            key,
            wide(name).as_ptr(),
            std::ptr::null(),
            &mut ty,
            std::ptr::null_mut(),
            &mut bytes,
        );
        if rc != ERROR_SUCCESS
            || !(ty == REG_SZ || ty == REG_EXPAND_SZ)
            || bytes == 0
            || bytes > 65_536
        {
            RegCloseKey(key);
            return None;
        }
        let mut data = vec![0u8; bytes as usize];
        let rc = RegQueryValueExW(
            key,
            wide(name).as_ptr(),
            std::ptr::null(),
            &mut ty,
            data.as_mut_ptr(),
            &mut bytes,
        );
        RegCloseKey(key);
        (rc == ERROR_SUCCESS && bytes % 2 == 0).then(|| {
            String::from_utf16_lossy(std::slice::from_raw_parts(
                data.as_ptr() as *const u16,
                bytes as usize / 2,
            ))
            .trim_end_matches('\0')
            .to_string()
        })
    }
}
#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
fn add_tokens(tokens: &mut HashSet<String>, value: &str) {
    let value = value.trim().to_ascii_lowercase();
    if value.len() >= 4 {
        tokens.insert(value.clone());
        if let Some(first) = value
            .split(|c: char| !c.is_alphanumeric())
            .find(|part| part.len() >= 4)
        {
            tokens.insert(first.into());
        }
    }
}
