// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded maintenance checks for the opt-in migration lab. They detect known
//! running writers but do not prevent a process from starting after a scan.
#![allow(dead_code)]
fn known_writer(name: &str) -> bool {
    ["wincommander-free.exe", "wincommander-pro.exe", "wincommander-svc.exe", "wincommander.exe"]
        .iter().any(|expected| name.eq_ignore_ascii_case(expected))
}
fn allowed_service(name: &str, pid: u32, expected: Option<u32>) -> bool {
    pid != 0 && Some(pid) == expected && name.eq_ignore_ascii_case("wincommander-svc.exe")
}

/// Check the named service and all sessions for known binaries. Caller must
/// also disable their launch routes throughout maintenance. This is not a
/// defense against a renamed binary or a new launch racing after a scan.
#[cfg(windows)]
pub(crate) fn verify_known_writers_stopped() -> Result<(), String> {
    verify_known_writers_except_service(None)
}

/// Caller must authenticate and retain the matching service process separately.
#[cfg(windows)]
pub(crate) fn verify_known_writers_except_service(allowed_service_pid: Option<u32>) -> Result<(), String> {
    use std::{ffi::c_void, mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_NO_MORE_FILES, ERROR_SERVICE_DOES_NOT_EXIST, INVALID_HANDLE_VALUE},
        System::{Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
            Services::{CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_MANAGER_CONNECT,
                SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_STATUS_PROCESS, SERVICE_RUNNING, SERVICE_STOPPED},
            Threading::GetCurrentProcessId},
    };
    struct Service(*mut c_void);
    impl Drop for Service { fn drop(&mut self) { if !self.0.is_null() { unsafe { CloseServiceHandle(self.0); } } } }
    let scm = Service(unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) });
    if scm.0.is_null() { return Err("Cannot inspect WinCommander service; existing search data was preserved.".into()); }
    let name: Vec<u16> = "WinCommanderSvc\0".encode_utf16().collect();
    let service = Service(unsafe { OpenServiceW(scm.0, name.as_ptr(), SERVICE_QUERY_STATUS) });
    if service.0.is_null() {
        if std::io::Error::last_os_error().raw_os_error() != Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            return Err("Cannot inspect WinCommander service; existing search data was preserved.".into());
        }
    } else {
        let mut state = SERVICE_STATUS_PROCESS::default(); let mut needed = 0;
        if unsafe { QueryServiceStatusEx(service.0, SC_STATUS_PROCESS_INFO, (&mut state as *mut SERVICE_STATUS_PROCESS).cast(), size_of::<SERVICE_STATUS_PROCESS>() as u32, &mut needed) } == 0 {
            return Err("Cannot inspect WinCommander service; existing search data was preserved.".into());
        }
        if state.dwCurrentState != SERVICE_STOPPED && !(state.dwCurrentState == SERVICE_RUNNING && Some(state.dwProcessId) == allowed_service_pid && state.dwProcessId != 0) { return Err("Stop the WinCommander service before moving the search cache; existing search data was preserved.".into()); }
    }
    struct Snapshot(*mut c_void);
    impl Drop for Snapshot { fn drop(&mut self) { if self.0 != INVALID_HANDLE_VALUE { unsafe { CloseHandle(self.0); } } } }
    let snapshot = Snapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
    if snapshot.0 == INVALID_HANDLE_VALUE { return Err("Cannot inspect running applications; existing search data was preserved.".into()); }
    let mut process = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut ok = unsafe { Process32FirstW(snapshot.0, &mut process) };
    if ok == 0 { return Err("Cannot inspect running applications; existing search data was preserved.".into()); }
    let own_pid = unsafe { GetCurrentProcessId() };
    loop {
        let len = process.szExeFile.iter().position(|c| *c == 0).unwrap_or(process.szExeFile.len());
        let image = String::from_utf16(&process.szExeFile[..len]).map_err(|_| "Cannot inspect a running application; existing search data was preserved.")?;
        let allowed_service = allowed_service(&image, process.th32ProcessID, allowed_service_pid);
        if process.th32ProcessID != own_pid && known_writer(&image) && !allowed_service {
            return Err("Close all WinCommander windows and services before moving the search cache; existing search data was preserved.".into());
        }
        ok = unsafe { Process32NextW(snapshot.0, &mut process) };
        if ok == 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err("Cannot inspect running applications; existing search data was preserved.".into());
            }
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_only_known_writer_image_names_case_insensitively() {
        for name in ["WinCommander-Free.EXE", "wincommander-pro.exe", "wincommander-svc.exe", "WinCommander.exe"] { assert!(known_writer(name)); }
        for name in ["other.exe", "wincommander-free.exe.bak", "unrelated-wincommander.exe", ""] { assert!(!known_writer(name)); }
    }
    #[test]
    fn allows_only_the_authenticated_service_pid() {
        assert!(allowed_service("WinCommander-Svc.exe", 42, Some(42)));
        assert!(!allowed_service("WinCommander-Pro.exe", 42, Some(42)));
        assert!(!allowed_service("WinCommander-Svc.exe", 43, Some(42)));
        assert!(!allowed_service("WinCommander-Svc.exe", 0, Some(0)));
        assert!(!allowed_service("WinCommander-Svc.exe", 42, None));
    }
}
