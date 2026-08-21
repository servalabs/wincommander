use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(super) fn run(args: &[&str]) -> Result<String, String> {
    let output = Command::new("netsh.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("netsh.exe unavailable: {e}"))?;
    result(output)
}

pub(super) fn run_owned(args: &[String]) -> Result<String, String> {
    let output = Command::new("netsh.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("netsh.exe unavailable: {e}"))?;
    result(output)
}

fn result(output: std::process::Output) -> Result<String, String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "netsh.exe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
