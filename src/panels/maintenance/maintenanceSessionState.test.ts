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
    "usePerformanceTools.ts",
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

test("visible maintenance scan controls use icon-only accessible actions", async () => {
  const sources = await Promise.all([
    "FileHygieneTools.tsx",
    "MalwareCenter.tsx",
    "PerformanceTools.tsx",
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
