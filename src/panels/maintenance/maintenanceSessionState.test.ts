import { expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

const read = (name: string) =>
  Bun.file(`src/panels/maintenance/${name}`).text();

test("maintenance scan hooks retain their results in the session store", async () => {
  const hooks = await Promise.all([
    "useFileHygiene.ts",
    "useMalwareCenter.ts",
    "useRegistryTools.ts",
    "useRoutineCleaner.ts",
    "useStartupDrivers.ts",
    "useSystemHygiene.ts",
  ].map(read));

  for (const source of hooks) {
    expect(source).toContain("useMaintenanceSessionState");
  }

  const panel = await read("index.tsx");
  expect(panel).toContain('useMaintenanceSessionState("maintenance.active-tab"');
});

test("system hygiene tab changes do not launch a scan", async () => {
  const source = await read("useSystemHygiene.ts");
  const changeTool = source.slice(
    source.indexOf("const changeTool"),
    source.indexOf("return { tool"),
  );

  expect(changeTool).toContain("setTool(next)");
  expect(changeTool).not.toContain("CleanerScan");
  expect(changeTool).not.toContain("await ");
});

test("registry audits pre-scan both views once and reuse the cached result on tab switch", async () => {
  const source = await read("useRegistryTools.ts");

  expect(source).toContain('"registry-hygiene.pre-scanned"');
  expect(source).toContain("Promise.all([backend.registryCleanerScan(), backend.explorerContextMenuScan()])");
});

test("system hygiene pre-scans each review once instead of scanning on tab changes", async () => {
  const source = await read("useSystemHygiene.ts");

  expect(source).toContain('"system-hygiene.pre-scanned"');
  expect(source).toContain("backendRef.current.shortcutCleanerScan()");
  expect(source).toContain("backendRef.current.environmentCleanerScan()");
  expect(source).toContain("backendRef.current.uninstallLeftoversScan()");
});

test("Maintenance preloads registry and hygiene reviews before their tabs mount", async () => {
  const source = await read("index.tsx");

  expect(source).toContain('"maintenance.review-preload-complete"');
  expect(source).toContain("backend.explorerContextMenuScan()");
  expect(source).toContain("backend.shortcutCleanerScan()");
  expect(source).toContain("backend.routineCleanerScan(APP_CACHE_CLEANUP_CATEGORIES)");
  expect(source).toContain('`${APP_CACHE_SESSION_KEY}.scan`');
});

test("storage dashboards do not auto-scan the manual analysis tools", async () => {
  const [analyzer, fileStats] = await Promise.all([
    read("DiskSpaceAnalyzerDialog.tsx"),
    read("FileStatsPanel.tsx"),
  ]);

  expect(analyzer).not.toContain("if (inline && !diskAnalyzerSession.meta");
  expect(fileStats).not.toContain("if (!fileStatsSession) void runScan()");
});

test("visible maintenance scan controls use icon-only accessible actions", async () => {
  const sources = await Promise.all([
    "FileHygieneTools.tsx",
    "MalwareCenter.tsx",
    "RegistryTools.tsx",
    "RoutineCleanerPanel.tsx",
    "SecurityData.tsx",
    "StartupDriverTools.tsx",
    "SystemHygieneTools.tsx",
  ].map(read));
  const combined = sources.join("\n");

  expect(combined).toContain('size="icon"');
  expect(combined).toContain('aria-label=');
  expect(combined).toContain('"refresh" : "search"');
  expect(/<Icon icon="stop" \/> Cancel/.test(combined)).toBe(false);
});
