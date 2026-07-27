import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

const read = (path: string) => Bun.file(path).text();

describe("System Cleanup panel reconstruction contracts", () => {
  test("the cleanup route delegates to the reconstructed panel", async () => {
    const route = await read("src/panels/cleanup/index.tsx");

    expect(route).toContain('from "./SystemCleanupPanel"');
    expect(route).not.toContain("useCleanupScan(");
    expect(route).not.toContain("CleanupCategoryGrid");
  });

  test("the reconstructed panel owns the cleanup viewport scroller", async () => {
    const appShell = await read("src/components/AppShell.tsx");
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(appShell).toMatch(/isViewportBoundPanel[\s\S]*activePanel\s*===\s*["']cleanup["']/);
    expect(panel).toContain('data-cleanup-scroll-root="true"');
    expect(panel).toMatch(/data-cleanup-scroll-root="true"[\s\S]{0,400}overflow-y-auto/);
  });

  test("the panel never fights user scroll through imperative correction", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");
    const scheduler = await read("src/components/cleanup/CleanupScheduleControl.tsx");
    const traceCard = await read("src/components/cleanup/CleanupTraceCard.tsx");
    const ownedSources = `${panel}\n${scheduler}\n${traceCard}`;

    expect(ownedSources).not.toContain("scrollIntoView(");
    expect(ownedSources).not.toContain(".scrollTo(");
    expect(ownedSources).not.toContain("panel-scroll-top");
    expect(ownedSources).not.toContain("data-scroll-locked");
    expect(ownedSources).not.toContain("document.body.style");
    expect(ownedSources).not.toContain("document.body.setAttribute");
    expect(panel).toContain("overflowAnchor: 'none'");
  });

  // The System Cleanup tab split (2026-07) is a viewport-bound, self-scrolling
  // panel (AppShell's isViewportBoundPanel) — every tab roughly fits one
  // viewport on its own, so tab-targeting is a plain setActiveTab, never
  // scrollIntoView/.scrollTo. This covers the two files the split newly
  // introduced tab content into, which the scroll-discipline test above
  // (fixed to panel/scheduler/traceCard) doesn't reach.
  test("the tab split introduces no scrollIntoView-based deep-linking", async () => {
    const grid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");
    const actionsMonitoring = await read("src/panels/cleanup/CleanupActionsMonitoring.tsx");
    const sessionState = await read("src/panels/cleanup/cleanupSessionState.ts");
    const sources = `${grid}\n${actionsMonitoring}\n${sessionState}`;

    expect(sources).not.toContain("scrollIntoView(");
    expect(sources).not.toContain(".scrollTo(");
  });

  test("card information surfaces cannot create a hover-focus scroll loop", async () => {
    const traceCard = await read("src/components/cleanup/CleanupTraceCard.tsx");
    const infoStart = traceCard.indexOf("const infoTooltip");
    const scheduleStart = traceCard.indexOf("const scheduleControl", infoStart);
    const infoSurface = traceCard.slice(infoStart, scheduleStart);

    expect(infoSurface).toContain("<Tooltip>");
    expect(infoSurface).toContain("<TooltipTrigger asChild>");
    expect(infoSurface).not.toContain("<Popover");
    expect(infoSurface).not.toContain("onMouseEnter");
    expect(infoSurface).not.toContain("onMouseLeave");
    expect(infoSurface).not.toContain("onFocus");
    expect(infoSurface).not.toContain("onBlur");
    expect(traceCard).not.toContain("infoOpen");
  });

  test("the reconstruction preserves scan, clear, detail, and one-time action wiring", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(panel).toContain("useCleanupScan({");
    expect(panel).toContain("<CleanupCategoryGrid");
    expect(panel).toContain("<CleanupActionsMonitoring");
    expect(panel).toContain("handleCardClear");
    expect(panel).toContain("handleCardLoad");
    expect(panel).toContain("handleCardClearAllUsers");
    expect(panel).toContain("<TraceDetailDialog");
    expect(panel).toContain("<DriveWipeDialog");
  });

  // Five tabs (2026-07 split), Low impact first/default — no Overview tab.
  test("the panel builds five tabs with Low impact as the default", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(panel).toContain('useCleanupSessionState("cleanup.active-tab", "low-impact")');
    expect(panel).toContain("<Tabs value={activeTab} onValueChange={setActiveTab}>");
    const triggerOrder = ['value="low-impact"', 'value="history-cache"', 'value="rebuilds-apps-connectivity"', 'value="data-accounts-recovery"', 'value="actions-monitoring"'];
    let cursor = -1;
    for (const trigger of triggerOrder) {
      const at = panel.indexOf(trigger, cursor + 1);
      expect(at).toBeGreaterThan(cursor);
      cursor = at;
    }
    expect(panel).not.toContain('value="overview"');
  });

  // useCleanupScan's module-level cache and useCleanupLegacyDialogs' 28
  // dialogs both assume exactly one subscriber — splitting cards across tabs
  // must not spawn a second instantiation inside the per-tier component.
  test("useCleanupScan and useCleanupLegacyDialogs stay single top-level instantiations across the tab split", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");
    const grid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");

    const scanCalls = (panel.match(/useCleanupScan\(/g) ?? []).length;
    const dialogCalls = (panel.match(/useCleanupLegacyDialogs\(/g) ?? []).length;
    expect(scanCalls).toBe(1);
    expect(dialogCalls).toBe(1);
    expect(grid).not.toContain("useCleanupScan(");
    expect(grid).not.toContain("useCleanupLegacyDialogs(");
    expect(grid).toContain("scan: ReturnType<typeof useCleanupScan>");
  });

  // The investigator review banner and the scheduler's portal-target
  // ancestors must stay visible/discoverable regardless of which tab is
  // active — they must sit outside every TabsContent, not duplicated inside one.
  test("the investigator banner and root data attributes wrap the whole tab tree", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    const bannerIndex = panel.indexOf("REVIEW MODE");
    const tabsIndex = panel.indexOf("<Tabs value={activeTab}");
    expect(bannerIndex).toBeGreaterThan(-1);
    expect(tabsIndex).toBeGreaterThan(bannerIndex);

    const rootIndex = panel.indexOf('data-cleanup-panel-root="true"');
    const overlayIndex = panel.indexOf('data-cleanup-overlay-root="true"');
    expect(rootIndex).toBeGreaterThan(-1);
    expect(tabsIndex).toBeGreaterThan(rootIndex);
    expect(overlayIndex).toBeGreaterThan(tabsIndex);
  });

  // Force SSD TRIM is not one of ACTION_CATEGORIES/useCleanupScan's dispatch
  // map — it lives in OsRepairCard, which SystemCleanupPanel now renders
  // directly inside this tab (Maintenance's "Repair & hygiene" tab that used
  // to host it was removed, 2026-07).
  test("Force SSD TRIM is not duplicated in System Cleanup's one-time actions", async () => {
    const categories = await read("src/panels/cleanup/cleanupCategories.ts");
    const actionsMonitoring = await read("src/panels/cleanup/CleanupActionsMonitoring.tsx");
    const scan = await read("src/panels/cleanup/useCleanupScan.ts");
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(categories).not.toContain("id: 'ssdTrim'");
    expect(categories).not.toContain("invokeSSDTrim");
    expect(actionsMonitoring).toContain("Force SSD TRIM lives in OsRepairCard");
    expect(scan).not.toContain("ssdTrim: invokeSSDTrim");
    expect(scan).not.toContain("invokeSSDTrim,");
    expect(panel).toContain('from "../maintenance/OsRepairCard"');
    expect(panel).toContain("<OsRepairCard />");
  });

  // Each of the four usability tiers renders through the same component,
  // parameterized by tier — not four forked copies of the grid.
  test("each usability tier renders in its own tab via a tier prop", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");
    const grid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");

    expect(grid).toContain("tier: CleanupUsabilityTier");
    for (const tier of ["low-impact", "history-cache", "rebuilds-apps-connectivity", "data-accounts-recovery"]) {
      expect(panel).toContain(`tier="${tier}"`);
    }
  });

  test("paid scheduling remains gated until entitlement and Pro installation resolve", async () => {
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(panel).toContain("entitlementsReady: !entitlementsLoading");
    expect(panel).toContain("migrationEnabled: hasPaid && proInstalled && !isInvestigator");
    expect(panel).toContain("schedulesEnabled={hasPaid && !isInvestigator}");
    expect(panel).toContain("onRequestScheduleAccess");
  });

  test("the scheduler is a stable non-Radix control with runtime test hooks", async () => {
    const scheduler = await read("src/components/cleanup/CleanupScheduleControl.tsx");

    expect(scheduler).not.toContain("@radix-ui");
    expect(/\bPopover\b/.test(scheduler)).toBe(false);
    expect(scheduler).toContain('data-cleanup-schedule-control="true"');
    expect(scheduler).toContain('data-cleanup-schedule-menu="true"');
    expect(scheduler).toContain("aria-expanded={isOpen}");
    expect(scheduler).toContain("onSetSchedule");
    expect(scheduler).toContain("onClearSchedule");
  });

  test("scheduler mutations close only after the backend reports success", async () => {
    const scheduler = await read("src/components/cleanup/CleanupScheduleControl.tsx");
    const applyStart = scheduler.indexOf("const applySchedule");
    const clearStart = scheduler.indexOf("const clearSchedule");
    const renderStart = scheduler.indexOf("return (", clearStart);
    const applyHandler = scheduler.slice(applyStart, clearStart);
    const clearHandler = scheduler.slice(clearStart, renderStart);

    expect(applyHandler).toContain("const succeeded = await onSetSchedule(");
    expect(applyHandler).toContain("if (succeeded)");
    expect(applyHandler.indexOf("if (succeeded)") < applyHandler.indexOf("setIsOpen(false)")).toBe(true);
    expect(clearHandler).toContain("const succeeded = await onClearSchedule()");
    expect(clearHandler).toContain("if (succeeded)");
    expect(clearHandler.indexOf("if (succeeded)") < clearHandler.indexOf("setIsOpen(false)")).toBe(true);
  });

  test("every deep cleanup card is connected to scan, clear, and details", async () => {
    const scan = await read("src/panels/cleanup/useCleanupScan.ts");
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");
    const deepIds = [
      "webCache", "thumbnailDb", "notificationDb", "branchCache",
      "eventTranscript", "activitiesTimeline", "rdpBitmapCache",
      "servicingLogs", "deviceInstallLogs", "usageTraceLogs", "defenderHistory",
    ];

    for (const id of deepIds) {
      expect(scan).toContain(`${id}: get`);
      expect(scan).toContain(`${id}: clear`);
      expect(panel).toContain(`${id}: () => openSharedDetails('${id}')`);
    }
  });

  test("one-card profile scans cannot overwrite unrelated cached cards", async () => {
    const scan = await read("src/panels/cleanup/useCleanupScan.ts");

    expect(scan).toContain("const requestedIds = categoryIds?.length ? new Set(categoryIds) : null;");
    expect(scan).toContain("if (requestedIds && !requestedIds.has(cat.id)) continue;");
    expect(scan).toContain("MULTI_USER_CLEANUP_IDS.has(cat.id)");
    expect(scan).toContain("if (s.targetUser) continue;");
  });
});
