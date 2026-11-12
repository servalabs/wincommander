import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  DESTRUCT_STEPS,
  DESTRUCT_STEP_DESCRIPTIONS,
} from "./lockdownSteps";

const TRACE_CLEANUP_COMMANDS = {
  trace_application_logs: "ApplicationTraceLogs",
  trace_system_temp_files: "SystemTempFiles",
  trace_all_user_certificates: "AllUserCertificates",
  trace_all_scheduled_tasks: "AllScheduledTasks",
  trace_all_services: "AllServices",
  trace_all_wireless_profiles: "AllWirelessProfiles",
  trace_all_vpn_connections: "AllVpnConnections",
  trace_all_chrome_extensions: "AllChromeExtensions",
  trace_all_crypto_provider_data: "AllCryptoProviderData",
  trace_windows_update_log: "WindowsUpdateLog",
  trace_push_notifications: "PushNotifications",
} as const;

test("lockdown progress labels match their native event labels", () => {
  const native = readFileSync("src-tauri/commander-free/src/action_steps.rs", "utf8").replace(/\r\n/g, "\n");
  for (const step of DESTRUCT_STEPS) {
    if (step.id === "remove_schedules") continue; // This step is driven by the frontend.
    const escapedId = step.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const nativeStep = native.match(new RegExp(`id: "${escapedId}",\\s*label: "([^"]+)"`));
    expect(nativeStep?.[1]).toBe(step.label);
  }
});

describe("system hygiene full-scope advanced cleanup steps", () => {
  test("remain opt-in Deep Trace actions with an operator-facing impact explanation", () => {
    for (const [id, commandSuffix] of Object.entries(TRACE_CLEANUP_COMMANDS)) {
      const step = DESTRUCT_STEPS.find((candidate) => candidate.id === id);
      expect(step).toBeTruthy();
      expect(step?.group).toBe("deepDfir");
      expect(step?.defaultEnabled).toBe(false);
      expect(step?.command).toBe(`Clear-${commandSuffix}`);

      const description = DESTRUCT_STEP_DESCRIPTIONS[id];
      expect(description).toBeTruthy();
      expect(description.length).toBeGreaterThan(70);
    }
  });
});
