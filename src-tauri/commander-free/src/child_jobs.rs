// src-tauri/commander-free/src/child_jobs.rs
//
// ═══════════════════════════════════════════════════════════════════════
// Auto-kill orphan child processes when WinCommander dies
// ═══════════════════════════════════════════════════════════════════════
//
// Background — the Privacy Shield Python process is spawned by a
// PowerShell wrapper, NOT directly by WinCommander. PowerShell exits
// right after `[System.Diagnostics.Process]::Start(...)` returns, so
// Python's parent becomes `services.exe` (or the desktop shell), not
// WinCommander. That orphan survives forever if the user end-tasks
// WinCommander via Task Manager — the tray-quit cleanup never runs,
// and the user has to kill Python by hand.
//
// Fix — use a Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
//   1. At startup, we create a single Job for the entire WinCommander
//      lifetime. The Job handle lives in a static, so when the process
//      dies (normally, via Ctrl-Alt-Del, or via Task Manager end-task)
//      the OS closes every handle including this one. Closing the last
//      handle on a kill-on-close Job terminates every assigned process.
//   2. `assign_pid(pid)` opens the target process with PROCESS_TERMINATE
//      | PROCESS_SET_QUOTA and adds it to the Job.
//
// The wrapper script (privacy_shield.ps1) returns the spawned Python
// PID. We pass that PID through to `assign_pid` so the OS becomes
// responsible for the cleanup. No watchdog thread, no parent-poll
// loop, no race.

#[cfg(windows)]
use std::sync::OnceLock;

#[cfg(windows)]
static JOB: OnceLock<usize> = OnceLock::new();

/// Initialise the kill-on-close Job. Safe to call multiple times; only
/// the first call creates the handle. Returns true on first
/// successful creation.
#[cfg(windows)]
pub fn init() -> bool {
    use windows_sys::Win32::Foundation::FALSE;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    if JOB.get().is_some() {
        return false;
    }

    unsafe {
        let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if handle.is_null() {
            crate::log_message(
                "warn",
                "[ChildJobs] CreateJobObjectW returned NULL — orphan cleanup disabled.",
            );
            return false;
        }
        // Set KILL_ON_JOB_CLOSE so all assigned processes die when this
        // handle's refcount hits zero (including OS-forced WinCommander
        // termination).
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..std::mem::zeroed()
        };
        let ok = SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        );
        if ok == FALSE {
            crate::log_message(
                "warn",
                "[ChildJobs] SetInformationJobObject failed — orphan cleanup disabled.",
            );
            return false;
        }
        let _ = JOB.set(handle as usize);
        crate::log_message(
            "debug",
            "[ChildJobs] Job object initialised (kill-on-close).",
        );
        true
    }
}

/// Assign an existing process by PID to the global Job. Returns true on
/// success. Silent no-op on non-Windows and when the Job hasn't been
/// initialised.
#[cfg(windows)]
pub fn assign_pid(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Foundation::FALSE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    let Some(&handle_usize) = JOB.get() else {
        crate::log_message("warn", "[ChildJobs] assign_pid called before init()");
        return false;
    };
    let job = handle_usize as windows_sys::Win32::Foundation::HANDLE;
    unsafe {
        let target = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, FALSE as _, pid);
        if target.is_null() {
            crate::log_message(
                "warn",
                &format!(
                    "[ChildJobs] OpenProcess for PID {} failed (already exited?)",
                    pid
                ),
            );
            return false;
        }
        let ok = AssignProcessToJobObject(job, target);
        let _ = CloseHandle(target);
        if ok == FALSE {
            crate::log_message(
                "warn",
                &format!(
                    "[ChildJobs] AssignProcessToJobObject failed for PID {} \
                    (Win 7 / nested job? OS will not kill on exit)",
                    pid
                ),
            );
            return false;
        }
        crate::log_message(
            "debug",
            &format!(
                "[ChildJobs] PID {} jobbed — will die when WinCommander exits",
                pid
            ),
        );
        true
    }
}

/// Query-only liveness check for a PID. Opens the process with
/// PROCESS_QUERY_LIMITED_INFORMATION (NOT a terminate right) and reads its
/// exit code. Used by the Privacy Shield event reader to stop tailing the
/// sidecar once the shield process is gone.
#[cfg(windows)]
pub fn is_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE as _, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        let _ = CloseHandle(handle);
        ok != FALSE && code == STILL_ACTIVE
    }
}

// Non-Windows stubs so the rest of the codebase can call these
// unconditionally.
#[cfg(not(windows))]
pub fn init() -> bool {
    false
}
#[cfg(not(windows))]
pub fn assign_pid(_pid: u32) -> bool {
    false
}
#[cfg(not(windows))]
pub fn is_pid_alive(_pid: u32) -> bool {
    false
}
