// Windows registry helpers — Run/RunOnce + Uninstall enumeration and
// HKCU-scoped mutation. All writes are scoped to the current user; HKLM
// scope is read-only here. HKLM mutations land in the Pro binary in a
// future phase along with elevation.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunValue {
    pub hive: String,   // "HKCU" | "HKLM"
    pub subkey: String, // "Software\\Microsoft\\Windows\\CurrentVersion\\Run" etc.
    pub name: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UninstallEntry {
    pub hive: String,
    pub subkey: String, // the per-app key under Uninstall\
    pub display_name: Option<String>,
    pub display_icon: Option<String>,
    pub install_location: Option<String>,
    pub uninstall_string: Option<String>,
    pub publisher: Option<String>,
    pub system_component: Option<u32>, // current DWORD, if present
}

// ─── Wide-string helpers ─────────────────────────────────────────────────

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn read_wide_lossy(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ─── Resilient enumeration cursor ────────────────────────────────────────

/// One step of a `RegEnumKeyExW` / `RegEnumValueW` call, already classified.
pub(crate) enum EnumStep<T> {
    Item(T),
    /// `ERROR_MORE_DATA`; payload is the size the API reported (0 = unknown).
    MoreData(usize),
    /// `ERROR_NO_MORE_ITEMS` - the only condition that ends enumeration.
    NoMoreItems,
    /// Any other Win32 error: this index is skipped, enumeration continues.
    Error(u32),
}

const ENUM_MAX_INDEX: u32 = 1_000_000;
const ENUM_MAX_RETRIES: u32 = 4;
const ENUM_MAX_CAP: usize = 1 << 20;
/// A handle that fails permanently (e.g. closed) would otherwise spin through
/// every index; give up after this many consecutive failures.
const ENUM_MAX_CONSECUTIVE_ERRORS: u32 = 64;

/// Drives an enumeration so one bad entry cannot hide the rest: retries an
/// index with the size the API reported on `ERROR_MORE_DATA`, skips (and logs)
/// an index that fails otherwise, and stops only on `NoMoreItems`.
#[derive(Default)]
pub(crate) struct EnumCursor {
    index: u32,
    consecutive_errors: u32,
}

impl EnumCursor {
    /// `step(index, capacity_hint)`; `capacity_hint` is 0 on the first attempt
    /// and the size to use on a retry after `MoreData`.
    pub(crate) fn next<T>(
        &mut self,
        mut step: impl FnMut(u32, usize) -> EnumStep<T>,
    ) -> Option<T> {
        loop {
            if self.index >= ENUM_MAX_INDEX
                || self.consecutive_errors >= ENUM_MAX_CONSECUTIVE_ERRORS
            {
                return None;
            }
            let idx = self.index;
            let mut cap = 0usize;
            let mut tries = 0u32;
            let outcome = loop {
                match step(idx, cap) {
                    EnumStep::Item(v) => break Ok(v),
                    EnumStep::NoMoreItems => return None,
                    EnumStep::Error(rc) => break Err(rc),
                    EnumStep::MoreData(need) => {
                        tries += 1;
                        if tries > ENUM_MAX_RETRIES || need > ENUM_MAX_CAP {
                            break Err(234); // ERROR_MORE_DATA
                        }
                        cap = if need > cap {
                            need
                        } else {
                            cap.saturating_mul(2).max(1)
                        };
                    }
                }
            };
            self.index += 1;
            match outcome {
                Ok(v) => {
                    self.consecutive_errors = 0;
                    return Some(v);
                }
                Err(rc) => {
                    self.consecutive_errors += 1;
                    crate::log::log_message(
                        "warn",
                        &format!("[Registry] enumeration skipped index {idx}: error {rc}"),
                    );
                }
            }
        }
    }
}

/// `RegEnumKeyExW` for one index. Key names are at most 255 characters, so a
/// 256-wchar buffer is always sufficient.
#[cfg(windows)]
pub(crate) fn enum_key_name(
    key: windows_sys::Win32::System::Registry::HKEY,
    index: u32,
) -> EnumStep<String> {
    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::RegEnumKeyExW;
    let mut name = [0u16; 256];
    let mut len = name.len() as u32;
    let rc = unsafe {
        RegEnumKeyExW(
            key,
            index,
            name.as_mut_ptr(),
            &mut len,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    match rc {
        ERROR_SUCCESS => EnumStep::Item(String::from_utf16_lossy(&name[..(len as usize).min(256)])),
        ERROR_NO_MORE_ITEMS => EnumStep::NoMoreItems,
        other => EnumStep::Error(other),
    }
}

/// `RegEnumValueW` for one index. The name buffer is the documented maximum
/// (16,383 chars + NUL); the data buffer starts at 4 KiB and grows to the size
/// the API reports on `ERROR_MORE_DATA`. Returns `(name, type, data)`.
#[cfg(windows)]
pub(crate) fn enum_value(
    key: windows_sys::Win32::System::Registry::HKEY,
    index: u32,
    cap: usize,
) -> EnumStep<(String, u32, Vec<u8>)> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::RegEnumValueW;
    let mut name = vec![0u16; 16_384];
    let mut name_len = name.len() as u32;
    let mut data = vec![0u8; if cap == 0 { 4096 } else { cap }];
    let mut data_len = data.len() as u32;
    let mut ty = 0u32;
    let rc = unsafe {
        RegEnumValueW(
            key,
            index,
            name.as_mut_ptr(),
            &mut name_len,
            std::ptr::null_mut(),
            &mut ty,
            data.as_mut_ptr(),
            &mut data_len,
        )
    };
    match rc {
        ERROR_SUCCESS => {
            data.truncate((data_len as usize).min(data.len()));
            let n = (name_len as usize).min(name.len());
            EnumStep::Item((String::from_utf16_lossy(&name[..n]), ty, data))
        }
        ERROR_NO_MORE_ITEMS => EnumStep::NoMoreItems,
        ERROR_MORE_DATA => EnumStep::MoreData(data_len as usize),
        other => EnumStep::Error(other),
    }
}

// ─── Run-key reads ───────────────────────────────────────────────────────

#[cfg(windows)]
pub fn read_run_values() -> Vec<RunValue> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
        REG_EXPAND_SZ, REG_SZ,
    };

    fn read(hive_name: &str, root: HKEY, subkey: &str, out: &mut Vec<RunValue>) {
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            let wide = to_wide(subkey);
            if RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
                return;
            }
            let mut cursor = EnumCursor::default();
            while let Some((name, value_type, data)) =
                cursor.next(|i, cap| enum_value(hkey, i, cap))
            {
                if value_type == REG_SZ || value_type == REG_EXPAND_SZ {
                    let (pairs, _) = data.as_chunks::<2>();
                    let units: Vec<u16> = pairs.iter().map(|b| u16::from_le_bytes(*b)).collect();
                    out.push(RunValue {
                        hive: hive_name.into(),
                        subkey: subkey.into(),
                        name,
                        command: read_wide_lossy(&units),
                    });
                }
            }
            RegCloseKey(hkey);
        }
    }

    let mut out = Vec::new();
    for subkey in [
        "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
    ] {
        read("HKCU", HKEY_CURRENT_USER, subkey, &mut out);
        read("HKLM", HKEY_LOCAL_MACHINE, subkey, &mut out);
    }
    out
}

