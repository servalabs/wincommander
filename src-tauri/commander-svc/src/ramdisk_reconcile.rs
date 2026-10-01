// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-time repair for duplicate ImDisk TEMP R: units left by older clients.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

mod windows;
use windows::{
    eject_without_mountpoint, mount_existing_unit, mount_owner, mounted_label, unit_label,
};

const SIZE_BYTES: u64 = 768 * 1024 * 1024;

#[derive(Debug)]
struct Unit {
    number: u32,
    letter: Option<char>,
    size_bytes: Option<u64>,
    is_vm: bool,
    has_no_image: bool,
}

impl Unit {
    fn is_temp_r_candidate(&self) -> bool {
        self.letter == Some('R')
            && self.size_bytes == Some(SIZE_BYTES)
            && self.is_vm
            && self.has_no_image
    }
}

fn parse_unit(number: u32, text: &str) -> Unit {
    let mut unit = Unit {
        number,
        letter: None,
        size_bytes: None,
        is_vm: false,
        has_no_image: false,
    };
    for line in text.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("Drive letter: ") {
            unit.letter = value.chars().next().map(|c| c.to_ascii_uppercase());
        } else if let Some(value) = line.strip_prefix("Mount point: ") {
            unit.letter = value.chars().next().map(|c| c.to_ascii_uppercase());
        } else if line == "No image file." {
            unit.has_no_image = true;
        } else if let Some(value) = line.strip_prefix("Size: ") {
            unit.size_bytes = value.split_whitespace().next().and_then(|n| n.parse().ok());
            unit.is_vm = value.to_ascii_lowercase().contains("virtual memory");
        }
    }
    unit
}

fn imdisk_exe() -> Option<PathBuf> {
    [
        "SystemRoot",
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ]
    .into_iter()
    .filter_map(|key| std::env::var_os(key).map(|root| (key, root)))
    .map(|(key, root)| {
        let mut path = PathBuf::from(root);
        if key == "SystemRoot" {
            path.push("System32");
        } else {
            path.push("ImDisk");
        }
        path.push("imdisk.exe");
        path
    })
    .find(|path| path.is_file())
}

fn run(exe: &PathBuf, args: &[&str]) -> Result<std::process::Output> {
    Command::new(exe)
        .args(args)
        .output()
        .with_context(|| format!("could not run ImDisk {}", args.join(" ")))
}

fn output_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn device_list_succeeded(output: &std::process::Output) -> bool {
    device_list_result_succeeded(output.status.code(), &output.stderr)
}

fn device_list_result_succeeded(exit_code: Option<i32>, stderr: &[u8]) -> bool {
    matches!(exit_code, Some(0 | 1)) && stderr.iter().all(u8::is_ascii_whitespace)
}

pub fn reconcile_temp_r() -> Result<()> {
    let Some(exe) = imdisk_exe() else {
        return Ok(());
    };
    windows::with_coordination_lock(|| reconcile_locked(&exe))
}

fn reconcile_locked(exe: &PathBuf) -> Result<()> {
    let mut owner = mount_owner()?;
    if owner.is_some() {
        let mounted = run(exe, &["-l", "-m", "R:"])?;
        if !mounted.status.success() {
            bail!("could not inspect live R: RAM disk");
        }
    }
    let list = run(exe, &["-l", "-n"])?;
    if !device_list_succeeded(&list) {
        bail!("could not enumerate ImDisk units");
    }
    let mut units = Vec::new();
    for line in output_text(&list)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let number: u32 = line.parse().context("unrecognized ImDisk device list")?;
        let details = run(exe, &["-l", "-u", &number.to_string()])?;
        if !details.status.success() {
            bail!("could not inspect ImDisk unit {number}");
        }
        units.push(parse_unit(number, &output_text(&details)));
    }
    if let Some(number) = owner {
        if !units
            .iter()
            .any(|unit| unit.number == number && unit.is_temp_r_candidate())
            || !mounted_label()?.eq_ignore_ascii_case("TEMP")
        {
            return Ok(()); // Unrelated R: disk: leave it untouched.
        }
    }
    if units
        .iter()
        .any(|unit| unit.letter == Some('R') && !unit.is_temp_r_candidate())
    {
        bail!("R: contains an unrecognized ImDisk unit");
    }
    if owner.is_none() {
        for candidate in units.iter().filter(|unit| unit.is_temp_r_candidate()) {
            if !unit_label(candidate.number)?.eq_ignore_ascii_case("TEMP") {
                bail!("ImDisk unit {} is not a TEMP volume", candidate.number);
            }
            if mount_existing_unit(candidate.number)? {
                owner = Some(candidate.number);
                break;
            }
        }
        if owner.is_none() {
            return Ok(()); // No matching attached TEMP disk; creation is opt-in.
        }
        if !run(exe, &["-l", "-m", "R:"])?.status.success() {
            bail!("restored R: could not be inspected by ImDisk");
        }
    }
    let Some(owner) = owner else { return Ok(()) };
    for unit in units
        .iter()
        .filter(|unit| unit.is_temp_r_candidate() && unit.number != owner)
    {
        eject_without_mountpoint(unit.number)?;
        if mount_owner()? != Some(owner) {
            bail!("R: mount changed during duplicate removal");
        }
        let mut remains = true;
        for _ in 0..15 {
            remains = run(exe, &["-l", "-u", &unit.number.to_string()])?
                .status
                .success();
            if !remains {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if remains {
            bail!("stale ImDisk unit {} remains attached", unit.number);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_cli_shape_identifies_only_matching_temp_ram_disk() {
        let live = parse_unit(7, "Drive letter: R\nNo image file.\nSize: 805306368 bytes (768 MB), Removable, Virtual Memory, HDD, Modified.\n");
        assert!(live.is_temp_r_candidate());
        assert_eq!(live.number, 7);
        let image = parse_unit(8, "Drive letter: R\nImage file: C:\\disk.img\nSize: 805306368 bytes (768 MB), Virtual Memory, HDD.\n");
        assert!(!image.is_temp_r_candidate());
        let other_size = parse_unit(9, "Drive letter: R\nNo image file.\nSize: 536870912 bytes (512 MB), Virtual Memory, HDD.\n");
        assert!(!other_size.is_temp_r_candidate());
    }

    #[test]
    fn imdisk_device_list_accepts_its_unusual_success_code() {
        assert!(device_list_result_succeeded(Some(1), b""));
        assert!(device_list_result_succeeded(Some(0), b""));
        assert!(!device_list_result_succeeded(
            Some(0),
            b"driver unavailable"
        ));
        assert!(!device_list_result_succeeded(Some(9), b""));
    }
}
