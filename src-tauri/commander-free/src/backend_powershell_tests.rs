// SPDX-License-Identifier: AGPL-3.0-or-later
use super::build_powershell_command;
use std::os::windows::process::CommandExt;
use std::{
    io::Write,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn hidden_backend_reads_acls_without_foreign_parent_modules() {
    let fixture = tempfile::tempdir().expect("create disposable ACL fixture");
    let file = fixture.path().join("sentinel.txt");
    std::fs::write(&file, "harmless ACL fixture").unwrap();
    let foreign_path = fixture.path().join("foreign-modules");
    let inherited = std::env::var("PSModulePath").unwrap_or_default();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "backend::powershell_host_tests::acl_probe_in_foreign_parent",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "PSModulePath",
            format!("{};{inherited}", foreign_path.display()),
        )
        .env("WINCMD_TEST_FOREIGN_MODULE_PATH", &foreign_path)
        .env("WINCMD_TEST_ACL_DIRECTORY", fixture.path())
        .env("WINCMD_TEST_ACL_FILE", &file)
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start isolated backend regression probe");
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("backend ACL regression probe exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "isolated backend ACL probe failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "isolated helper invoked by hidden_backend_reads_acls_without_foreign_parent_modules"]
fn acl_probe_in_foreign_parent() {
    assert!(std::env::var_os("WINCMD_TEST_ACL_FILE").is_some());
    let (mut command, _) = build_powershell_command();
    let mut child = command.spawn().expect("start actual hidden backend host");
    // One statement avoids stdin's interactive multiline-block terminator.
    let script = concat!(
        "& { $ErrorActionPreference='Stop'; ",
        "if (($env:PSModulePath -split ';') -contains $env:WINCMD_TEST_FOREIGN_MODULE_PATH) ",
        "{ throw 'Foreign PowerShell module path reached backend' }; ",
        "foreach ($path in @($env:WINCMD_TEST_ACL_DIRECTORY,$env:WINCMD_TEST_ACL_FILE)) ",
        "{ $acl=Get-Acl -LiteralPath $path -ErrorAction Stop; ",
        "if (-not $acl.GetOwner([Security.Principal.SecurityIdentifier]).Value) ",
        "{ throw 'ACL owner was not read' } }; ",
        "$registryAcl=Get-Acl -Path 'HKLM:\\SOFTWARE' -ErrorAction Stop; ",
        "if (-not $registryAcl.GetOwner([Security.Principal.SecurityIdentifier]).Value) ",
        "{ throw 'Registry ACL owner was not read' }; ",
        "$module=Get-Module Microsoft.PowerShell.Security; ",
        "if (-not $module.Path.StartsWith($PSHOME,[StringComparison]::OrdinalIgnoreCase)) ",
        "{ throw 'Wrong PowerShell Security module loaded' }; ",
        "[Console]::Out.Write('backend-acl-ok') }\n"
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "hidden backend failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "backend-acl-ok"
    );
}