#[cfg(not(windows))]
pub fn read_run_values() -> Vec<RunValue> {
    Vec::new()
}

// ─── HKCU Run-key mutation: rename value (so it stops auto-starting) ────

/// Atomically rename a Run value in HKCU. Returns the new name on success.
/// If `target_name` already exists it is overwritten — the manifest is the
/// source of truth for which app the renamed value belongs to.
#[cfg(windows)]
pub fn hkcu_rename_run_value(subkey: &str, from_name: &str, to_name: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
    };

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let wide_sub = to_wide(subkey);
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide_sub.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW({}) failed: {}", subkey, rc));
        }

        let wide_from = to_wide(from_name);
        let mut value_type: u32 = 0;
        let mut value_buf: [u8; 4096] = [0; 4096];
        let mut value_len: u32 = value_buf.len() as u32;

        let rc = RegQueryValueExW(
            hkey,
            wide_from.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            value_buf.as_mut_ptr(),
            &mut value_len,
        );
        if rc != ERROR_SUCCESS {
            RegCloseKey(hkey);
            return Err(format!("RegQueryValueExW({}) failed: {}", from_name, rc));
        }

        let wide_to = to_wide(to_name);
        let rc = RegSetValueExW(
            hkey,
            wide_to.as_ptr(),
            0,
            value_type,
            value_buf.as_ptr(),
            value_len,
        );
        if rc != ERROR_SUCCESS {
            RegCloseKey(hkey);
            return Err(format!("RegSetValueExW({}) failed: {}", to_name, rc));
        }

        let rc = RegDeleteValueW(hkey, wide_from.as_ptr());
        // Roll back the SetValue if the delete fails.
        if rc != ERROR_SUCCESS {
            let _ = RegDeleteValueW(hkey, wide_to.as_ptr());
            RegCloseKey(hkey);
            return Err(format!("RegDeleteValueW({}) failed: {}", from_name, rc));
        }

        RegCloseKey(hkey);
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn hkcu_rename_run_value(
    _subkey: &str,
    _from_name: &str,
    _to_name: &str,
) -> Result<(), String> {
    Err("not implemented on non-Windows".into())
}

