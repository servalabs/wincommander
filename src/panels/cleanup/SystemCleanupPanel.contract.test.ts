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
