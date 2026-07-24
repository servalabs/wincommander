use super::{CachedUpdate, Manager, PackageUpdateResult, CANCELLED};
use std::process::Command;
use std::sync::atomic::Ordering;

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
        let args: Vec<&str> = match update.manager {
            Manager::Winget => vec![
                "upgrade",
                "--id",
                &update.package,
                "--exact",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ],
            Manager::Chocolatey => vec!["upgrade", &update.package, "--yes", "--no-progress"],
            Manager::Scoop => vec!["update", &update.package],
            Manager::Npm => vec!["update", "-g", &update.package],
        };
        match run(update.manager.executable(), &args) {
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
    let output = Command::new(executable)
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

pub(super) fn run_npm_outdated() -> Result<String, String> {
    let output = Command::new("npm.cmd")
        .args(["outdated", "-g", "--json"])
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