// ─── Uninstall enumeration + SystemComponent toggle ─────────────────────

#[cfg(windows)]
pub fn read_uninstall_entries() -> Vec<UninstallEntry> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, REG_EXPAND_SZ, REG_SZ,
    };

    fn read_str(hkey: windows_sys::Win32::System::Registry::HKEY, name: &str) -> Option<String> {
        unsafe {
            let wide = to_wide(name);
            let mut ty: u32 = 0;
            let mut buf: [u8; 2048] = [0; 2048];
            let mut len: u32 = buf.len() as u32;
            let rc = RegQueryValueExW(
                hkey,
                wide.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr(),
                &mut len,
            );
            if rc != ERROR_SUCCESS {
                return None;
            }
            if ty != REG_SZ && ty != REG_EXPAND_SZ {
                return None;
            }
            let chars = (len as usize) / 2;
            let slice = std::slice::from_raw_parts(buf.as_ptr() as *const u16, chars);
            Some(read_wide_lossy(slice))
        }
    }

    fn read_dword(hkey: windows_sys::Win32::System::Registry::HKEY, name: &str) -> Option<u32> {
        unsafe {
            let wide = to_wide(name);
            let mut ty: u32 = 0;
            let mut v: u32 = 0;
            let mut len: u32 = std::mem::size_of::<u32>() as u32;
            let rc = RegQueryValueExW(
                hkey,
                wide.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                &mut v as *mut u32 as *mut u8,
                &mut len,
            );
            if rc == ERROR_SUCCESS && ty == REG_DWORD {
                Some(v)
            } else {
                None
            }
        }
    }

    fn enumerate(hive_name: &str, root: HKEY, base_subkey: &str, out: &mut Vec<UninstallEntry>) {
        unsafe {
            let mut base: HKEY = std::ptr::null_mut();
            let wide = to_wide(base_subkey);
            if RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_READ, &mut base) != ERROR_SUCCESS {
                return;
            }
            let mut cursor = EnumCursor::default();
            while let Some(child_name) = cursor.next(|i, _| enum_key_name(base, i)) {
                let full_subkey = format!("{}\\{}", base_subkey, child_name);
                let full_wide = to_wide(&full_subkey);
                let mut child: HKEY = std::ptr::null_mut();
                if RegOpenKeyExW(root, full_wide.as_ptr(), 0, KEY_READ, &mut child) != ERROR_SUCCESS
                {
                    continue;
                }

                let entry = UninstallEntry {
                    hive: hive_name.into(),
                    subkey: full_subkey,
                    display_name: read_str(child, "DisplayName"),
                    display_icon: read_str(child, "DisplayIcon"),
                    install_location: read_str(child, "InstallLocation"),
                    uninstall_string: read_str(child, "UninstallString"),
                    publisher: read_str(child, "Publisher"),
                    system_component: read_dword(child, "SystemComponent"),
                };
                RegCloseKey(child);

                if entry.display_name.is_some() || entry.uninstall_string.is_some() {
                    out.push(entry);
                }
            }
            RegCloseKey(base);
        }
    }

    let mut out = Vec::new();
    enumerate(
        "HKCU",
        HKEY_CURRENT_USER,
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        &mut out,
    );
    enumerate(
        "HKLM",
        HKEY_LOCAL_MACHINE,
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        &mut out,
    );
    enumerate(
        "HKLM",
        HKEY_LOCAL_MACHINE,
        "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        &mut out,
    );
    out
}

