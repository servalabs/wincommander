// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{
    fs,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
        TOKEN_USER,
    },
    System::{
        SystemInformation::GetWindowsDirectoryW,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};
use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};

const UNAVAILABLE: &str = "user profile locations could not be verified";
const OTHER_PROFILE: &str = "another user profile is protected";

fn current_sid() -> Result<String, &'static str> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(UNAVAILABLE);
    }
    let mut bytes = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut bytes);
    }
    let mut data = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
    let ok = bytes != 0
        && unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                data.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        } != 0;
    unsafe {
        CloseHandle(token);
    }
    if !ok {
        return Err(UNAVAILABLE);
    }
    let sid = unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut value = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0 || value.is_null() {
        return Err(UNAVAILABLE);
    }
    let mut length = 0;
    unsafe {
        while *value.add(length) != 0 {
            length += 1;
        }
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(value, length) })
        .map_err(|_| UNAVAILABLE);
    unsafe {
        LocalFree(value.cast());
    }
    result
}

fn resolve_profile_string(value: &str, windows_dir: &Path) -> Result<PathBuf, &'static str> {
    let normalized = value.to_ascii_lowercase();
    let path =
        if normalized.starts_with("%systemroot%\\") || normalized.starts_with("%systemroot%/") {
            windows_dir.join(&value[13..])
        } else if value.contains('%') {
            return Err(UNAVAILABLE);
        } else {
            PathBuf::from(value)
        };
    if !path.is_absolute() || path.to_string_lossy().contains('%') {
        return Err(UNAVAILABLE);
    }
    Ok(path)
}

fn windows_directory() -> Result<PathBuf, &'static str> {
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(UNAVAILABLE);
    }
    let windows_dir = String::from_utf16(&buffer[..length]).map_err(|_| UNAVAILABLE)?;
    Ok(PathBuf::from(windows_dir))
}

fn expand_path(value: &str) -> Result<PathBuf, &'static str> {
    let path = resolve_profile_string(value, &windows_directory()?)?;
    match fs::canonicalize(&path) {
        Ok(root) => Ok(root),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Other users' profile roots may deny opening the directory while
            // still permitting their parent and attributes to be inspected.
            // Resolve the parent and reject reparse-point roots; never guess
            // about a missing, redirected, or uninspectable profile.
            let meta = fs::symlink_metadata(&path).map_err(|_| UNAVAILABLE)?;
            if !meta.is_dir() || super::safety::is_link_or_reparse(&meta) { return Err(UNAVAILABLE); }
            let parent = path.parent().ok_or(UNAVAILABLE)?;
            let name = path.file_name().ok_or(UNAVAILABLE)?;
            Ok(fs::canonicalize(parent).map_err(|_| UNAVAILABLE)?.join(name))
        }
        Err(_) => Err(UNAVAILABLE),
    }
}

// Built-in service profiles under Windows are already inside the protected
// system directory; a normal user cannot open systemprofile to canonicalize
// it. Never skip a redirected or unexpected service profile location.
fn protected_builtin_profile(sid: &str, value: &str, windows: &Path) -> bool {
    let suffix = match sid {
        "S-1-5-18" => r"System32\config\systemprofile",
        "S-1-5-19" => r"ServiceProfiles\LocalService",
        "S-1-5-20" => r"ServiceProfiles\NetworkService",
        _ => return false,
    };
    resolve_profile_string(value, windows).ok().is_some_and(|path| {
        path.to_string_lossy().replace('/', "\\").trim_end_matches('\\')
            .eq_ignore_ascii_case(&windows.join(suffix).to_string_lossy())
    })
}

fn profiles() -> Result<(String, Vec<(String, PathBuf)>), &'static str> {
    let sid = current_sid()?;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList")
        .map_err(|_| UNAVAILABLE)?;
    let mut paths = Vec::new();
    for name in key.enum_keys() {
        let name = name.map_err(|_| UNAVAILABLE)?;
        if !name.starts_with("S-1-") {
            continue;
        }
        let subkey = key.open_subkey(&name).map_err(|_| UNAVAILABLE)?;
        let path: String = subkey
            .get_value("ProfileImagePath")
            .map_err(|_| UNAVAILABLE)?;
        if !name.eq_ignore_ascii_case(&sid) && protected_builtin_profile(&name, &path, &windows_directory()?) { continue; }
        paths.push((name, expand_path(&path)?));
    }
    if !paths
        .iter()
        .any(|(profile_sid, _)| profile_sid.eq_ignore_ascii_case(&sid))
    {
        return Err(UNAVAILABLE);
    }
    Ok((sid, paths))
}

fn contains(path: &Path, root: &Path) -> bool {
    let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let root = root
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

fn classify(
    path: &Path,
    current_sid: &str,
    profiles: &[(String, PathBuf)],
) -> Result<(), &'static str> {
    if profiles.is_empty()
        || !profiles
            .iter()
            .any(|(sid, _)| sid.eq_ignore_ascii_case(current_sid))
    {
        return Err(UNAVAILABLE);
    }
    if profiles
        .iter()
        .any(|(sid, root)| !sid.eq_ignore_ascii_case(current_sid) && contains(path, root))
    {
        return Err(OTHER_PROFILE);
    }
    Ok(())
}

fn describe_profile<'a>(
    path: &Path,
    current_sid: &str,
    profiles: &'a [(String, PathBuf)],
) -> (&'static str, Option<&'a Path>) {
    if profiles.is_empty()
        || !profiles
            .iter()
            .any(|(sid, _)| sid.eq_ignore_ascii_case(current_sid))
    {
        return ("unavailable", None);
    }
    if let Some((_, root)) = profiles
        .iter()
        .find(|(sid, root)| !sid.eq_ignore_ascii_case(current_sid) && contains(path, root))
    {
        return ("other", Some(root.as_path()));
    }
    if let Some((_, root)) = profiles
        .iter()
        .find(|(sid, root)| sid.eq_ignore_ascii_case(current_sid) && contains(path, root))
    {
        return ("current", Some(root.as_path()));
    }
    ("outside", None)
}

