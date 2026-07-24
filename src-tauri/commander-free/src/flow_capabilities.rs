// src-tauri/commander-free/src/flow_capabilities.rs
//
// ═══════════════════════════════════════════════════════════════════════
// FLOW CAPABILITY PROBE — machine-local "what works" scan
// ═══════════════════════════════════════════════════════════════════════
//
// Surfaces, for each platform-bound subsystem a Flow can depend on,
// whether the subsystem is healthy on THIS machine right now. Two
// audiences:
//
//   1. Operator: opens the Capability Check dialog and sees a green
//      checklist of "what your flows can rely on", or red rows
//      explaining why a particular trigger type won't fire.
//   2. Future health-dashboard: the per-block status map feeds the
//      "Why is this flow not ready?" tile.
//
// The probe is deliberately CHEAP — each check returns in <50ms or
// fails fast. The dialog re-runs the probe each time it opens; we
// don't cache. (If we ever need to cache, do it in the frontend, not
// here — the operator might be plugging/unplugging USB / starting
// Tailscale between probes.)
//
// Design notes:
//   - Checks are pure-function `fn() -> CapabilityStatus` — no shared
//     state, no AppHandle, no `&self`. Easy to extend; easy to test
//     in isolation.
//   - Per-block readiness is a derived view, not its own probe — we
//     express it as a static map of (block_type → required check_ids)
//     and roll up at the end.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityStatus {
    /// Probe ran and the subsystem is fully functional.
    Ok,
    /// Probe ran but produced a non-fatal anomaly (e.g. zero USB
    /// devices currently attached — listener works, just nothing to
    /// observe yet).
    Warn,
    /// Probe failed outright. Flows depending on this subsystem won't
    /// fire and the operator needs to investigate.
    Fail,
    /// Probe was skipped — typically platform-gated (e.g. lid-close on
    /// non-Windows builds).
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCheck {
    /// Stable id used by per-block dependency rollup. Lowercase, dashed.
    pub id: String,
    /// User-facing label rendered in the dialog.
    pub label: String,
    pub status: CapabilityStatus,
    /// Optional one-line explanation of WHY this state — surfaced in
    /// the dialog as a subtitle. Always present for Warn/Fail; usually
    /// empty for Ok.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    /// Every probe, in dialog-display order.
    pub checks: Vec<CapabilityCheck>,
    /// Block-type → rolled-up readiness across the checks that block
    /// depends on. `"ok"` iff every required check is Ok; `"warn"` if
    /// any are Warn but none Fail; `"fail"` if any Fail.
    pub block_readiness: HashMap<String, String>,
}

// ═══════════════════════════════════════════════════════════════════════
// INDIVIDUAL PROBES
// ═══════════════════════════════════════════════════════════════════════

fn check_windows_platform() -> CapabilityCheck {
    let is_win = cfg!(target_os = "windows");
    CapabilityCheck {
        id: "windows-platform".to_string(),
        label: "Windows platform APIs".to_string(),
        status: if is_win {
            CapabilityStatus::Ok
        } else {
            CapabilityStatus::Fail
        },
        detail: if is_win {
            String::new()
        } else {
            "Build is not compiled for Windows — most Flow triggers are Windows-only.".to_string()
        },
    }
}

/// Probe the Pvt Mesh VPN by calling `tailscale.exe ip --4` (Tailscale
/// is the underlying implementation; we never surface that name to the
/// operator). Used to gate WebhookTrigger / SignalReceivedTrigger /
/// NetworkTrigger flows. Cheap (~20ms cold, ~5ms warm) and synchronous
/// so we don't have to thread `tokio` into the probe surface.
fn check_tailscale_up() -> CapabilityCheck {
    #[cfg(target_os = "windows")]
    {
        // Avoid spawning a window on Windows.
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("tailscale.exe")
            .args(["ip", "--4"])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
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
                    return CapabilityCheck {
                        id: "tailscale-up".to_string(),
                        label: "Pvt Mesh VPN".to_string(),
                        status: CapabilityStatus::Ok,
                        detail: format!("Bound to {}", ip),
                    };
                }
                CapabilityCheck {
                    id: "tailscale-up".to_string(),
                    label: "Pvt Mesh VPN".to_string(),
                    status: CapabilityStatus::Fail,
                    detail: format!("Unparseable mesh IPv4: '{}'", ip),
                }
            }
            Ok(_) => CapabilityCheck {
                id: "tailscale-up".to_string(),
                label: "Pvt Mesh VPN".to_string(),
                status: CapabilityStatus::Fail,
                detail: "Pvt Mesh VPN not running or not logged in.".to_string(),
            },
            Err(e) => CapabilityCheck {
                id: "tailscale-up".to_string(),
                label: "Pvt Mesh VPN".to_string(),
                status: CapabilityStatus::Fail,
                detail: format!("Pvt Mesh VPN client not found ({}).", e),
            },
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        CapabilityCheck {
            id: "tailscale-up".to_string(),
            label: "Pvt Mesh VPN".to_string(),
            status: CapabilityStatus::Skipped,
            detail: "Non-Windows build — mesh probe disabled.".to_string(),
        }
    }
}