#[cfg(not(windows))]
pub fn read_uninstall_entries() -> Vec<UninstallEntry> {
    Vec::new()
}

/// Set or clear SystemComponent DWORD on an HKLM Uninstall entry. The app
/// operation requires an elevated administrator token.
#[cfg(windows)]
pub fn hklm_set_system_component(subkey: &str, value: Option<u32>) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_SET_VALUE, REG_DWORD,
    };

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let wide_sub = to_wide(subkey);
        let rc = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide_sub.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW(HKLM\\{}) failed: {}", subkey, rc));
        }

        let name_wide = to_wide("SystemComponent");
        let rc = match value {
            Some(v) => RegSetValueExW(
                hkey,
                name_wide.as_ptr(),
                0,
                REG_DWORD,
                &v as *const u32 as *const u8,
                std::mem::size_of::<u32>() as u32,
            ),
            None => {
                let rc = RegDeleteValueW(hkey, name_wide.as_ptr());
                if rc == ERROR_SUCCESS || rc == 2 {
                    0
                } else {
                    rc
                }
            }
        };

        RegCloseKey(hkey);
        if rc != ERROR_SUCCESS {
            Err(format!(
                "Set/Delete SystemComponent on HKLM\\{} failed: {}",
                subkey, rc
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
pub fn hklm_set_system_component(_subkey: &str, _value: Option<u32>) -> Result<(), String> {
    Err("not implemented on non-Windows".into())
}

/// Atomically rename a Run value in HKLM. Requires administrator privileges
/// (the caller must be elevated).
#[cfg(windows)]
pub fn hklm_rename_run_value(subkey: &str, from_name: &str, to_name: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE,
    };

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let wide_sub = to_wide(subkey);
        let rc = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide_sub.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW(HKLM\\{}) failed: {}", subkey, rc));
        }

        let wide_from = to_wide(from_name);
        let mut value_type: u32 = 0;
        let mut value_buf: [u8; 4096] = [0; 4096];
        let mut value_len: u32 = value_buf.len() as u32;

        let rc = RegQueryValueExW(
            hkey,
            wide_from.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            value_buf.as_mut_ptr(),
            &mut value_len,
        );
        if rc != ERROR_SUCCESS {
            RegCloseKey(hkey);
            return Err(format!("RegQueryValueExW({}) failed: {}", from_name, rc));
        }

        let wide_to = to_wide(to_name);
        let rc = RegSetValueExW(
            hkey,
            wide_to.as_ptr(),
            0,
            value_type,
            value_buf.as_ptr(),
            value_len,
        );
        if rc != ERROR_SUCCESS {
            RegCloseKey(hkey);
            return Err(format!("RegSetValueExW({}) failed: {}", to_name, rc));
        }

        let rc = RegDeleteValueW(hkey, wide_from.as_ptr());
        if rc != ERROR_SUCCESS {
            let _ = RegDeleteValueW(hkey, wide_to.as_ptr());
            RegCloseKey(hkey);
            return Err(format!("RegDeleteValueW({}) failed: {}", from_name, rc));
        }

        RegCloseKey(hkey);
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn hklm_rename_run_value(
    _subkey: &str,
    _from_name: &str,
    _to_name: &str,
) -> Result<(), String> {
    Err("not implemented on non-Windows".into())
}

/// Set or clear SystemComponent DWORD on an HKCU Uninstall entry. Pass
/// `None` to delete the value entirely (restoring entries that had no
/// SystemComponent value before we ever touched them).
#[cfg(windows)]
pub fn hkcu_set_system_component(subkey: &str, value: Option<u32>) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_DWORD,
    };

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let wide_sub = to_wide(subkey);
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide_sub.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW({}) failed: {}", subkey, rc));
        }

        let name_wide = to_wide("SystemComponent");
        let rc = match value {
            Some(v) => RegSetValueExW(
                hkey,
                name_wide.as_ptr(),
                0,
                REG_DWORD,
                &v as *const u32 as *const u8,
                std::mem::size_of::<u32>() as u32,
            ),
            None => {
                let rc = RegDeleteValueW(hkey, name_wide.as_ptr());
                // ERROR_FILE_NOT_FOUND is OK — that's the intent of None.
                if rc == ERROR_SUCCESS || rc == 2 {
                    0
                } else {
                    rc
                }
            }
        };

        RegCloseKey(hkey);
        if rc != ERROR_SUCCESS {
            Err(format!("Set/Delete SystemComponent failed: {}", rc))
        } else {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
pub fn hkcu_set_system_component(_subkey: &str, _value: Option<u32>) -> Result<(), String> {
    Err("not implemented on non-Windows".into())
}

// ─── App Paths (Windows Search "Run command" entries) ────────────────────────

const APP_PATHS_BASE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths";

/// Read the (Default) and Path values from `HKLM\...\App Paths\{exe_name}`.
/// Returns `None` if the key does not exist.
#[cfg(windows)]
pub fn hklm_read_app_path(exe_name: &str) -> Option<(Option<String>, Option<String>)> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SZ,
    };

    fn read_str_val(
        hkey: windows_sys::Win32::System::Registry::HKEY,
        name: &str,
    ) -> Option<String> {
        unsafe {
            let wide_name = to_wide(name);
            let mut ty: u32 = 0;
            let mut buf: [u8; 2048] = [0; 2048];
            let mut len: u32 = buf.len() as u32;
            let rc = RegQueryValueExW(
                hkey,
                wide_name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr(),
                &mut len,
            );
            if rc != ERROR_SUCCESS || (ty != REG_SZ && ty != REG_EXPAND_SZ) {
                return None;
            }
            let chars = (len as usize) / 2;
            let slice = std::slice::from_raw_parts(buf.as_ptr() as *const u16, chars);
            Some(read_wide_lossy(slice))
        }
    }

    unsafe {
        let full = format!("{}\\{}", APP_PATHS_BASE, exe_name);
        let wide = to_wide(&full);
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            return None;
        }
        let default_val = read_str_val(hkey, "");
        let path_val = read_str_val(hkey, "Path");
        RegCloseKey(hkey);
        Some((default_val, path_val))
    }
}

