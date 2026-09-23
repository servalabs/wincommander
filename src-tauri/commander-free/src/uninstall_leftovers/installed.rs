use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct InstalledAppEvidence {
    pub(super) names: HashSet<String>,
    pub(super) locations: Vec<PathBuf>,
}

#[derive(Default)]
pub(super) struct RunningAppEvidence {
    pub(super) names: HashSet<String>,
    pub(super) executable_paths: Vec<PathBuf>,
}

#[cfg(windows)]
pub(super) fn installed_app_evidence() -> InstalledAppEvidence {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    let mut evidence = InstalledAppEvidence::default();
    let roots = [
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
    ];
    for (root, parent) in roots {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            unsafe {
                let mut key: HKEY = std::ptr::null_mut();
                if RegOpenKeyExW(root, wide(parent).as_ptr(), 0, KEY_READ | view, &mut key)
                    != ERROR_SUCCESS
                {
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
                    if let Some(display_name) = read_value(root, &subkey, "DisplayName", view) {
                        add_name(&mut evidence.names, &display_name);
                    }
                    if let Some(location) = read_value(root, &subkey, "InstallLocation", view) {
                        if let Some(folder) = Path::new(&location).file_name() {
                            add_name(&mut evidence.names, &folder.to_string_lossy());
                        }
                        evidence
                            .locations
                            .push(PathBuf::from(expand_known_environment(&location)));
                    }
                }
                RegCloseKey(key);
            }
        }
    }
    evidence
}

#[cfg(not(windows))]
pub(super) fn installed_app_evidence() -> InstalledAppEvidence {
    InstalledAppEvidence::default()
}

#[cfg(windows)]
pub(super) fn running_app_evidence() -> RunningAppEvidence {
    let mut evidence = RunningAppEvidence::default();
    let system = sysinfo::System::new_all();
    for process in system.processes().values() {
        add_name(&mut evidence.names, &process.name().to_string_lossy());
        if let Some(path) = process.exe() {
            if let Some(stem) = path.file_stem() {
                add_name(&mut evidence.names, &stem.to_string_lossy());
            }
            evidence.executable_paths.push(path.to_path_buf());
        }
    }
    evidence
}

#[cfg(not(windows))]
pub(super) fn running_app_evidence() -> RunningAppEvidence {
    RunningAppEvidence::default()
}

#[cfg(windows)]
fn read_value(
    root: windows_sys::Win32::System::Registry::HKEY,
    key_name: &str,
    name: &str,
    view: u32,
) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(key_name).as_ptr(), 0, KEY_READ | view, &mut key)
            != ERROR_SUCCESS
        {
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

fn add_name(names: &mut HashSet<String>, value: &str) {
    let value = value.trim();
    let value = if value.to_ascii_lowercase().ends_with(".exe") {
        &value[..value.len() - 4]
    } else {
        value
    };
    let normalized = normalize_name(value);
    if normalized.chars().count() >= 4 {
        names.insert(normalized);
    }
}

pub(super) fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(windows)]
fn expand_known_environment(value: &str) -> String {
    let mut expanded = value.to_string();
    for name in [
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "SystemRoot",
        "SystemDrive",
        "USERPROFILE",
    ] {
        if let Ok(value) = std::env::var(name) {
            expanded = expanded.replace(&format!("%{name}%"), &value);
        }
    }
    expanded
}

#[cfg(test)]
mod tests {
    use super::{add_name, normalize_name};
    use std::collections::HashSet;

    #[test]
    fn normalizes_display_names_and_folder_names_without_partial_tokens() {
        let mut names = HashSet::new();
        add_name(&mut names, "Example Tools, Inc.");

        assert!(names.contains(&normalize_name("Example Tools, Inc.")));
        assert!(!names.contains("example"));
    }

    #[test]
    fn normalizes_case_and_punctuation_for_exact_folder_comparisons() {
        assert_eq!(normalize_name("Example-App_2.0"), "exampleapp20");
    }
}
