// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::engine::{Observation, UNVERIFIED};
use base64::Engine;
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_OUTPUT: usize = 16_384;
fn powershell() -> Result<std::path::PathBuf, String> {
    let mut buffer = [0u16; 1024];
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    };
    if count == 0 || count as usize >= buffer.len() {
        return Err(UNVERIFIED.into());
    }
    Ok(
        std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..count as usize]))
            .join("WindowsPowerShell\\v1.0\\powershell.exe"),
    )
}

pub(super) fn execute(script: &str) -> Result<Observation, String> {
    let path = powershell()?;
    let modules = path.parent().ok_or(UNVERIFIED)?.join("Modules");
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let mut child = Command::new(path)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded,
        ])
        .env("PSModulePath", modules)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| UNVERIFIED.to_string())?;
    let output = child.stdout.take().ok_or(UNVERIFIED)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = Instant::now();
    let exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break Some(exit),
            Ok(None) if started.elapsed() < Duration::from_secs(8) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| UNVERIFIED.to_string())?
        .map_err(|_| UNVERIFIED.to_string())?;
    if bytes.len() > MAX_OUTPUT {
        return Err(UNVERIFIED.into());
    }
    let text = String::from_utf8(bytes).map_err(|_| UNVERIFIED.to_string())?;
    if !exit.is_some_and(|exit| exit.success()) {
        let message = match text.trim() {
            "BT_POLICY_UNSUPPORTED" => super::super::engine::UNSUPPORTED,
            "BT_POLICY_MANAGED" => super::super::engine::MANAGED,
            "BT_ACCESS_DENIED" => "[BT_ACCESS_DENIED] The Windows policy provider requires the authenticated SYSTEM service.",
            "BT_POLICY_REJECTED" => "[BT_POLICY_REJECTED] Windows did not confirm the Bluetooth policy request. Its outcome may be incomplete; refresh before retrying.",
            _ => UNVERIFIED,
        };
        return Err(message.into());
    }
    serde_json::from_str(text.trim().trim_start_matches('\u{feff}')).map_err(|_| UNVERIFIED.into())
}