#[cfg(not(windows))]
pub fn hklm_read_app_path(_exe_name: &str) -> Option<(Option<String>, Option<String>)> {
    None
}

/// Delete the HKLM App Paths key for `exe_name`.
#[cfg(windows)]
pub fn hklm_delete_app_path(exe_name: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteKeyExW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS,
        KEY_WOW64_64KEY,
    };

    unsafe {
        let parent_wide = to_wide(APP_PATHS_BASE);
        let mut parent: HKEY = std::ptr::null_mut();
        let rc = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            parent_wide.as_ptr(),
            0,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            &mut parent,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW(App Paths) failed: {}", rc));
        }
        let child_wide = to_wide(exe_name);
        let rc = RegDeleteKeyExW(parent, child_wide.as_ptr(), KEY_WOW64_64KEY, 0);
        RegCloseKey(parent);
        // 2 = ERROR_FILE_NOT_FOUND — key was already absent, treat as success
        if rc == ERROR_SUCCESS || rc == 2 {
            Ok(())
        } else {
            Err(format!(
                "RegDeleteKeyExW(App Paths\\{}) failed: {}",
                exe_name, rc
            ))
        }
    }
}

#[cfg(not(windows))]
pub fn hklm_delete_app_path(_exe_name: &str) -> Result<(), String> {
    Err("not implemented on non-Windows".into())
}

