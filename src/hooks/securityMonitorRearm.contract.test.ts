import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = decodeURIComponent(new URL("../../", import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, "$1");
const source = (path: string) => readFileSync(`${root}/${path}`, "utf8");

describe("security monitor startup contracts", () => {
  test("ransomware applies policy and folders before arming", async () => {
    const hook = source("src/hooks/useRansomwareMonitor.ts");
    const config = hook.indexOf('await invoke("set_ransomware_config"');
    const folders = hook.indexOf('await invoke("set_ransomware_watch_dirs"');
    const arm = hook.indexOf('await invoke(enabled ? "start_ransomware_monitor"');
    expect(config).toBeGreaterThan(-1);
    expect(folders).toBeGreaterThan(config);
    expect(arm).toBeGreaterThan(folders);
    expect(hook).toContain("attempt < 3");
    expect(hook).toContain('action: hasPaid ? action : "monitor"');
  });

  test("decoy rearm forwards one full persisted configuration", async () => {
    const hook = source("src/hooks/useDecoyMonitor.ts");
    expect(hook).toContain('await invoke("start_decoy_monitor", {');
    expect(hook).toContain("paths: enrolledPaths");
    expect(hook).toContain("readAuditEnabled");
    expect(hook).toContain("fleetAlertEnabled");
    expect(hook).not.toContain('invoke("list_decoys")');
    expect(hook).not.toContain('invoke("enroll_decoy")');
    expect(hook).toContain("lastReconciled.current = fingerprint");
    expect(hook).toContain("attempt < MAX_REARM_ATTEMPTS");
  });

  test("USB basic rearm is Free and paid guards clean up after expiry", async () => {
    const app = source("src/App.tsx");
    const blockStart = app.indexOf("// Free owns only the basic attach timeline");
    const blockEnd = app.indexOf("// Wi-Fi Guard retains", blockStart);
    const block = app.slice(blockStart, blockEnd);
    expect(block.indexOf('await invoke(basicMonitorDesired ? "start_usb_monitor"')).toBeGreaterThan(-1);
    expect(block.indexOf('"reconcile_usb_guard"')).toBeGreaterThan(
      block.indexOf('await invoke(basicMonitorDesired ? "start_usb_monitor"'),
    );
    expect(block).toContain("Pro's ProgramData state is canonical");
    expect(block).toContain('invoke<{ installed?: boolean }>("get_pro_install_status")');
    expect(block).toContain('await invoke("stop_usb_autosandbox")');
    expect(block).toContain('await invoke("stop_usb_metering")');
    expect(block).toContain('await invoke("stop_usb_hid_guard")');
    expect(block).toContain('await invoke("stop_usb_hid_approval_gate")');
    expect(block).toContain("hidApprovalGateEnabled: usbHidApprovalGateEnabled");
    expect(block).toContain("hidApprovalTtlSecs: usbHidApprovalTtlSecs");
    expect(block).toContain("if (!usbSecurityConfigured && (entitlementLoading || hasPaid)) return");
  });

  test("expired paid settings cannot re-arm the decoy watcher", async () => {
    const app = source("src/App.tsx");
    expect(app).toContain("useDecoyMonitor(hasPaid && decoyEnabled");
    const hook = source("src/hooks/useDecoyMonitor.ts");
    expect(hook).toContain("if (entitlementLoading || mode === \"decoy\") return");
    expect(hook).toContain('await invoke("stop_decoy_monitor")');
  });

  test("saved screen-capture detection is reconciled at app startup", async () => {
    const app = source("src/App.tsx");
    expect(app).toContain('? "start_screen_capture_watch"');
    expect(app).toContain("Screen-capture detection could not re-arm");
  });

  test("startup readiness is driven by enabled rearm results, never cache hydration", () => {
    const app = source("src/App.tsx");
    expect(app).toContain("createStartupProtectionReadiness(reportStartupPhase)");
    expect(app).toContain("startupProtectionConfiguredRef.current = true");
    expect(app).toContain("configure(configuredStartupProtectionOperations)");
    for (const operation of [
      "decoy-monitor",
      "ransomware-monitor",
      "remote-access-monitor",
      "usb-security",
      "wifi-guard",
      "auth-anomaly-monitor",
      "screen-capture-watch",
    ]) {
      expect(app).toContain(`\"${operation}\" as const`);
    }
    expect(app).toContain('reportStartupProtectionRearm("usb-security", true)');
    expect(app).toContain('reportStartupProtectionRearm("screen-capture-watch", true)');
    expect(source("src/hooks/useRansomwareMonitor.ts")).toContain('onStartupRearm?.("ransomware-monitor", true)');
    expect(source("src/hooks/useDecoyMonitor.ts")).toContain('onStartupRearm?.("decoy-monitor", true)');
    expect(source("src/hooks/useRemoteAccessMonitor.ts")).toContain('onStartupRearm?.("remote-access-monitor", true)');
    expect(source("src/hooks/useWifiGuardMonitor.ts")).toContain('onStartupRearm?.("wifi-guard", true)');
    expect(source("src/hooks/useAuthAnomalyMonitor.ts")).toContain('onStartupRearm?.("auth-anomaly-monitor", true)');
  });
});
