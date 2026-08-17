use super::{CANCELLED, CachedUpdate, Manager, PackageUpdateResult};
use std::process::Command;
use std::sync::atomic::Ordering;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
// CREATE_NO_WINDOW: without it, every spawned console executable (winget,
// choco, Scoop, and npm) briefly flashes its own console window because this app
// is a GUI-subsystem process with no console of its own for children to
// attach to.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn command(executable: &str) -> Command {
    let mut cmd = Command::new(executable);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

pub(super) fn apply_updates(updates: Vec<CachedUpdate>) -> Result<PackageUpdateResult, String> {
    let mut result = PackageUpdateResult {
        updated: 0,
        cancelled: false,
        errors: Vec::new(),
    };
    for update in updates {
        if CANCELLED.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        let npm_prefix = machine_npm_prefix();
        let args: Vec<&str> = match update.manager {
            Manager::Winget => vec![
                "upgrade",
                "--id",
                &update.package,
                "--exact",
                "--scope",
                "machine",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ],
            Manager::Chocolatey => vec!["upgrade", &update.package, "--yes", "--no-progress"],
            Manager::Scoop => vec!["update", &update.package, "--global"],
            Manager::Npm => vec!["update", "-g", "--prefix", &npm_prefix, &update.package],
        };
        match run(&update.manager.resolve(), &args) {
            Ok(_) => result.updated += 1,
            Err(error) => result.errors.push(format!(
                "{} {}: {error}",
                update.manager.label(),
                update.package
            )),
        }
    }
    Ok(result)
}

pub(super) fn run(executable: &str, args: &[&str]) -> Result<String, String> {
    let output = command(executable)
        .args(args)
        .output()
        .map_err(|e| format!("{executable} unavailable: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{executable} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn machine_npm_prefix() -> String {
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    format!("{program_data}\\WinCommander\\npm")
}

pub(super) fn run_npm_outdated() -> Result<String, String> {
    let npm_prefix = machine_npm_prefix();
    let output = command(&Manager::Npm.resolve())
        .args(["outdated", "-g", "--prefix", &npm_prefix, "--json"])
        .output()
        .map_err(|e| format!("npm.cmd unavailable: {e}"))?;
    // npm exits 1 when it successfully reports outdated packages.
    if output.status.success() || !output.stdout.is_empty() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "npm.cmd failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