/// Recreate the HKLM App Paths key for `exe_name` with the given values.
#[cfg(windows)]
pub fn hklm_create_app_path(
    exe_name: &str,
    default_val: Option<&str>,
    path_val: Option<&str>,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS,
        KEY_WOW64_64KEY, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    unsafe {
        let full = format!("{}\\{}", APP_PATHS_BASE, exe_name);
        let full_wide = to_wide(&full);
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            full_wide.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            std::ptr::null_mut(),
            &mut hkey,
            std::ptr::null_mut(),
        );
        if rc != ERROR_SUCCESS {
            return Err(format!(
                "RegCreateKeyExW(App Paths\\{}) failed: {}",
                exe_name, rc
            ));
        }

        let write_str = |name: &str, val: &str| {
            let name_wide = to_wide(name);
            let val_wide: Vec<u16> = val.encode_utf16().chain(std::iter::once(0)).collect();
            RegSetValueExW(
                hkey,
                name_wide.as_ptr(),
                0,
                REG_SZ,
                val_wide.as_ptr() as *const u8,
                (val_wide.len() * 2) as u32,
            )
        };

        if let Some(v) = default_val {
            write_str("", v);
        }
        if let Some(v) = path_val {
            write_str("Path", v);
        }

        RegCloseKey(hkey);
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn hklm_create_app_path(
    _exe_name: &str,
    _default_val: Option<&str>,
    _path_val: Option<&str>,
) -> Result<(), String> {
    Err("not implemented on non-Windows".into())
}

#[cfg(test)]
mod enum_cursor_tests {
    use super::*;

    /// Simulated key: Some(Ok) = value, Err(MoreData(n)) = needs n bytes.
    fn run(entries: Vec<Result<&'static str, u32>>, big_need: usize) -> Vec<&'static str> {
        let mut cursor = EnumCursor::default();
        let mut out = Vec::new();
        while let Some(v) = cursor.next(|i, cap| match entries.get(i as usize) {
            None => EnumStep::NoMoreItems,
            Some(Ok(v)) => EnumStep::Item(*v),
            Some(Err(0)) => {
                if cap >= big_need {
                    EnumStep::Item("big")
                } else {
                    EnumStep::MoreData(big_need)
                }
            }
            Some(Err(rc)) => EnumStep::Error(*rc),
        }) {
            out.push(v);
        }
        out
    }

    #[test]
    fn oversized_value_is_retried_and_later_entries_survive() {
        let got = run(vec![Ok("a"), Err(0), Ok("c")], 10_000);
        assert_eq!(got, vec!["a", "big", "c"]);
    }

    #[test]
    fn other_error_skips_index_and_continues() {
        let got = run(vec![Ok("a"), Err(5), Ok("c")], 0);
        assert_eq!(got, vec!["a", "c"]);
    }

    #[test]
    fn unsatisfiable_more_data_is_skipped_not_fatal() {
        let got = run(vec![Err(0), Ok("b")], usize::MAX);
        assert_eq!(got, vec!["b"]);
    }

    #[test]
    fn only_no_more_items_ends_and_persistent_errors_are_bounded() {
        let mut cursor = EnumCursor::default();
        let mut calls = 0u32;
        let r: Option<()> = cursor.next(|_, _| {
            calls += 1;
            EnumStep::Error(6)
        });
        assert!(r.is_none());
        assert_eq!(calls, ENUM_MAX_CONSECUTIVE_ERRORS);
    }
}
