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

    // The concrete tab type can be supplied as a generic; the persisted key
    // and default are the behavioural contract.
    expect(panel).toMatch(/useCleanupSessionState(?:<[^>]+>)?\("cleanup\.active-tab", "low-impact"\)/);
    expect(panel).toMatch(/<Tabs value=\{activeTab\} onValueChange=\{\(value\) => setActiveTab\(/);
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
  // map — it lives in OsRepairCard and is embedded in the unified one-time
  // action card, so the seven actions remain available without duplication.
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
    expect(panel).toContain("<OsRepairCard embedded />");
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

  test("clean compact cards retain their supported auto-clean control", async () => {
    const traceCard = await read("src/components/cleanup/CleanupTraceCard.tsx");

    const compactCleanStart = traceCard.indexOf("{compact ? (");
    const compactCleanEnd = traceCard.indexOf(") : (", compactCleanStart);
    const compactCleanBranch = traceCard.slice(compactCleanStart, compactCleanEnd);

    expect(compactCleanBranch).toContain("{scheduleControl}");
    expect(traceCard).toContain("showScheduler && !isActionOnly");
  });

  test("the cleanup toolbar exposes only global Scan All and Clean All actions", async () => {
    const scan = await read("src/panels/cleanup/useCleanupScan.ts");
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(scan).toContain("_scanningBatchIds");
    expect(scan).toContain("getCleanupScanBatchId");
    expect(scan).toContain("isCategoryBatchScanning");
    expect(panel).toContain("const allScanCategories = [...orderedScanCategories, ...VIEW_ONLY_CATEGORIES];");
    expect(panel).toContain("handleClearAllCategories");
    expect(panel).not.toContain('text={isScanningAll ? "Scanning All..." : "Scan All"}');
    expect(panel).not.toContain('text="Clean All"');
    expect(panel).not.toContain('text={isScanningThisTab ? "Scanning section..." : "Scan section"}');
    const grid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");
    expect(grid).not.toContain('text="Clear Low-Impact"');
    expect(grid).not.toContain("text={`Clear ${tierMeta.label}`}");
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
    expect(applyHandler.indexOf("if (succeeded)") < applyHandler.indexOf("closeAndReturnFocus()")).toBe(true);
    expect(clearHandler).toContain("const succeeded = await onClearSchedule()");
    expect(clearHandler).toContain("if (succeeded)");
    expect(clearHandler.indexOf("if (succeeded)") < clearHandler.indexOf("closeAndReturnFocus()")).toBe(true);
    expect(scheduler).toContain("const closeAndReturnFocus = useCallback");
    expect(scheduler).toContain("window.requestAnimationFrame(() => triggerRef.current?.focus())");
    expect(scheduler).toContain("window.innerWidth - VIEWPORT_MARGIN * 2");
    expect(scheduler).toContain("maxHeight: `calc(100vh - ${VIEWPORT_MARGIN * 2}px)`");
  });

  test("scan failure, empty, loading, and populated cards remain visibly distinct", async () => {
    const traceCard = await read("src/components/cleanup/CleanupTraceCard.tsx");

    expect(traceCard).toContain('type BandState = "has" | "clean" | "scanning" | "error"');
    expect(traceCard).toMatch(/: loading[\s\S]{0,100}\? "scanning"[\s\S]{0,100}: error[\s\S]{0,100}\? "error"/);
    expect(traceCard).toContain('label: "Scan failed"');
    expect(traceCard).toContain('role="alert"');
    expect(traceCard).toContain('preview.length === 0');
    expect(traceCard).toContain('open details to inspect');
  });

  test("the free-space dialog distinguishes discovery errors and contains invocation failures", async () => {
    const driveWipe = await read("src/panels/cleanup/DriveWipeDialog.tsx");

    expect(driveWipe).toContain("const [loadError, setLoadError]");
    expect(driveWipe).toContain("Could not detect local drives.");
    expect(driveWipe).toContain("Retry drive detection");
    expect(driveWipe).toContain('role="alert"');
    expect(driveWipe).toMatch(/try \{[\s\S]*invokeUnallocatedSpaceErase[\s\S]*\} catch \(error\)/);
    expect(driveWipe).toContain("ariaLabel={`Select ${drive.letter}");
  });

  test("one-time actions and browser inventory collapse safely on narrow viewports", async () => {
    const actions = await read("src/panels/cleanup/CleanupActionsMonitoring.tsx");
    const legacyDialogs = await read("src/panels/cleanup/useCleanupLegacyDialogs.tsx");

    expect(actions).toContain("grid grid-cols-1 gap-2");
    expect(actions).toContain("grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3");
    expect(actions).toContain('aria-labelledby="cleanup-one-time-actions-heading"');
    expect(actions).toContain('aria-labelledby="cleanup-system-monitoring-heading"');
    expect(legacyDialogs).toContain("grid grid-cols-1 gap-3 pb-1.5 lg:grid-cols-2");
    expect(legacyDialogs).toContain("grid grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-2");
  });

  test("active bespoke viewers expose loading, error, empty, and category-specific actions", async () => {
    const legacyDialogs = await read("src/panels/cleanup/useCleanupLegacyDialogs.tsx");

    expect(legacyDialogs).toContain("const [browserFootprintsError, setBrowserFootprintsError]");
    expect(legacyDialogs).toContain("Auditing browser profiles…");
    expect(legacyDialogs).toContain("Browser audit failed:");
    expect(legacyDialogs).toContain('text="RETRY AUDIT"');
    expect(legacyDialogs).toContain("No browser profiles detected");
    expect(legacyDialogs).toContain("aria-label={`Forget Wi-Fi profile ${p.name}`}");
    expect(legacyDialogs).toContain("aria-label={`Clear ${b.browser} artifacts for ${b.profilePath}`}");
    expect(legacyDialogs).toContain("disabled={browserFootprints.length === 0 || !!localLoadingMap['browserFootprints']}");
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

  test("profile and all-users viewers preserve structured records", async () => {
    const backendTypes = await read("src/hooks/useBackend.ts");
    const scan = await read("src/panels/cleanup/useCleanupScan.ts");
    const panel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");
    const dialog = await read("src/components/shared/TraceDetailDialog.tsx");
    const cleanupScript = await read("src-tauri/commander-free/scripts/modules/privacy/cleanup.ps1");

    expect(backendTypes).toContain("records?: Array<Record<string, unknown>>");
    expect(scan).toContain("raw: c.records?.length ? { records: c.records } : undefined");
    expect(panel).toContain("rawData={otherUserDataMap[otherDetail.catId]?.raw}");
    expect(panel).toContain("rawData: userData?.raw");
    expect(dialog).toContain("buildTraceView(group.rawData, group.items)");
    expect(cleanupScript).toContain("$count = 0; $items = @(); $records = @()");
    expect(cleanupScript).toContain("records = @($records | Select-Object -First $previewMax)");
  });

  test("multi-user delegation reads WAL and Recall result collections from their real fields", async () => {
    const cleanupScript = await read("src-tauri/commander-free/scripts/modules/privacy/cleanup.ps1");
    const delegationComment = cleanupScript.indexOf("# For the current user delegate");
    const delegatedStart = cleanupScript.indexOf("if ($isCurrentUser)", delegationComment);
    const otherUsersStart = cleanupScript.indexOf("# Other-users path", delegatedStart);
    const delegated = cleanupScript.slice(delegatedStart, otherUsersStart);
    const walStart = delegated.indexOf("'walFiles'");
    const recallStart = delegated.indexOf("'recallDb'", walStart);
    const webCacheStart = delegated.indexOf("'webCache'", recallStart);
    const walBlock = delegated.slice(walStart, recallStart);
    const recallBlock = delegated.slice(recallStart, webCacheStart);

    expect(walBlock).toContain("Get-SQLiteWALList");
    expect(walBlock).toContain("$records = @($r.files)");
    expect(walBlock).not.toContain("$r.databases");
    expect(recallBlock).toContain("Get-RecallDatabaseInfo");
    expect(recallBlock).toContain("$records = @($r.databases)");
    expect(recallBlock).not.toContain("$r.files");
  });
});
