// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/vpn_kill_switch.rs
//
// ═══════════════════════════════════════════════════════════════════════
// VPN-DROP KILL SWITCH — Free, in-process tunnel watchdog
// ═══════════════════════════════════════════════════════════════════════
//
// When "armed", a background thread polls the active VPN tunnel state
// (Tailscale / Pvt Mesh and/or ProtonVPN). If a tunnel that was UP goes
// DOWN, it engages the existing internet kill switch (block-all firewall
// rules) so no traffic leaks over the bare connection until the tunnel
// comes back (or the user releases the block).
//
// All probes use native exes (tailscale.exe, netsh) — no PowerShell, no
// AV-flaggable strings, no Pro sidecar. The firewall ops are the same Free
// primitives the Dashboard's internet kill switch already uses
// (network_toggle::internet_kill_switch_set).
//
// Fires ONLY on a genuine UP→DOWN transition, never on the first poll
// (treated as UNKNOWN), so a VPN that hasn't connected yet at app launch
// doesn't trigger a spurious block.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const DEFAULT_POLL_SECS: u64 = 5;

// ── Runtime state ────────────────────────────────────────────────────
static ARMED: AtomicBool = AtomicBool::new(false);
static FIRED: AtomicBool = AtomicBool::new(false); // did WE engage the block?
static EPOCH: AtomicU64 = AtomicU64::new(0); // bumped on each arm/disarm; old threads exit
static LAST_FIRED_AT: AtomicI64 = AtomicI64::new(0); // epoch secs, 0 = never

// ── Pure tunnel model (unit-tested) ──────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Block,
    Release,
    None,
}

/// Decide what to do given the previous and current tunnel state.
/// Block ONLY on a real Up→Down transition; Release on Down→Up. Any
/// transition involving Unknown is a no-op (covers the first poll + probe
/// glitches), so we never cut traffic before the tunnel has had a chance
/// to come up.
pub fn evaluate_transition(prev: TunnelState, curr: TunnelState) -> Action {
    match (prev, curr) {
        (TunnelState::Up, TunnelState::Down) => Action::Block,
        (TunnelState::Down, TunnelState::Up) => Action::Release,
        _ => Action::None,
    }
}

/// True if an adapter line/name looks like a ProtonVPN tunnel (WireGuard
/// "ProtonVPN TUN"/"TAP" or OpenVPN "ProtonVPN").
pub fn is_protonvpn_adapter(name: &str) -> bool {
    name.to_ascii_lowercase().contains("proton")
}

// ── Probes (native exes only) ────────────────────────────────────────

#[cfg(target_os = "windows")]
fn probe_tailscale() -> TunnelState {
    let output = Command::new("tailscale.exe")
        .args(["ip", "--4"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let ip = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if ip.parse::<std::net::IpAddr>().is_ok() {
                TunnelState::Up
            } else {
                TunnelState::Down
            }
        }
        // Client present but not connected / not logged in.
        Ok(_) => TunnelState::Down,
        // Client not installed → can't judge this provider.
        Err(_) => TunnelState::Unknown,
    }
}

