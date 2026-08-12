// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/internet_kill_switch.rs
//
// ═══════════════════════════════════════════════════════════════════════
// INTERNET KILL SWITCH — Free, in-process Windows Firewall control
// ═══════════════════════════════════════════════════════════════════════
//
// A one-tap "cut all internet" control surfaced on the Dashboard
// (SystemStatusCard) alongside the CAM/MIC capability toggles.
//
// When engaged, two Windows Firewall rules are added that block ALL
// traffic — outbound AND inbound — across every profile:
//   WinCommander-KillSwitch-Out  (dir=out  action=block)
//   WinCommander-KillSwitch-In   (dir=in   action=block)
//
// This is a true hard cut: WinCommander's own network activity (licence
// checks, update checker, AI advisor) is blocked too, by design.
//
// Why netsh, not New-NetFirewallRule: this is a FREE command that must run
// in-process (no Pro sidecar, no PowerShell module / encrypt step). netsh
// is a native exe, not a `shell.Command("powershell")` call, so it doesn't
// trip the AGENTS.md "never shell powershell from JS" rule — and the tier
// gate + arg construction all stay in Rust. Rule names are fixed constants;
// nothing from the frontend reaches the command line.

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn rule_out() -> String {
    crate::paths::firewall_rule_name("KillSwitch-Out")
}
fn rule_in() -> String {
    crate::paths::firewall_rule_name("KillSwitch-In")
}

fn netsh() -> Command {
    let mut cmd = Command::new("netsh");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Add a block-all rule for the given direction. Idempotent: an existing
/// rule with the same name is deleted first so we never stack duplicates.
fn add_block_rule(name: &str, direction: &str) -> Result<(), String> {
    // Best-effort delete first (ignore "no rules match" failures).
    let _ = netsh()
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={}", name),
        ])
        .output();

    let out = netsh()
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={}", name),
            &format!("dir={}", direction),
            "action=block",
            "enable=yes",
            "profile=any",
        ])
        .output()
        .map_err(|e| format!("netsh add {} failed to launch: {}", name, e))?;

    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "netsh add {} failed: {}",
            name,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn delete_rule(name: &str) -> Result<(), String> {
    // `delete rule` returns a non-zero exit code when no rule matches; that's
    // fine for our purposes (disabling an already-absent switch). Only treat
    // a launch failure as an error.
    netsh()
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={}", name),
        ])
        .output()
        .map_err(|e| format!("netsh delete {} failed to launch: {}", name, e))?;
    Ok(())
}

fn rule_exists(name: &str) -> bool {
    match netsh()
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            &format!("name={}", name),
        ])
        .output()
    {
        // `show rule` prints "No rules match the specified criteria." and
        // exits non-zero when the rule is absent; success means it exists.
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Engage or release the internet kill switch. Returns the resulting state
/// (true = internet cut). Persists nothing itself — the caller patches
/// `settings.app.internetKillSwitch` so the Dashboard reflects it on reload.
#[tauri::command]
pub fn internet_kill_switch_set(enable: bool) -> Result<bool, String> {
    // Rate-limit (audit L1): a compromised WebView could otherwise script rapid
    // toggles as a network-disruption/harassment vector. The action is reversible
    // so no confirmation prompt is added — just a floor on call frequency.
    {
        static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        let mut last = LAST.lock().unwrap();
        if let Some(t) = *last {
            if t.elapsed().as_millis() < 1500 {
                return Err(
                    "Please wait a moment before toggling the kill switch again.".to_string(),
                );
            }
        }
        *last = Some(std::time::Instant::now());
    }
    if enable {
        add_block_rule(&rule_out(), "out")?;
        // If the inbound rule fails, roll back the outbound one so we never
        // leave the machine in a half-blocked state the UI can't represent.
        if let Err(e) = add_block_rule(&rule_in(), "in") {
            let _ = delete_rule(&rule_out());
            return Err(e);
        }
        crate::log_message("info", "[KillSwitch] internet cut — block rules added");
        Ok(true)
    } else {
        delete_rule(&rule_out())?;
        delete_rule(&rule_in())?;
        crate::log_message(
            "info",
            "[KillSwitch] internet restored — block rules removed",
        );
        Ok(false)
    }
}

/// Report whether the kill switch is currently engaged. The outbound rule is
/// the authoritative marker (both are added/removed together).
#[tauri::command]
pub fn internet_kill_switch_get() -> bool {
    rule_exists(&rule_out())
}