pub(super) struct Snapshot(Result<(String, Vec<(String, PathBuf)>), &'static str>);

impl Snapshot {
    pub(super) fn load() -> Self {
        Self(profiles())
    }

    pub(super) fn check(&self, path: &Path) -> Result<(), &'static str> {
        let (sid, paths) = self.0.as_ref().map_err(|error| *error)?;
        classify(path, sid, paths)
    }

    pub(super) fn current_root(&self) -> Result<PathBuf, &'static str> {
        let (sid, paths) = self.0.as_ref().map_err(|error| *error)?;
        paths.iter().find(|(candidate, _)| candidate.eq_ignore_ascii_case(sid))
            .map(|(_, root)| root.clone()).ok_or(UNAVAILABLE)
    }

    pub(super) fn describe(&self, path: &Path) -> (&'static str, Option<&Path>) {
        match &self.0 {
            Ok((sid, paths)) => describe_profile(path, sid, paths),
            Err(_) => ("unavailable", None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_scan_root_comes_from_matching_profile_sid() {
        let snapshot = Snapshot(Ok(("S-1-mine".into(), vec![
            ("S-1-other".into(), PathBuf::from(r"D:\Other")),
            ("s-1-MINE".into(), PathBuf::from(r"D:\Mine")),
        ])));
        assert_eq!(snapshot.current_root().unwrap(), PathBuf::from(r"D:\Mine"));
        assert!(Snapshot(Ok(("missing".into(), Vec::new()))).current_root().is_err());
        assert!(Snapshot(Err(UNAVAILABLE)).current_root().is_err());
    }
    #[test]
    fn only_standard_service_profiles_under_windows_are_covered_by_system_protection() {
        let windows = Path::new(r"C:\Windows");
        assert!(protected_builtin_profile("S-1-5-18", r"%SystemRoot%\System32\config\systemprofile", windows));
        assert!(protected_builtin_profile("S-1-5-19", r"C:\Windows\ServiceProfiles\LocalService", windows));
        assert!(!protected_builtin_profile("S-1-5-18", r"D:\RedirectedService", windows));
        assert!(!protected_builtin_profile("S-1-5-21-123", r"C:\Windows\System32\config\systemprofile", windows));
    }
    #[test]
    fn redirected_other_profile_on_another_drive_is_protected() {
        let profiles = vec![
            ("mine".into(), PathBuf::from(r"C:\Users\Me")),
            ("other".into(), PathBuf::from(r"D:\Redirected\Alice")),
        ];
        assert_eq!(
            classify(
                Path::new(r"d:\redirected\ALICE\shared\report.pdf"),
                "mine",
                &profiles
            ),
            Err(OTHER_PROFILE)
        );
        assert_eq!(
            classify(
                Path::new(r"D:\Redirected\Alice-archive\report.pdf"),
                "mine",
                &profiles
            ),
            Ok(())
        );
        assert_eq!(
            classify(
                Path::new(r"C:\Users\Me\Documents\report.pdf"),
                "mine",
                &profiles
            ),
            Ok(())
        );
    }
    #[test]
    fn profile_classification_uses_registered_roots_not_folder_names() {
        let paths = vec![
            ("mine".into(), PathBuf::from(r"C:\Custom\Me")),
            ("other".into(), PathBuf::from(r"D:\Redirected\Alice")),
        ];
        assert_eq!(
            describe_profile(Path::new(r"D:\Redirected\Alice\report.pdf"), "mine", &paths),
            ("other", Some(Path::new(r"D:\Redirected\Alice")))
        );
        assert_eq!(
            describe_profile(Path::new(r"C:\Custom\Me\a.pdf"), "mine", &paths).0,
            "current"
        );
        assert_eq!(
            describe_profile(Path::new(r"D:\Users\NotAProfile\a.pdf"), "mine", &paths).0,
            "outside"
        );
        assert_eq!(
            describe_profile(Path::new(r"C:\Custom\Me\a.pdf"), "missing", &paths).0,
            "unavailable"
        );
    }
    #[test]
    fn unavailable_or_unmatched_profile_inventory_fails_closed() {
        assert_eq!(
            classify(Path::new(r"D:\data\report.pdf"), "mine", &[]),
            Err(UNAVAILABLE)
        );
        assert_eq!(
            classify(
                Path::new(r"D:\data\report.pdf"),
                "missing",
                &[("other".into(), PathBuf::from(r"D:\Users\Other"))]
            ),
            Err(UNAVAILABLE)
        );
    }
    #[test]
    fn per_user_and_unrecognized_environment_variables_fail_closed() {
        let windows = Path::new(r"C:\Windows");
        assert_eq!(
            resolve_profile_string(r"%USERPROFILE%\Documents", windows),
            Err(UNAVAILABLE)
        );
        assert_eq!(
            resolve_profile_string(r"D:\Users\%USERNAME%", windows),
            Err(UNAVAILABLE)
        );
        assert_eq!(
            resolve_profile_string(r"%SystemRoot%\System32", windows).unwrap(),
            windows.join("System32")
        );
    }
    #[test]
    fn live_registry_includes_the_current_account() {
        let (sid, paths) = profiles().expect("profile inventory must be readable for removal");
        assert!(paths
            .iter()
            .any(|(entry, _)| entry.eq_ignore_ascii_case(&sid)));
    }
}
