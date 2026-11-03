export type InactivityState = "disabled" | "triggered" | "watchdog_stopped" | "settings_unavailable" | "invalid_baseline" | "lockdown_disabled" | "starting" | "check_failed" | "monitoring";

export function inactivityPresentation(value: unknown): { label: string; message: string } {
  switch (value) {
    case "disabled": return { label: "Off", message: "The inactivity timer is off." };
    case "triggered": return { label: "Triggered", message: "The timer recorded a trigger attempt. This does not confirm that Lockdown completed." };
    case "watchdog_stopped": return { label: "Unavailable", message: "The local watchdog is not running. Open or restart WinCommander to check it." };
    case "settings_unavailable": return { label: "Unavailable", message: "Personal automation settings are unavailable, so the watchdog cannot evaluate this timer." };
    case "invalid_baseline": return { label: "Unavailable", message: "The last activity time is invalid or ahead of the device clock. Check the timer before relying on it." };
    case "lockdown_disabled": return { label: "Lockdown off", message: "Self-destruct is not enabled. The timer can still record a trigger, but Lockdown will refuse to run." };
    case "starting": return { label: "Starting", message: "The watchdog is running and has not yet completed its first check." };
    case "check_failed": return { label: "Check failed", message: "The latest watchdog check failed. The next check may retry." };
    case "monitoring": return { label: "Running", message: "The watchdog is running; its latest iteration returned without an error." };
    default: return { label: "Unknown", message: "The local timer status could not be confirmed." };
  }
}
