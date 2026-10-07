// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fix All uses each reviewed command's real Windows scope, not its elevation flag.
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    CurrentUser,
    Machine,
    Mixed,
    Native,
}

impl Scope {
    pub(super) fn requires_elevation(self) -> bool {
        self != Self::CurrentUser
    }
    fn label(self) -> &'static str {
        match self {
            Self::CurrentUser => "current-user",
            Self::Machine => "machine",
            Self::Mixed => "machine-and-current-user",
            Self::Native => "application-defined",
        }
    }
}

pub(super) fn resolve(command: &str, machine_policy: bool) -> Option<Scope> {
    if machine_policy {
        return Some(Scope::Machine);
    }
    Some(match command {
        "Hide-SyncProviderNotifications"
        | "Show-SyncProviderNotifications"
        | "Enable-EnthusiastMode"
        | "Disable-EnthusiastMode"
        | "Show-DesktopIconRecycleBin"
        | "Hide-DesktopIconRecycleBin"
        | "Show-ClockSeconds"
        | "Hide-ClockSeconds"
        | "Disable-RecentFilesTracking"
        | "Enable-RecentFilesTracking"
        | "Disable-TerminalHistory"
        | "Enable-TerminalHistory" => Scope::CurrentUser,
        "Disable-CrashDumps"
        | "Enable-CrashDumps"
        | "Enable-KernelDMAProtection"
        | "Disable-KernelDMAProtection"
        | "Set-DesktopShellPriority"
        | "Reset-DesktopShellPriority"
        | "Enable-ForensicToolBlock"
        | "Disable-ForensicToolBlock"
        | "Enable-LidClosePowerOff"
        | "Disable-LidClosePowerOff"
        | "Enable-RamSpillControl"
        | "Disable-RamSpillControl"
        | "Disable-IEEnhancedSecurity"
        | "Enable-IEEnhancedSecurity"
        | "Set-ServicesManual" => Scope::Machine,
        // Screensaver protection is HKCU; the active power plan is machine-wide.
        "Disable-SleepPassword" | "Enable-SleepPassword" => Scope::Mixed,
        // The package/browser operation retains its own scope and authorization.
        "Install-Dependency" | "Enable-HardenBrowserByName" => Scope::Native,
        _ => return None,
    })
}

pub(super) fn annotate(command: &str, scope: Scope, value: Value) -> Value {
    let state = value.get("status").and_then(Value::as_str).unwrap_or("");
    let error = value.get("error").is_some_and(|error| {
        error.as_bool() == Some(true) || error.as_str().is_some_and(|s| !s.is_empty())
    });
    let service_failures = command == "Set-ServicesManual"
        && ["manual", "disable"].iter().any(|section| {
            value[*section]["failed"]
                .as_array()
                .is_some_and(|failures| !failures.is_empty())
        });
    let status = if error
        || service_failures
        || value["ok"] == false
        || value["success"] == false
        || matches!(
            state,
            "failed" | "error" | "blocked" | "unsupported" | "partial"
        ) {
        if state == "blocked" {
            "blocked"
        } else {
            "failed"
        }
    } else if matches!(
        state,
        "preference_set_reboot_needed" | "pending" | "reboot_required"
    ) || (command == "Enable-KernelDMAProtection" && value["actuallyActive"] != true)
    {
        "pending"
    } else {
        "applied"
    };
    match value {
        Value::Object(mut object) => {
            if let Some(original) = object.get("status").cloned() {
                object
                    .entry("operationStatus".to_string())
                    .or_insert(original);
            }
            object.insert("scope".into(), json!(scope.label()));
            object.insert("status".into(), json!(status));
            Value::Object(object)
        }
        data => json!({"command":command,"scope":scope.label(),"status":status,"data":data}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reported_user_preferences_run_only_for_current_user() {
        for command in [
            "Hide-SyncProviderNotifications",
            "Show-SyncProviderNotifications",
            "Enable-EnthusiastMode",
            "Disable-EnthusiastMode",
            "Show-DesktopIconRecycleBin",
            "Hide-DesktopIconRecycleBin",
            "Show-ClockSeconds",
            "Hide-ClockSeconds",
        ] {
            let scope = resolve(command, false).unwrap();
            assert_eq!(scope, Scope::CurrentUser);
            assert!(!scope.requires_elevation());
            assert_eq!(
                annotate(command, scope, json!({"success":true}))["scope"],
                "current-user"
            );
        }
    }
    #[test]
    fn machine_and_mixed_actions_never_become_user_only() {
        for command in [
            "Disable-CrashDumps",
            "Enable-KernelDMAProtection",
            "Set-DesktopShellPriority",
            "Enable-ForensicToolBlock",
            "Enable-LidClosePowerOff",
            "Enable-RamSpillControl",
            "Enable-IEEnhancedSecurity",
        ] {
            assert_eq!(resolve(command, false), Some(Scope::Machine));
        }
        assert_eq!(resolve("Disable-SleepPassword", false), Some(Scope::Mixed));
        assert!(Scope::Machine.requires_elevation());
        assert!(Scope::Mixed.requires_elevation());
        assert_eq!(resolve("Disable-Telemetry", true), Some(Scope::Machine));
        assert_eq!(resolve("arbitrary-command", false), None);
    }
    #[test]
    fn receipts_preserve_failures_and_unconfirmed_hardware() {
        for result in [
            json!({"success":false}),
            json!({"ok":false}),
            json!({"error":"denied"}),
            json!({"status":"unsupported"}),
            json!({"status":"partial"}),
        ] {
            assert_eq!(
                annotate("example", Scope::Machine, result)["status"],
                "failed"
            );
        }
        let pending = annotate(
            "Enable-KernelDMAProtection",
            Scope::Machine,
            json!({"status":"preference_set_reboot_needed","actuallyActive":false}),
        );
        assert_eq!(pending["status"], "pending");
        assert_eq!(pending["operationStatus"], "preference_set_reboot_needed");
        assert_eq!(
            annotate(
                "Enable-KernelDMAProtection",
                Scope::Machine,
                json!({"actuallyActive":true})
            )["status"],
            "applied"
        );
        assert_eq!(
            annotate(
                "Set-ServicesManual",
                Scope::Machine,
                json!({"status":"done","manual":{"failed":["example"]},"disable":{"failed":[]}})
            )["status"],
            "failed"
        );
    }
}
