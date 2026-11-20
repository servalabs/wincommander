import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const source = (path: string) => readFileSync(`${root}/${path}`, "utf8").replace(/\r\n/g, "\n");

// A Fleet command changes the device from outside the UI. These contracts keep
// the event-driven refresh wired end to end; the polls remain only as fallback.
describe("device-state refresh wiring", () => {
  test("the app mounts the bridge that listens for native settings and Pro device-state events", () => {
    expect(source("App.tsx")).toContain("<DeviceStateBridge />");
    const bridge = source("components/DeviceStateBridge.tsx");
    expect(bridge).toContain("SETTINGS_CHANGED_EVENT");
    expect(bridge).toContain("DEVICE_STATE_CHANGED_EVENT");
    expect(bridge).toContain('mode === "decoy"');
  });

  test("the bridge re-reads settings and never writes or displays event data", () => {
    const bridge = source("components/DeviceStateBridge.tsx");
    expect(bridge).toContain("refreshSettingsRef.current()");
    expect(bridge).toContain("invalidateQueries");
    expect(bridge).not.toMatch(/patch|setAppSettings|event\.payload/);
  });

  test("the active-panel poller refreshes on a device change and keeps its interval fallback", () => {
    const poller = source("hooks/useActivePanelPoller.ts");
    expect(poller).toContain("onDeviceStateRefresh");
    expect(poller).toContain("refreshNetwork(true, true)");
    expect(poller).toContain("PANEL_POLL_INTERVAL");
  });

  test("monitor status readers re-read on a device change in addition to polling", () => {
    for (const file of [
      "panels/privacy/ArgusDlpSection.tsx",
      "panels/privacy/ArgusAppUsageSection.tsx",
      "panels/privacy/ArgusPrintUsbSection.tsx",
      "panels/privacy/ArgusTamperSection.tsx",
      "panels/privacy/RansomwareMonitorSection.tsx",
      "panels/privacy/AuthAnomalySection.tsx",
      "panels/privacy/DecoyMonitorSection.tsx",
      "panels/privacy/CanaryTokensSection.tsx",
      "panels/network/WifiGuardSection.tsx",
      "hooks/usePortGuard.ts",
    ]) {
      expect(source(file), file).toContain("useDeviceStateRefresh(");
    }
  });
});
