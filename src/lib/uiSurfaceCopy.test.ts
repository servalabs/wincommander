import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

function read(path: string): Promise<string> {
  return Bun.file(path).text();
}

describe("redesign surface copy guardrails", () => {
  test("title bar uses health wording instead of armed/sovereignty chrome", async () => {
    const source = await read("src/components/TitleBar.tsx");

    expect(/\/\/ ARMED|["'`]ARMED["'`]|["'`]SOVEREIGNTY["'`]/.test(source)).toBe(false);
  });

  test("privacy monitoring cards do not expose monitor labels", async () => {
    const source = (await Promise.all([
      "src/panels/privacy/PasteMonitorSection.tsx",
      "src/panels/privacy/DecoyMonitorSection.tsx",
      "src/panels/privacy/RansomwareMonitorSection.tsx",
      "src/panels/privacy/RemoteAccessMonitorSection.tsx",
      "src/panels/privacy/MonitorIntros.tsx",
    ].map(read))).join("\n");

    expect(/\b(Paste|Decoy|Ransomware|Remote-Access|Anti-Ransomware)\s+Monitor\b/.test(source)).toBe(false);
  });

  test("File search launches from the right sidebar, with Ctrl+Space overlay preserved", async () => {
    // Decision 2026-06-09 (owner): Search Files moved off the left rail to a
    // right-sidebar launcher. The inline EverythingSearchBar stays out of the
    // main shell and the Ctrl+Space overlay window is preserved.
    expect(await read("src/App.tsx")).not.toContain("<EverythingSearchBar");
    expect(await read("src/main.tsx")).toContain('windowLabel === "search-overlay"');
    expect(await read("src/main.tsx")).toContain("<EverythingSearchBar overlayMode");
    expect(await read("src-tauri/commander-free/src/lib.rs")).toContain('on_shortcut("Ctrl+Space"');
    expect(await read("src-tauri/commander-free/src/lib.rs")).toContain("handle_search_hotkey(app)");
    // The right sidebar now hosts the search launcher (navigates to the panel).
    expect(await read("src/components/RightSidebar.tsx")).toContain('"search-files"');
  });

  test("AI Advisor is a right-sidebar launcher panel, not a settings shortcut", async () => {
    const panelsTs = await read("src/types/panels.ts");
    expect(panelsTs).toContain('id: "advisor"');
    // Decision 2026-06-09 (owner): AI Advisor is hidden from the left rail via
    // navTier: "hidden" in the manifest (the previous per-component filter in
    // identity/index.tsx was removed when the manifest resolver took ownership).
    expect(panelsTs).toContain('navTier: "hidden"');
    // Decision 2026-06-09 (owner): AI Advisor + Search are right-sidebar launchers.
    expect(await read("src/components/RightSidebar.tsx")).toContain('"advisor"');
    expect(await read("src/components/RightSidebar.tsx")).toContain('"search-files"');
  });

  test("Secure Storage shows system-drive status and action buttons above mounted volumes", async () => {
    const source = await read("src/panels/vault/index.tsx");

    // Encrypted Volumes moved onto the shared SectionCard (2026-07); its
    // title prop is the header, not a bespoke "vault-card-new-header" div.
    const headerIndex = source.indexOf('title="Encrypted Volumes"');
    const systemStatusIndex = source.indexOf("<SystemEncryptionSection compact />");
    const mountedVolumesIndex = source.indexOf('className={`vault-content ${volumes.length === 0 ? "vault-content--empty" : ""}`}');
    const actionRowIndex = source.indexOf('className="vault-card-action-row"');

    expect(headerIndex).toBeGreaterThanOrEqual(0);
    expect(headerIndex < systemStatusIndex).toBe(true);
    expect(systemStatusIndex < actionRowIndex).toBe(true);
    expect(actionRowIndex < mountedVolumesIndex).toBe(true);
  });

  test("System Maintenance owns Disk Cleanup and application caches, not Secure Storage or Packages & Apps", async () => {
    const vault = await read("src/panels/vault/index.tsx");
    const maintenance = await read("src/panels/maintenance/index.tsx");
    const reclaim = await read("src/panels/maintenance/ReclaimSpaceCard.tsx");
    const apps = await read("src/panels/apps/index.tsx");

    expect(vault).not.toContain("DiskCleanupGranular");
    expect(vault).not.toContain("DiskSpaceAnalyzerDialog");
    expect(maintenance).toContain("<DiskSpaceAnalyzerDialog inline");
    expect(apps).not.toContain("RoutineCleanerPanel");
    expect(maintenance).not.toContain('TabsTrigger value="updates"');

    // Both engines render side by side in one grid (2026-07): they used to
    // clean the same nine Windows paths through two different backends, so
    // ReclaimSpaceCard keeps them reading from one deduped category source
    // instead of independently re-scanning the same paths twice.
    expect(reclaim).toContain("<DiskCleanupGranular />");
    expect(reclaim).toContain("APP_CACHE_CLEANUP_CATEGORIES");
    expect(maintenance).not.toContain("<DiskCleanupGranular />");
  });

  test("Secure Storage shows RAM-disk action buttons above active disks", async () => {
    const source = await read("src/panels/vault/RamDisksSection.tsx");

    // RAM Disks moved onto the shared SectionCard (2026-07). `title="RAM Disks"`
    // appears twice (the early-return "engine not installed" card and the real
    // one) so `headerRight={` — unique to the real card's header — is the marker.
    const headerIndex = source.indexOf("headerRight={");
    // Class gained a second token "ramdisk-create-row" when the autostart widget
    // was added to the same row (2026-06). Match by startsWith to stay stable.
    const actionRowIndex = source.indexOf('className="vault-card-action-row ramdisk-create-row"');
    const activeDisksIndex = source.indexOf('className={`vault-content ${disks.length === 0 ? "vault-content--empty" : ""}`}');

    expect(headerIndex).toBeGreaterThanOrEqual(0);
    expect(headerIndex < actionRowIndex).toBe(true);
    expect(actionRowIndex < activeDisksIndex).toBe(true);
  });

  test("dashboard radar stays visible when ambient motion is disabled", async () => {
    const css = await read("src/components/dashboard/SovereigntyRadar.css");

    expect(css).toContain("html.wc-no-motion .sov-radar__scope");
    expect(css).toContain("html.wc-no-motion .sov-radar__node");
    expect(css).toContain("opacity: 1 !important");
    expect(css).toContain("transform: translate(-50%, -50%) scale(1)");
  });

  test("ambient motion disabled never sets the global motion scalar to zero", async () => {
    // The motion class logic now lives in the motionPolicy SSOT; AppShell just
    // delegates to it. Guardrail intent is unchanged: --motion is never zeroed
    // (several CSS rules divide by it), wc-no-motion is the real off switch, and
    // wc-motion-enabled only turns on for an explicit user opt-in.
    const policy = await read("src/lib/motionPolicy.ts");
    const shell = await read("src/components/AppShell.tsx");
    const globalCss = await read("src/index.css");

    expect(policy).not.toContain('setProperty("--motion", "0")');
    expect(policy).toContain('setProperty("--motion", "1")');
    expect(policy).toContain('classList.toggle("wc-no-motion"');
    expect(policy).toContain('classList.toggle("wc-motion-enabled", pref === "1")');
    expect(shell).toContain("applyMotionClass()");
    expect(globalCss).toContain("html.wc-no-motion *,");
    expect(globalCss).toContain("animation: none !important");
  });

  test("explicit app motion-on overrides OS reduced-motion media rules", async () => {
    const hook = await read("src/hooks/useMotionPreference.ts");
    const globalCss = await read("src/index.css");
    const radarCss = await read("src/components/dashboard/SovereigntyRadar.css");
    const themeCss = await read("src/styles/v2-theme.css");

    expect(hook).toContain('localStorage.getItem("wc-motion") === "1"');
    expect(hook).toContain("osWantsReduced() && !userExplicitlyEnabledMotion()");
    expect(globalCss).toContain("@media (prefers-reduced-motion: reduce)");
    expect(globalCss).toContain("html:not(.wc-motion-enabled) *:not(");
    expect(radarCss).toContain("html:not(.wc-motion-enabled) .sov-radar__sweep");
    expect(themeCss).toContain("html:not(.wc-motion-enabled) .wc-blink");
  });

  test("Blueprint compatibility triggers never pass Radix props to fragments", async () => {
    const source = await read("src/components/ui/bp.tsx");

    expect(source).toContain("function renderTriggerChild");
    expect(source).not.toContain('fill ? <span className="w-full">{children}</span> : <>{children}</>');
    expect(source).not.toContain(": <>{children}</>;");
  });

  test("app inventory timeout keeps the backend scan guard alive", async () => {
    const source = await read("src/context/AppContext.tsx");

    expect(source).toContain("appInventoryBackendInFlightRef");
    expect(source).toContain("waitForSoftTimeout");
    expect(source).toContain("still running after 45 seconds");
    expect(source).not.toContain("Promise.race([\n                    getAppInventory()");
  });

  test("background listeners do not log on normal mount", async () => {
    const source = await read("src/components/BackgroundPollers.tsx");

    expect(source).not.toContain("[BackgroundPollers] mounted;");
  });

  test("network speed listener stays app-level instead of dashboard-only", async () => {
    const app = await read("src/App.tsx");
    const card = await read("src/components/dashboard/NetworkTrafficCard.tsx");

    expect(app).toContain("useNetworkTrafficListener();");
    expect(card).toContain("useNetworkTraffic()");
    expect(card).not.toContain('listen<NetSample>("metrics://network"');
    expect(card).not.toContain('listen<MetricAlertEvent>("metrics://metric-alert"');
  });

  test("dashboard keeps the whitelabeled wordmark mounted while Needs Attention expands", async () => {
    const dashboard = await read("src/panels/dashboard/index.tsx");

    expect(dashboard).toContain('<div className="dashboard-radar-watermark" aria-hidden="true">{branding.productLabel}</div>');
    expect(dashboard).not.toContain("{hideCenterChrome && <div className=\"dashboard-radar-watermark\"");
  });

  test("dashboard places Public IP and DNS below the Internet toggle", async () => {
    const dashboard = await read("src/panels/dashboard/index.tsx");
    const privacyToggles = await read("src/components/dashboard/PrivacyTogglesCard.tsx");

    expect(dashboard).not.toContain("RadarControlStrip");
    expect(privacyToggles).toContain('import RadarControlStrip from "./RadarControlStrip";');

    const internetButtonIndex = privacyToggles.indexOf('className={`killswitch-toggle');
    const radarStripIndex = privacyToggles.indexOf("<RadarControlStrip />");

    expect(internetButtonIndex > -1).toBe(true);
    expect(radarStripIndex > internetButtonIndex).toBe(true);
  });

  test("Privacy Settings does not render the Public IP and DNS strip", async () => {
    const privacyPanel = await read("src/panels/privacy/index.tsx");

    expect(privacyPanel).not.toContain("RadarControlStrip");
    expect(privacyPanel).not.toContain("Public IP + DNS readout");
  });

  test("light mode cards use contrasted surfaces and borders", async () => {
    const theme = await read("src/styles/v2-theme.css");
    const sectionCard = await read("src/components/shared/SectionCard.css");

    expect(theme).toContain("--surface: #fbfcfa;");
    expect(theme).toContain("--border-strong: rgba(28, 42, 34, 0.36);");
    expect(sectionCard).toContain("background-color: var(--surface) !important;");
    expect(sectionCard).toContain("border-color: var(--border-strong) !important;");
    expect(sectionCard).not.toContain("border-color: rgba(0, 0, 0, 0.1) !important;");
  });

  test("debloat is a Free Packages and Apps surface with local Appx commands", async () => {
    const appsPanel = await read("src/panels/apps/index.tsx");
    const backend = await read("src-tauri/commander-free/src/backend.rs");
    const uninstaller = await read("src-tauri/commander-free/scripts/modules/apps/uninstaller.ps1");

    expect(appsPanel).toContain("<DebloatPanel />");
    expect(appsPanel).not.toContain("showDebloat");
    expect(backend).toContain('"Remove-AppxByName" => Some("apps/uninstaller")');
    expect(backend).toContain('"Restore-AppxByName" => Some("apps/uninstaller")');
    expect(backend).toContain('"Set-AppxDeprovisioned" => Some("apps/uninstaller")');
    expect(uninstaller).toContain("function Remove-AppxByName");
    expect(uninstaller).toContain("function Restore-AppxByName");
    expect(uninstaller).toContain("function Set-AppxDeprovisioned");
    expect(uninstaller).not.toContain("fallback");
  });

  test("System Cleanup stacks four confirmed-clean cards in one readable column", async () => {
    const cleanupGrid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");

    // Confirmed-clean cards remain visible and use four readable rows so their
    // full titles and per-card controls do not get clipped.
    expect(cleanupGrid).toContain("const orderUnscannedFirst");
    expect(cleanupGrid).toContain("const activeSysCats = orderUnscannedFirst(");
    expect(cleanupGrid).toContain("const cleanCardPacks = packCleanCards(");
    expect(cleanupGrid).toContain("cleanCardPacks.map((pack, index) => (");
    expect(cleanupGrid).toContain('className="grid h-[168px] grid-cols-1 grid-rows-4 gap-1"');
    expect(cleanupGrid).toContain('className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4"');
    expect(cleanupGrid).not.toContain("cleanCardsOpen");
  });

  test("System Cleanup hides card rescan actions until a card has scan output", async () => {
    const traceCard = await read("src/components/cleanup/CleanupTraceCard.tsx");

    expect(traceCard).toContain("const hasScanOutput");
    expect(traceCard).toContain("!isActionOnly && onLoad && hasScanOutput ? (");
    expect(traceCard).not.toContain("disabled={loading || isNotLoaded}");
  });

  test("System Cleanup trace cards keep stable height across scan states", async () => {
    const traceCard = await read("src/components/cleanup/CleanupTraceCard.tsx");

    expect(traceCard).toContain('const TRACE_CARD_HEIGHT = "168px";');
    expect(traceCard).not.toContain("height: '96px'");
  });

  test("System Cleanup trace cards do not become scroll anchors during scan reflow", async () => {
    const css = await read("src/components/cleanup/CleanupTraceCard.css");

    expect(css).toContain("overflow-anchor: none");
    expect(css).not.toContain(".cleanup-panel {");
  });

  // Scan All belongs in the tab-navigation row and scans only the active
  // cleanup group. The summary stays directly below that row and remains
  // unconditional while results arrive.
  test("System Cleanup keeps the summary footprint stable while scan results arrive", async () => {
    const cleanupGrid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");
    const cleanupPanel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(cleanupGrid).toContain("{isLowImpactTab && (");
    expect(cleanupGrid).toContain('data-cleanup-summary="true"');
    expect(cleanupGrid).not.toContain('className="cleanup-scan-all-btn"');
    expect(cleanupPanel).toContain("function CleanupTabNavigation");
    expect(cleanupPanel).toContain("<CleanupTabNavigation activeTab={activeTab} scan={scan} />");
    expect(cleanupPanel).toContain("loadCategoryBatch(categories, \"standard\")");
    expect(cleanupPanel).toContain("<CleanupSummaryStats scan={scan} />");
    expect(cleanupGrid).not.toContain("{(summaryStats.needsCleaning > 0 || summaryStats.clean > 0) && (");
    expect(cleanupGrid).not.toContain("{summaryStats.needsCleaning > 0 && (");
    expect(cleanupGrid).not.toContain("{summaryStats.clean > 0 && (");
    expect(cleanupGrid).not.toContain("{loadingAll !== null && (");
  });

  test("System Cleanup scheduler uses a panel-owned non-modal surface", async () => {
    const scheduler = await read("src/components/cleanup/CleanupScheduleControl.tsx");

    expect(scheduler).toContain('data-cleanup-schedule-control="true"');
    expect(scheduler).toContain('data-cleanup-schedule-menu="true"');
    expect(scheduler).toContain('[data-cleanup-overlay-root="true"]');
    expect(scheduler).not.toContain("@radix-ui");
    expect(scheduler).not.toContain("scrollIntoView(");
    expect(scheduler).not.toContain("document.body.style");
  });

  test("System Cleanup never launches paid schedule migration before entitlement resolution", async () => {
    const cleanupPanel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");
    const cleanupScan = await read("src/panels/cleanup/useCleanupScan.ts");
    const cleanupGrid = await read("src/panels/cleanup/CleanupCategoryGrid.tsx");

    expect(cleanupPanel).toContain("entitlementsReady: !entitlementsLoading");
    expect(cleanupPanel).toContain("migrationEnabled: hasPaid && proInstalled && !isInvestigator");
    expect(cleanupPanel).toContain("schedulesEnabled={hasPaid && !isInvestigator}");
    expect(cleanupPanel).not.toContain('schedulesEnabled={canUse("paid")}');
    expect(cleanupScan).toContain("if (!entitlementsReady || !schedulesEnabled || !migrationEnabled || migrationStarted.current) return;");
    expect(cleanupGrid).toContain("onRequestScheduleAccess");
  });

  test("System Cleanup keeps the schedule editor open until the backend confirms a change", async () => {
    const scheduler = await read("src/components/cleanup/CleanupScheduleControl.tsx");
    const cleanupScan = await read("src/panels/cleanup/useCleanupScan.ts");

    expect(scheduler).toContain("const succeeded = await onSetSchedule(");
    expect(scheduler).toContain("const succeeded = await onClearSchedule()");
    expect(scheduler).toContain("if (succeeded) {");
    expect(scheduler).toContain("if (succeeded) closeAndReturnFocus();");
    expect(scheduler).toContain("const closeAndReturnFocus = useCallback");
    expect(cleanupScan).toContain("return true;");
    expect(cleanupScan).toContain("return false;");
  });

  test("System Cleanup does not poll and mutate the global body scroll lock", async () => {
    const cleanupPanel = await read("src/panels/cleanup/SystemCleanupPanel.tsx");

    expect(cleanupPanel).not.toContain('document.body.hasAttribute("data-scroll-locked")');
    expect(cleanupPanel).not.toContain("window.setInterval(clearIfLeaked");
  });

  test("privacy monitor cards use a single shared card shell", async () => {
    const remote = await read("src/panels/privacy/RemoteAccessMonitorSection.tsx");
    const rdp = await read("src/panels/privacy/RdpIdleCard.tsx");
    const ransomware = await read("src/panels/privacy/RansomwareMonitorSection.tsx");

    expect(remote).not.toContain('className="rounded-lg border p-5 transition-colors"');
    expect(rdp).not.toContain("<ToggleSection");
    expect(ransomware).toContain('import SectionCard from "../../components/shared/SectionCard";');
    expect(ransomware).toContain("<SectionCard");
  });

  test("Network Control keeps DNS and hosts copy concise", async () => {
    const network = await read("src/panels/network/index.tsx");

    expect(network).not.toContain("nobody on the network can see");
    expect(network).not.toContain("Censorship protection lives in Settings.");
    expect(network).not.toContain("Block categories before connections leave this PC");
    expect(network).not.toContain("changes apply automatically");
    expect(network).not.toContain("Encrypted DNS is applied per adapter.");
    expect(network).not.toContain("Choose categories to filter.");
    expect(network).toContain('title="DNS Firewall"');
    expect(network).toContain('title="Hosts Protection"');
    expect(network).toContain('aria-labelledby="dns-optional-heading"');
    expect(network).toContain("Government and social media are off by default");
    expect(network).toContain("Recommended");
  });

  test("Windows Settings owns the power and graphics controls while System Maintenance keeps an issue-first driver view", async () => {
    const startupDrivers = await read("src/panels/maintenance/StartupDriverTools.tsx");
    const maintenance = await read("src/panels/maintenance/index.tsx");
    const network = await read("src/panels/network/NetworkMaintenanceTools.tsx");

    const tweaks = await read("src/panels/tweaks/index.tsx");
    expect(startupDrivers).toContain("<StartupManager embedded scanKey={managerScanKey} />");
    expect(startupDrivers).toContain("<DriverHealthSection embedded hideActions scanKey={driverScanKey} />");
    expect(startupDrivers).toContain('showAllDrivers ? "Hide all drivers" : "Show all drivers"');
    expect(startupDrivers).toContain('tools.drivers?.drivers.slice(0, 200)');
    expect(startupDrivers).toContain('signed ? "Signed" : "Unsigned"');
    expect(startupDrivers).toContain('className="system-manager-scan"');
    expect(tweaks).toContain('<PowerGraphicsCard');
    expect(tweaks).toContain('powerSection={TWEAKS_SECTIONS[6]}');
    expect(tweaks).toContain('gpuSection={gpuVendorDetected ? TWEAKS_SECTIONS[5] : undefined}');
    expect(tweaks).toContain('titleOverride="Energy & Speed"');
    expect(maintenance).not.toContain('TabsTrigger value="performance"');
    expect(maintenance).not.toContain("PerformanceTools");
    expect(tweaks).not.toContain("<StartupManager embedded />");
    expect(tweaks).not.toContain("<DriverHealthSection");
    expect(network).toContain("NetworkMaintenanceTools");
  });

  test("System Managers workbench (Users/Tasks/Services/Conceal) lives in Startup & drivers, not Windows Settings", async () => {
    const startupDrivers = await read("src/panels/maintenance/StartupDriverTools.tsx");
    const tweaks = await read("src/panels/tweaks/index.tsx");

    expect(startupDrivers).toContain("<LocalUsersManager embedded scanKey={managerScanKey} />");
    expect(startupDrivers).toContain("<ScheduledTasksManager embedded scanKey={managerScanKey} />");
    expect(startupDrivers).toContain("<ServiceManager embedded scanKey={managerScanKey} />");
    expect(startupDrivers).toContain("<RuntimeVisibilityManager embedded scanKey={managerScanKey} />");
    expect(tweaks).not.toContain("LocalUsersManager");
    expect(tweaks).not.toContain("ScheduledTasksManager");
    expect(tweaks).not.toContain("ServiceManager");
    expect(tweaks).not.toContain("RuntimeVisibilityManager");
    expect(tweaks).not.toContain("System Managers");
  });

  test("Private Network VPN kill switch reads as a card-level toggle", async () => {
    const source = await read("src/panels/mesh/VpnKillSwitchSection.tsx");
    const css = await read("src/panels/mesh/VpnKillSwitchSection.css");

    expect(source).toContain("vpn-ks-card--armed");
    expect(source).toContain("Feature");
    expect(css).toContain(".vpn-ks-card--armed");
  });

  test("Disk Space Analyzer keeps scan session state outside the mounted panel", async () => {
    const analyzer = await read("src/panels/maintenance/DiskSpaceAnalyzerDialog.tsx");

    expect(analyzer).toContain("diskAnalyzerSession");
    expect(analyzer).toContain("subscribeDiskAnalyzerSession");
    expect(analyzer).toContain("persistDiskAnalyzerSession");
  });

  test("Packages and Apps keeps search, filters, and batch actions in one responsive toolbar", async () => {
    const panel = await read("src/panels/apps/components/AppInstallerPanel.tsx");
    const css = await read("src/panels/apps/components/AppInstallerPanel.css");

    expect(panel).toContain('className="installer-toolbar-layout"');
    expect(panel).toContain('className="installer-toolbar-search"');
    expect(panel).toContain('className="installer-filter-row"');
    // actions-row is applied via cn() so it can toggle an is-hidden class for the
    // engines view — assert the class reference rather than a static className=.
    expect(panel).toContain('"installer-actions-row"');
    expect(panel).toContain('text="REFRESH"');
    expect(panel).toContain('text="SELECT ALL"');
    expect(panel).toContain('text="CLEAR"');
    expect(panel).toContain('text="UPDATE ALL"');
    expect(panel).toContain('<TabsTrigger value="not-installed">Not Installed</TabsTrigger>');
    expect(panel).toContain('<TabsTrigger value="updates">Updates</TabsTrigger>');
    expect(panel).toContain('<TabsTrigger value="installed">Installed</TabsTrigger>');
    // The action row spans the full toolbar width so the controls no longer
    // compete with the search and category chips. The filter area receives the
    // flexible column; search remains a readable fixed-width control.
    expect(css).toContain("grid-column: 1 / -1");
    expect(css).toContain(".installer-toolbar-layout");
    expect(css).toContain("grid-template-columns: minmax(240px, 340px) 1fr");
    expect(css).toContain(".installer-action-buttons-utility");
    expect(css).toContain(".installer-action-buttons-bulk");
  });

  test("dashboard resolves hidden Risk Matrix and More Products before rendering views", async () => {
    const dashboard = await read("src/panels/dashboard/index.tsx");

    expect(dashboard).toContain("const effectiveViewMode");
    expect(dashboard).toContain('viewMode={effectiveViewMode}');
    expect(dashboard).toContain('effectiveViewMode === "risk"');
    expect(dashboard).toContain('effectiveViewMode === "products"');
  });

  test("dashboard view toggle boosts label contrast in light mode", async () => {
    const css = await read("src/panels/dashboard/index.css");

    expect(css).toContain("color: var(--color-text-secondary);");
    expect(css).toContain("background: color-mix(in srgb, var(--color-bg-tertiary) 58%, transparent);");
    expect(css).toContain("html.light .view-toggle-container {");
    expect(css).toContain("background: color-mix(in srgb, var(--color-bg-secondary) 94%, transparent);");
    expect(css).toContain("html.light .view-toggle-btn.active {");
  });

  test("USB Intelligence renders backend numeric trust scores", async () => {
    const source = await read("src/panels/privacy/UsbDevicesSection.tsx");
    const lib = await read("src-tauri/commander-free/src/lib.rs");

    expect(source).toContain('invoke<UsbTrustScore>("usb_device_trust_score"');
    expect(source).toContain("formatTrustScore");
    expect(source).toContain("trustScoreTone");
    expect(source).toContain("Trust score");
    expect(lib).toContain("usb_policy::usb_device_trust_score");
  });
});
