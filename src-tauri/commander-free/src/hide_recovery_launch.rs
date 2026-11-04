// SPDX-License-Identifier: AGPL-3.0-or-later
//! Start the recovery listener with the interactive shell's standard token.
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
    System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
        InitializeProcThreadAttributeList, OpenProcess, OpenProcessToken,
        UpdateProcThreadAttribute, WaitForSingleObject, EXTENDED_STARTUPINFO_PRESENT,
        PROCESS_CREATE_PROCESS, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, STARTUPINFOEXW,
    },
    UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn user_token(process: HANDLE) -> Result<Handle, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err("Could not inspect the interactive shell identity".into());
    }
    Ok(Handle(token))
}

fn token_user(token: HANDLE) -> Result<Vec<usize>, String> {
    let mut bytes = 0;
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut bytes) };
    if bytes == 0 {
        return Err("Could not inspect the interactive user".into());
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err("Could not read the interactive user".into());
    }
    Ok(storage)
}

fn interactive_shell() -> Result<Handle, String> {
    let shell_window = unsafe { GetShellWindow() };
    if shell_window.is_null() {
        return Err("The interactive Windows shell is unavailable".into());
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(shell_window, &mut pid) };
    let shell = Handle(unsafe {
        OpenProcess(
            PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    });
    if shell.0.is_null() {
        return Err("Could not access the interactive Windows shell".into());
    }
    let shell_token = user_token(shell.0)?;
    let current_token = user_token(unsafe { GetCurrentProcess() })?;
    let shell_user = token_user(shell_token.0)?;
    let current_user = token_user(current_token.0)?;
    let shell_sid = unsafe { (*(shell_user.as_ptr() as *const TOKEN_USER)).User.Sid };
    let current_sid = unsafe { (*(current_user.as_ptr() as *const TOKEN_USER)).User.Sid };
    if unsafe { EqualSid(shell_sid, current_sid) } == 0 {
        return Err("The interactive Windows account differs from the elevated account".into());
    }
    Ok(shell)
}

pub(crate) fn verify_interactive_user() -> Result<(), String> {
    interactive_shell().map(|_| ())
}

pub(crate) fn launch(exe: &Path, shortcut: &str) -> Result<(), String> {
    let shell = interactive_shell()?;
    let mut bytes = 0;
    unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes) };
    if bytes == 0 {
        return Err("Could not prepare the recovery listener".into());
    }
    let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let list = storage.as_mut_ptr().cast();
    if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) } == 0 {
        return Err("Could not initialize the recovery listener".into());
    }
    let result = (|| {
        if unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                (&shell.0 as *const HANDLE).cast(),
                std::mem::size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err("Could not select the interactive shell token".into());
        }
        let path: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        let command: Vec<u16> = OsStr::new(&format!(
            "\"{}\" --hide-recovery-update \"{}\"",
            exe.display(),
            shortcut
        ))
        .encode_wide()
        .chain(Some(0))
        .collect();
        let mut command = command;
        let mut info = STARTUPINFOEXW::default();
        info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        info.lpAttributeList = list;
        let mut process = PROCESS_INFORMATION::default();
        if unsafe {
            CreateProcessW(
                path.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                ptr::null(),
                &info.StartupInfo,
                &mut process,
            )
        } == 0
        {
            return Err("Could not start the standard-token recovery listener".into());
        }
        unsafe {
            CloseHandle(process.hThread);
        }
        let status = unsafe { WaitForSingleObject(process.hProcess, 10_000) };
        let mut code = 1;
        let success = status == 0
            && unsafe { GetExitCodeProcess(process.hProcess, &mut code) } != 0
            && code == 0;
        unsafe {
            CloseHandle(process.hProcess);
        }
        if success {
            Ok(())
        } else {
            Err("The standard-token recovery setup did not complete".into())
        }
    })();
    unsafe { DeleteProcThreadAttributeList(list) };
    result
}
