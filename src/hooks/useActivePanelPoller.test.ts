import { describe, expect, test } from "bun:test";
import { getAutoPollRefreshKey, getDashboardPanelRefreshKeys, runRefreshIfIdle, shouldAutoPollPanel } from "./useActivePanelPoller";

describe("active panel polling policy", () => {
  test("keeps heavy panels out of automatic polling", () => {
    expect(shouldAutoPollPanel("dashboard", "refreshDashboard")).toBe(true);
    expect(shouldAutoPollPanel("network", "refreshNetwork")).toBe(true);

    expect(shouldAutoPollPanel("apps", undefined)).toBe(false);
    expect(shouldAutoPollPanel("cleanup", undefined)).toBe(false);
    expect(shouldAutoPollPanel("vault", "refreshVault")).toBe(false);
  });

  test("dashboard only auto-polls dashboard-owned refresh work", () => {
    expect(getAutoPollRefreshKey("dashboard")).toBe("refreshDashboard");
    expect(getDashboardPanelRefreshKeys()).toEqual(["refreshDashboard"]);
    expect(getDashboardPanelRefreshKeys()).not.toContain("refreshPrivacy");
    expect(getDashboardPanelRefreshKeys()).not.toContain("refreshNetwork");
  });

  test("skips refresh ticks while the previous refresh is still running", async () => {
    const inFlight = { current: false };
    let releaseFirst!: () => void;
    const firstGate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let calls = 0;

    const first = runRefreshIfIdle(inFlight, async () => {
      calls += 1;
      await firstGate;
    });
    const second = await runRefreshIfIdle(inFlight, async () => {
      calls += 1;
    });

    expect(second).toBe("skipped");
    expect(calls).toBe(1);

    releaseFirst();
    await expect(first).resolves.toBe("ran");
    expect(inFlight.current).toBe(false);
  });
});