/// ProtonVPN tunnel state via `netsh interface show interface` (native, no PS).
///   Up      = a Proton interface is "Connected".
///   Down    = a Proton interface exists but is not connected.
///   Unknown = no Proton interface at all (not installed / different client).
#[cfg(target_os = "windows")]
fn probe_protonvpn() -> TunnelState {
    let mut cmd = Command::new("netsh");
    cmd.args(["interface", "show", "interface"]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = match cmd.output() {
        Ok(o) if o.status.success() => o,
        _ => return TunnelState::Unknown,
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut found = false;
    for line in text.lines() {
        if !is_protonvpn_adapter(line) {
            continue;
        }
        found = true;
        // netsh columns: Admin State | State | Type | Interface Name.
        // "State" == "Connected" (not "Disconnected") when the tunnel is up.
        let lower = line.to_ascii_lowercase();
        if lower.contains("connected") && !lower.contains("disconnected") {
            return TunnelState::Up;
        }
    }
    if found {
        TunnelState::Down
    } else {
        TunnelState::Unknown
    }
}

#[cfg(not(target_os = "windows"))]
fn probe_tailscale() -> TunnelState {
    TunnelState::Unknown
}
#[cfg(not(target_os = "windows"))]
fn probe_protonvpn() -> TunnelState {
    TunnelState::Unknown
}

/// Combine per-provider probes into one tunnel state for the configured
/// provider. "auto" = up if either is up; down only if a known provider is
/// present-but-down; otherwise unknown.
fn probe_state(provider: &str) -> TunnelState {
    match provider {
        "tailscale" => probe_tailscale(),
        "protonvpn" => probe_protonvpn(),
        _ => {
            let ts = probe_tailscale();
            if ts == TunnelState::Up {
                return TunnelState::Up;
            }
            let pv = probe_protonvpn();
            if pv == TunnelState::Up {
                return TunnelState::Up;
            }
            if ts == TunnelState::Down || pv == TunnelState::Down {
                TunnelState::Down
            } else {
                TunnelState::Unknown
            }
        }
    }
}

// ── Config (read from settings on arm) ───────────────────────────────

struct Config {
    provider: String,
    poll_secs: u64,
}

fn read_config() -> Config {
    let s = crate::settings::read_settings().ok();
    let vks = s.as_ref().map(|s| &s.ideal.network.vpn_kill_switch);
    Config {
        provider: vks
            .and_then(|v| v.provider.clone())
            .unwrap_or_else(|| "auto".to_string()),
        poll_secs: vks
            .and_then(|v| v.poll_interval_secs)
            .filter(|n| *n >= 1)
            .unwrap_or(DEFAULT_POLL_SECS),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Watcher control ──────────────────────────────────────────────────

fn spawn_watcher(app: AppHandle, my_epoch: u64) {
    std::thread::spawn(move || {
        let cfg = read_config();
        let interval = Duration::from_secs(cfg.poll_secs);
        // First observed state is the baseline; we never fire on it.
        let mut prev = TunnelState::Unknown;
        crate::log_message(
            "info",
            &format!(
                "[VpnKillSwitch] watcher started (provider={})",
                cfg.provider
            ),
        );
        loop {
            if EPOCH.load(Ordering::SeqCst) != my_epoch || !ARMED.load(Ordering::SeqCst) {
                crate::log_message("info", "[VpnKillSwitch] watcher stopped");
                return;
            }
            let curr = probe_state(&cfg.provider);
            match evaluate_transition(prev, curr) {
                Action::Block => {
                    if crate::network_toggle::internet_kill_switch_set(true).is_ok() {
                        FIRED.store(true, Ordering::SeqCst);
                        LAST_FIRED_AT.store(now_secs(), Ordering::SeqCst);
                        let _ = crate::native_notify::show_native_notification(
                            &app,
                            "VPN drop — internet cut",
                            "Your VPN tunnel dropped, so all internet was blocked to prevent a leak. Reconnect the VPN, or release the kill switch on the dashboard.",
                        );
                        let _ = app.emit("vpn-kill-switch-fired", true);
                        crate::log_message("warn", "[VpnKillSwitch] tunnel dropped — internet cut");
                    }
                }
                Action::Release => {
                    // Only auto-release the block if WE engaged it.
                    if FIRED.swap(false, Ordering::SeqCst) {
                        let _ = crate::network_toggle::internet_kill_switch_set(false);
                        let _ = app.emit("vpn-kill-switch-fired", false);
                        crate::log_message(
                            "info",
                            "[VpnKillSwitch] tunnel restored — internet released",
                        );
                    }
                }
                Action::None => {}
            }
            prev = curr;
            std::thread::sleep(interval);
        }
    });
}

/// Arm or disarm the watcher. Persists nothing — the frontend patches
/// `ideal.network.vpnKillSwitch.armed` so the UI reflects it on reload.
#[tauri::command]
pub fn vpn_kill_switch_arm(app: AppHandle, enable: bool) -> Result<(), String> {
    if enable {
        ARMED.store(true, Ordering::SeqCst);
        let epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
        spawn_watcher(app, epoch);
    } else {
        ARMED.store(false, Ordering::SeqCst);
        EPOCH.fetch_add(1, Ordering::SeqCst); // signal any running watcher to exit
                                              // If we had cut traffic, restore it on disarm so the user isn't stranded.
        if FIRED.swap(false, Ordering::SeqCst) {
            let _ = crate::network_toggle::internet_kill_switch_set(false);
            let _ = app.emit("vpn-kill-switch-fired", false);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnKsStatus {
    pub armed: bool,
    pub fired: bool,
    /// "up" | "down" | "unknown"
    pub tunnel_state: String,
    /// epoch seconds of the last block, 0 = never.
    pub last_fired_at: i64,
}

#[tauri::command]
pub fn vpn_kill_switch_status() -> VpnKsStatus {
    let armed = ARMED.load(Ordering::SeqCst);
    let state = if armed {
        let cfg = read_config();
        match probe_state(&cfg.provider) {
            TunnelState::Up => "up",
            TunnelState::Down => "down",
            TunnelState::Unknown => "unknown",
        }
    } else {
        "unknown"
    };
    VpnKsStatus {
        armed,
        fired: FIRED.load(Ordering::SeqCst),
        tunnel_state: state.to_string(),
        last_fired_at: LAST_FIRED_AT.load(Ordering::SeqCst),
    }
}

/// Called once from setup(): if the persisted config has armed=true, start
/// the watcher so the kill switch survives an app restart.
pub fn init_if_armed(app: &AppHandle) {
    let armed = crate::settings::read_settings()
        .ok()
        .and_then(|s| s.ideal.network.vpn_kill_switch.armed)
        .unwrap_or(false);
    if armed {
        ARMED.store(true, Ordering::SeqCst);
        let epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
        spawn_watcher(app.clone(), epoch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proton_adapter_name_recognised() {
        assert!(is_protonvpn_adapter("ProtonVPN TUN"));
        assert!(is_protonvpn_adapter(
            "  Enabled   Connected   Dedicated   ProtonVPN TAP"
        ));
        assert!(is_protonvpn_adapter("protonvpn"));
        assert!(!is_protonvpn_adapter("Ethernet"));
        assert!(!is_protonvpn_adapter("Wi-Fi"));
    }

    #[test]
    fn test_block_only_on_up_down() {
        assert_eq!(
            evaluate_transition(TunnelState::Up, TunnelState::Down),
            Action::Block
        );
        assert_eq!(
            evaluate_transition(TunnelState::Up, TunnelState::Up),
            Action::None
        );
        assert_eq!(
            evaluate_transition(TunnelState::Down, TunnelState::Down),
            Action::None
        );
    }

    #[test]
    fn test_no_fire_on_first_tick_or_glitch() {
        // First tick: prev is Unknown → never block.
        assert_eq!(
            evaluate_transition(TunnelState::Unknown, TunnelState::Down),
            Action::None
        );
        assert_eq!(
            evaluate_transition(TunnelState::Unknown, TunnelState::Up),
            Action::None
        );
        // Probe glitch to Unknown → no-op.
        assert_eq!(
            evaluate_transition(TunnelState::Up, TunnelState::Unknown),
            Action::None
        );
    }

    #[test]
    fn test_release_on_down_up() {
        assert_eq!(
            evaluate_transition(TunnelState::Down, TunnelState::Up),
            Action::Release
        );
    }

    #[test]
    fn test_config_roundtrip() {
        let cfg = crate::settings::VpnKillSwitchSettings {
            armed: Some(true),
            provider: Some("protonvpn".to_string()),
            poll_interval_secs: Some(10),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: crate::settings::VpnKillSwitchSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.armed, Some(true));
        assert_eq!(back.provider.as_deref(), Some("protonvpn"));
        assert_eq!(back.poll_interval_secs, Some(10));
    }
}