/// Probe the keyboard-hook subsystem. We don't actually install a hook
/// here (it has process-lifetime side effects) — we just verify the
/// shared service module is built+linked.
fn check_keyboard_hook() -> CapabilityCheck {
    // Compile-time gate: the keyboard_hook module is Windows-only.
    #[cfg(target_os = "windows")]
    {
        // The service auto-starts on first subscribe. We don't probe
        // by subscribing because the WH_KEYBOARD_LL install requires
        // a message loop, which we have running in the main thread —
        // touching it from this probe path would race. Compile-time
        // presence is sufficient.
        CapabilityCheck {
            id: "keyboard-hook".to_string(),
            label: "Global keyboard hook".to_string(),
            status: CapabilityStatus::Ok,
            detail: String::new(),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        CapabilityCheck {
            id: "keyboard-hook".to_string(),
            label: "Global keyboard hook".to_string(),
            status: CapabilityStatus::Skipped,
            detail: "Non-Windows build — WH_KEYBOARD_LL unavailable.".to_string(),
        }
    }
}

/// Probe the filesystem-watcher service by creating + watching + dropping
/// a temp directory. This is the strongest possible "it works" signal
/// short of doing the actual file-event test, and takes <10ms.
fn check_fs_watcher() -> CapabilityCheck {
    use crate::services::fs_watcher;

    // Use the system temp dir — present everywhere, no cleanup issue
    // since we drop the subscription immediately.
    let probe_path = std::env::temp_dir();
    match fs_watcher::subscribe(probe_path.clone(), false) {
        Ok(_handle) => {
            // Handle drops here, which unsubscribes. We don't need
            // to wait for an event — successful subscribe is enough.
            CapabilityCheck {
                id: "fs-watcher".to_string(),
                label: "Filesystem watcher".to_string(),
                status: CapabilityStatus::Ok,
                detail: format!("Subscribed + released on {}", probe_path.display()),
            }
        }
        Err(e) => CapabilityCheck {
            id: "fs-watcher".to_string(),
            label: "Filesystem watcher".to_string(),
            status: CapabilityStatus::Fail,
            detail: format!("subscribe() failed: {}", e),
        },
    }
}

/// Probe by reading the system clipboard (synchronous, harmless).
fn check_clipboard_read() -> CapabilityCheck {
    #[cfg(target_os = "windows")]
    {
        // clipboard-win is the same lib used by paste monitor.
        use clipboard_win::{formats, get_clipboard};
        match get_clipboard::<String, _>(formats::Unicode) {
            Ok(_) => CapabilityCheck {
                id: "clipboard-read".to_string(),
                label: "Clipboard read access".to_string(),
                status: CapabilityStatus::Ok,
                detail: String::new(),
            },
            Err(e) => CapabilityCheck {
                id: "clipboard-read".to_string(),
                label: "Clipboard read access".to_string(),
                // Clipboard contention is a soft failure — another app
                // holding the clipboard for a frame doesn't break
                // anything permanently.
                status: CapabilityStatus::Warn,
                detail: format!(
                    "Read failed once ({}); may be transient (another app holding clipboard).",
                    e
                ),
            },
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        CapabilityCheck {
            id: "clipboard-read".to_string(),
            label: "Clipboard read access".to_string(),
            status: CapabilityStatus::Skipped,
            detail: "Non-Windows build — clipboard-win unavailable.".to_string(),
        }
    }
}

/// USB watcher: probe by listing currently-attached USB devices. Zero
/// devices is `Warn` (listener works; nothing to observe).
async fn check_usb_watcher() -> CapabilityCheck {
    match crate::flow_engine::list_usb_devices().await {
        Ok(value) => {
            let count = match &value {
                serde_json::Value::Array(a) => a.len(),
                // PowerShell ConvertTo-Json wraps a single object; treat as 1.
                serde_json::Value::Object(_) => 1,
                _ => 0,
            };
            if count == 0 {
                CapabilityCheck {
                    id: "usb-watcher".to_string(),
                    label: "USB device watcher".to_string(),
                    status: CapabilityStatus::Warn,
                    detail: "Watcher reachable but no USB devices currently attached.".to_string(),
                }
            } else {
                CapabilityCheck {
                    id: "usb-watcher".to_string(),
                    label: "USB device watcher".to_string(),
                    status: CapabilityStatus::Ok,
                    detail: format!("{} USB device(s) currently attached", count),
                }
            }
        }
        Err(e) => CapabilityCheck {
            id: "usb-watcher".to_string(),
            label: "USB device watcher".to_string(),
            status: CapabilityStatus::Fail,
            detail: format!("Get-PnpDevice failed: {}", e),
        },
    }
}

/// Global-hotkey support. We can't actually register one without
/// conflicting with the panic-hotkey — but if the panic hotkey is
/// registered at all, the subsystem is healthy.
fn check_global_hotkey() -> CapabilityCheck {
    // The tauri-plugin-global-shortcut crate is unconditional in our
    // Cargo.toml so its support is compile-time true on every platform
    // we ship to. The runtime question is whether ANOTHER process is
    // hogging the same hotkey — but that's per-hotkey, not subsystem-
    // level, so it doesn't belong in this probe.
    CapabilityCheck {
        id: "global-hotkey".to_string(),
        label: "Global hotkey registration".to_string(),
        status: CapabilityStatus::Ok,
        detail: String::new(),
    }
}

/// Webhook listener: depends on the mesh VPN being up + the port being
/// bindable. Doesn't actually bind — bind-test would race against the
/// real listener if a flow has one armed.
fn check_webhook_listener(tailscale_status: CapabilityStatus) -> CapabilityCheck {
    if tailscale_status != CapabilityStatus::Ok {
        return CapabilityCheck {
            id: "webhook-listener".to_string(),
            label: "Webhook listener (mesh-bound)".to_string(),
            status: CapabilityStatus::Fail,
            detail:
                "Pvt Mesh VPN must be up first — webhook listener is bound to the mesh IP only."
                    .to_string(),
        };
    }
    CapabilityCheck {
        id: "webhook-listener".to_string(),
        label: "Webhook listener (mesh-bound)".to_string(),
        status: CapabilityStatus::Ok,
        detail: String::new(),
    }
}

/// Privacy Shield event bus. NOT YET WIRED — this is the dependency
/// for CameraTrigger, and the roadmap calls out `flows.fix-camera`
/// as blocked on Privacy Shield emitting events. So always Fail until
/// that ships.
fn check_privacy_shield_bus() -> CapabilityCheck {
    CapabilityCheck {
        id: "privacy-shield-bus".to_string(),
        label: "Privacy Shield event bus".to_string(),
        status: CapabilityStatus::Fail,
        detail: "Privacy Shield doesn't emit events yet — CameraTrigger flows cannot fire. Tracked as `flows.fix-camera` in the roadmap.".to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// BLOCK READINESS ROLLUP
// ═══════════════════════════════════════════════════════════════════════

/// For each block type, which `CapabilityCheck.id`s must be Ok for the
/// block to fire. Empty list = always Ok (e.g. ScheduleTrigger has no
/// platform dependency).
fn block_requirements() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
    // Triggers
    m.insert("HotkeyTrigger", vec!["global-hotkey"]);
    m.insert("KeySequenceTrigger", vec!["keyboard-hook"]);
    m.insert("USBTrigger", vec!["usb-watcher"]);
    m.insert("LidCloseTrigger", vec!["windows-platform"]);
    m.insert("WebhookTrigger", vec!["tailscale-up", "webhook-listener"]);
    m.insert("CameraTrigger", vec!["privacy-shield-bus"]);
    m.insert("ScheduleTrigger", vec![]); // cron is pure-Rust
    m.insert("NetworkTrigger", vec!["tailscale-up"]);
    m.insert("FileTrigger", vec!["fs-watcher"]);
    m.insert("ProcessTrigger", vec!["windows-platform"]);
    m.insert("SignalReceivedTrigger", vec!["tailscale-up"]);
    m.insert(
        "PasteMonitorTrigger",
        vec!["keyboard-hook", "clipboard-read"],
    );
    m.insert("DecoyMonitorTrigger", vec!["fs-watcher"]);
    m.insert("RansomwareMonitorTrigger", vec!["fs-watcher"]);
    // Conditions + Actions deliberately omitted — none have I/O
    // dependencies the probe currently knows how to test.
    m
}

/// Roll up per-block status from a list of checks.
fn rollup_block_readiness(checks: &[CapabilityCheck]) -> HashMap<String, String> {
    let by_id: HashMap<&str, CapabilityStatus> =
        checks.iter().map(|c| (c.id.as_str(), c.status)).collect();

    let mut out = HashMap::new();
    for (block, reqs) in block_requirements() {
        if reqs.is_empty() {
            out.insert(block.to_string(), "ok".to_string());
            continue;
        }
        let mut worst = CapabilityStatus::Ok;
        for req in reqs {
            let s = by_id.get(req).copied().unwrap_or(CapabilityStatus::Skipped);
            // ordering: Fail > Warn > Skipped > Ok
            worst = match (worst, s) {
                (CapabilityStatus::Fail, _) | (_, CapabilityStatus::Fail) => CapabilityStatus::Fail,
                (CapabilityStatus::Warn, _) | (_, CapabilityStatus::Warn) => CapabilityStatus::Warn,
                (CapabilityStatus::Skipped, _) | (_, CapabilityStatus::Skipped) => {
                    CapabilityStatus::Skipped
                }
                _ => CapabilityStatus::Ok,
            };
        }
        let s = match worst {
            CapabilityStatus::Ok => "ok",
            CapabilityStatus::Warn => "warn",
            CapabilityStatus::Fail => "fail",
            CapabilityStatus::Skipped => "skipped",
        };
        out.insert(block.to_string(), s.to_string());
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMAND
// ═══════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn probe_flow_capabilities() -> Result<CapabilityReport, String> {
    // Run cheap synchronous checks first.
    let windows = check_windows_platform();
    let tailscale = check_tailscale_up();
    let keyboard = check_keyboard_hook();
    let fs = check_fs_watcher();
    let clipboard = check_clipboard_read();
    let global_hotkey = check_global_hotkey();
    let privacy_shield = check_privacy_shield_bus();
    let webhook = check_webhook_listener(tailscale.status);

    // Async check.
    let usb = check_usb_watcher().await;

    // Order chosen for the dialog: platform → mesh → input → fs →
    // peripherals → derived listeners → known-broken.
    let checks = vec![
        windows,
        tailscale,
        keyboard,
        fs,
        clipboard,
        global_hotkey,
        webhook,
        usb,
        privacy_shield,
    ];

    let block_readiness = rollup_block_readiness(&checks);

    Ok(CapabilityReport {
        checks,
        block_readiness,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollup_all_ok_returns_ok() {
        let checks = vec![
            CapabilityCheck {
                id: "tailscale-up".to_string(),
                label: "".to_string(),
                status: CapabilityStatus::Ok,
                detail: String::new(),
            },
            CapabilityCheck {
                id: "webhook-listener".to_string(),
                label: "".to_string(),
                status: CapabilityStatus::Ok,
                detail: String::new(),
            },
        ];
        let m = rollup_block_readiness(&checks);
        assert_eq!(m.get("WebhookTrigger").map(String::as_str), Some("ok"));
    }

    #[test]
    fn rollup_any_fail_returns_fail() {
        let checks = vec![
            CapabilityCheck {
                id: "tailscale-up".to_string(),
                label: "".to_string(),
                status: CapabilityStatus::Fail,
                detail: String::new(),
            },
            CapabilityCheck {
                id: "webhook-listener".to_string(),
                label: "".to_string(),
                status: CapabilityStatus::Ok,
                detail: String::new(),
            },
        ];
        let m = rollup_block_readiness(&checks);
        assert_eq!(m.get("WebhookTrigger").map(String::as_str), Some("fail"));
    }

    #[test]
    fn rollup_no_deps_returns_ok() {
        let m = rollup_block_readiness(&[]);
        // ScheduleTrigger has no deps so should be Ok regardless of input.
        assert_eq!(m.get("ScheduleTrigger").map(String::as_str), Some("ok"));
    }

    #[test]
    fn fs_watcher_probe_succeeds_in_test_env() {
        let check = check_fs_watcher();
        // Test env should always have a writable temp dir.
        assert!(matches!(
            check.status,
            CapabilityStatus::Ok | CapabilityStatus::Warn
        ));
    }

    #[test]
    fn privacy_shield_always_fails_today() {
        let check = check_privacy_shield_bus();
        assert_eq!(check.status, CapabilityStatus::Fail);
        assert!(check.detail.contains("fix-camera"));
    }
}
