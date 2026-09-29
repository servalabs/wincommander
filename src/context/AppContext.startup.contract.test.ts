import { describe, expect, test } from "bun:test";

declare const Bun: { file(path: string): { text(): Promise<string> } };

describe("AppContext startup coordination", () => {
  test("waits for the shared native settings read after a soft timeout and keeps eligibility out of startup effect dependencies", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();

    expect(source).toMatch(/id:\s*['"]settings-cache['"]/);
    expect(source).toMatch(/cached\.outcome !== ['"]completed['"] \|\| !cached\.value/);
    expect(source).toContain("hydratedSettings = await hydrateWithinBudget(");
    expect(source).toContain("hydratedSettings = await initSettings(false);");
    expect(source).toContain("setStartupError(settingsRecoveryMessageRef.current ?? 'WinCommander could not load its settings. Retry to continue.')");
    expect(source).toContain("reportStartupPhase('settings_cache_hydrated')");
    expect(source).toMatch(/id:\s*['"]system-probe['"]/);
    expect(source).toMatch(/id:\s*['"]startup-status['"]/);
    expect(source).toContain("emitProgress(70, 'checking system readiness')");
    expect(source).toContain("setStartupComplete(true);");
    expect(source).toContain("settings = normalizeStartupSettings(settings);");
    expect(source).toContain("only an elevated administrator may save");
    expect(source).toContain("startupEligibilityRef.current");
    expect(source).not.toContain("runStartupJob,\n    startupEligibility,");
  });

  test("stops automatic retries when Windows account key recovery is required", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();
    const startup = source.slice(source.indexOf("const runStartupSequence ="));
    const boundedRetry = startup.indexOf("hydratedSettings = await hydrateWithinBudget(");
    const finalRetry = startup.indexOf("hydratedSettings = await initSettings(false);");
    const recoveryGuard = /if \(settingsRecoveryMessageRef\.current\) \{\s*setStartupError\(settingsRecoveryMessageRef\.current\);\s*return;/;

    expect(startup.slice(0, boundedRetry)).toMatch(recoveryGuard);
    expect(startup.slice(boundedRetry, finalRetry)).toMatch(recoveryGuard);
    expect(source).toContain("settingsRecoveryMessageRef.current = getStartupSettingsRecoveryMessage(error)");
  });

  test("hydrates cached settings without waiting for a settings write", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();
    const hydration = source.slice(source.indexOf("const initSettings ="), source.indexOf("// Phase 2: System probe"));

    expect(hydration).toContain("'get_settings'");
    expect(hydration).toContain("setAppSettings(settings)");
    expect(hydration).not.toContain("'patch_settings_cmd'");
    expect(hydration).not.toContain("'set_settings'");
  });

  test("keeps recovery notice independent of patch snapshots and out of decoy mode", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();
    const hydration = source.slice(source.indexOf("const initSettings ="), source.indexOf("// Phase 2: System probe"));
    const refresh = source.slice(source.indexOf("const refreshSettings ="), source.indexOf("const refreshHardening ="));
    const shell = await Bun.file("src/components/AppShell.tsx").text();

    expect(hydration).toContain("setPersonalSettingsStatus(previous => readPersonalSettingsStatus(settings, previous))");
    expect(refresh).toContain("setPersonalSettingsStatus(previous => readPersonalSettingsStatus(updated, previous))");
    expect(hydration).not.toContain("setStartupError(");
    expect(source).toContain('personalSettingsStatus: authMode === "decoy" ? null : personalSettingsStatus');
    expect(shell).toContain("<PersonalSettingsNotice status={personalSettingsStatus} />");
    expect(shell.indexOf("<PersonalSettingsNotice")).toBeGreaterThan(shell.indexOf("<TitleBar"));
  });

  test("persists normalized module defaults when the user edits modules", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();
    const mutation = source.slice(source.indexOf("const patchAppSettings ="), source.indexOf("const startupEligibility ="));

    expect(mutation).toContain("normalizeModulesConfig(");
    expect(mutation).toContain("...currentModules,");
    expect(mutation).toContain("...normalizedPatch.app.modules,");
    expect(mutation).toContain("invoke<AppSettings>('patch_settings_cmd', { patch: normalizedPatch })");
  });

  test("checks package updates automatically after launch without blocking startup", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();

    expect(source).toContain("id: 'package-updates'");
    expect(source).toContain("scheduleWhenIdle(0");
    expect(source).toContain("runPackageUpdateInventoryCheck(() => packageUpdatesInventory())");
    expect(source).toContain("existingPackageInventory.status === 'ready'");
    expect(source).toContain("if (isAppInventoryRefreshDue(inventoryAtStart))");
    expect(source).toContain("authMode !== 'decoy'");
    expect(source).toContain("isAppInventoryRefreshDue(latestCachedInventoryAt)");
    expect(source).not.toContain("PACKAGE_UPDATE_STARTUP_DELAY_MS");
  });
});
