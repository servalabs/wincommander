import { describe, expect, test } from "bun:test";
import { getAutoPollRefreshKey, getDashboardPanelRefreshKeys, shouldAutoPollPanel } from "./useActivePanelPoller";

describe("active panel polling policy", () => {
  test("keeps heavy panels out of automatic polling", () => {
    expect(shouldAutoPollPanel("dashboard", "refreshDashboard")).toBe(true);
    expect(shouldAutoPollPanel("network", "refreshNetwork")).toBe(true);

    expect(shouldAutoPollPanel("apps", undefined)).toBe(false);
    expect(shouldAutoPollPanel("cleanup", undefined)).toBe(false);
    expect(shouldAutoPollPanel("vault", "refreshVault")).toBe(true);
    expect(getAutoPollRefreshKey("vault")).toBe("refreshVault");
  });

  test("dashboard only auto-polls dashboard-owned refresh work", () => {
    expect(getAutoPollRefreshKey("dashboard")).toBe("refreshDashboard");
    expect(getDashboardPanelRefreshKeys()).toEqual(["refreshDashboard"]);
    expect(getDashboardPanelRefreshKeys()).not.toContain("refreshPrivacy");
    expect(getDashboardPanelRefreshKeys()).not.toContain("refreshNetwork");
  });

});
