import { expect, test } from "bun:test";
import { getStartupSettingsRecoveryMessage, hydrateWithinBudget, normalizeModulesConfig, normalizeStartupSettings, readPersonalSettingsStatus } from "./startupHydration";
import { getDefaultModules, getFirstRunModules } from "../types/modules";
import type { AppSettings } from "../types/settings";
import { createStartupProbeStore } from '../services/startupProbeStore';

test("unreadable legacy settings can hydrate fresh defaults without blocking startup", async () => {
  const response = {
    app: { firstRunComplete: true, theme: "dark" },
    personalSettingsStatus: { mode: "service", recoveryRequired: true, canSave: true },
  } as unknown as AppSettings;
  const hydrated = await hydrateWithinBudget(async () => normalizeStartupSettings(response));

  expect(hydrated?.app.theme).toBe("dark");
  expect(readPersonalSettingsStatus(hydrated)).toEqual({ mode: "service", recoveryRequired: true, canSave: true });
});

test("runtime recovery notice survives settings responses without metadata", () => {
  const previous = readPersonalSettingsStatus({ personalSettingsStatus: { mode: "temporary", recoveryRequired: true, canSave: false } });
  expect(readPersonalSettingsStatus({ app: { theme: "light" } }, previous)).toBe(previous);
  expect(readPersonalSettingsStatus({ personalSettingsStatus: { canSave: true } }, previous)).toBe(previous);
  expect(readPersonalSettingsStatus({ personalSettingsStatus: { mode: "service", recoveryRequired: "false", canSave: true } }, previous)).toBe(previous);
});

test("authoritative reads update recovery and service availability independently", () => {
  const unavailable = readPersonalSettingsStatus({ personalSettingsStatus: { mode: "temporary", recoveryRequired: true, canSave: false } });
  const serviceRestored = readPersonalSettingsStatus({ personalSettingsStatus: { mode: "service", recoveryRequired: true, canSave: true } }, unavailable);
  expect(serviceRestored).toEqual({ mode: "service", recoveryRequired: true, canSave: true });
  const recovered = readPersonalSettingsStatus({ personalSettingsStatus: { mode: "service", recoveryRequired: false, canSave: true } }, serviceRestored);
  expect(recovered).toEqual({ mode: "service", recoveryRequired: false, canSave: true });
  expect(readPersonalSettingsStatus({ personalSettingsStatus: recovered }, recovered)).toBe(recovered);
});

test("account key failures explain recovery without revealing native diagnostics", () => {
  for (const error of [
    "SETTINGS_KEY_UNAVAILABLE: native failure at sensitive profile path",
    new Error("read_settings failed: SETTINGS_KEY_UNAVAILABLE: native failure"),
  ]) {
    const message = getStartupSettingsRecoveryMessage(error);
    expect(message).toContain("Your saved data is preserved");
    expect(message).toContain("Restore this Windows account's access");
    expect(message).not.toContain("native failure");
    expect(message).not.toContain("sensitive profile path");
  }
});

test("ordinary settings failures keep the normal retry path", () => {
  for (const error of ["Access denied", new Error("Temporary sharing violation"), null, {}]) {
    expect(getStartupSettingsRecoveryMessage(error)).toBeNull();
  }
});

test("failed settings read settles without making startup ready", async () => {
  expect(await hydrateWithinBudget(async () => { throw new Error("unavailable"); })).toBeNull();
});

test("hung settings read expires and late consumer is aborted", async () => {
  let signal!: AbortSignal;
  let resolve!: (value: string) => void;
  const pending = new Promise<string>(done => { resolve = done; });
  const result = await hydrateWithinBudget(active => { signal = active; return pending; }, 5);
  expect(result).toBeNull();
  expect(signal.aborted).toBe(true);
  resolve("late settings");
});

test("successful retry returns settings within its budget", async () => {
  expect(await hydrateWithinBudget(async () => ({ loaded: true }))).toEqual({ loaded: true });
});

test('repeated startup recovery waits share one native read and accept its eventual result', async () => {
  const store = createStartupProbeStore<string>();
  let finish!: (value: string) => void;
  let calls = 0;
  const native = new Promise<string>(resolve => { finish = resolve; });
  const read = (signal: AbortSignal) => store.refresh(() => { calls++; return native; }, signal);
  expect(await hydrateWithinBudget(read, 5)).toBeNull();
  expect(await hydrateWithinBudget(read, 5)).toBeNull();
  const recovery = hydrateWithinBudget(read, 100);
  finish('saved choices');
  expect(await recovery).toBe('saved choices');
  expect(calls).toBe(1);
});

test("sparse settings hydrate without changing the shared snapshot or saved choices", () => {
  const modules = Object.freeze({ network: false, vault: true });
  const app = Object.freeze({ modules, firstRunComplete: true, experienceLevel: "simple", theme: "dark" });
  const current = Object.freeze({ device: { hostname: "test-machine" } });
  const settings = Object.freeze({ app, current, ideal: { retained: true } }) as unknown as AppSettings;

  const hydrated = normalizeStartupSettings(settings);

  expect(hydrated.app.modules).toEqual({ ...getDefaultModules("simple"), network: false, vault: true });
  expect(hydrated.app.theme).toBe("dark");
  expect(hydrated.current).toBe(current);
  expect(hydrated.ideal).toBe(settings.ideal);
  expect(settings.app.modules).toEqual({ network: false, vault: true });
});

test("separate sessions normalize their own choices without modifying other sessions", () => {
  const settings = [
    { firstRunComplete: true, experienceLevel: "simple", modules: { network: false } },
    { firstRunComplete: true, experienceLevel: "advanced", modules: { vault: false } },
    { firstRunComplete: false, modules: { privacy: false } },
  ].map(app => ({ app }) as AppSettings);

  const hydrated = settings.map(normalizeStartupSettings);
  expect(hydrated[0].app.modules).toEqual({ ...getDefaultModules("simple"), network: false });
  expect(hydrated[1].app.modules).toEqual({ ...getDefaultModules("advanced"), vault: false });
  expect(hydrated[2].app.modules).toEqual({ ...getFirstRunModules(), privacy: false });
  hydrated[0].app.modules!.apps = false;
  expect(hydrated[1].app.modules!.apps).toBe(true);
  expect(hydrated[2].app.modules!.apps).toBe(true);
});

test("actual module edits retain the same complete normalization used at startup", () => {
  const settings = { app: { firstRunComplete: true, experienceLevel: "standard", modules: { vault: false } } } as AppSettings;
  const hydrated = normalizeStartupSettings(settings);
  const edit = { ...normalizeModulesConfig(hydrated.app.modules, hydrated.app.experienceLevel, hydrated.app.firstRunComplete), vault: true };

  expect(edit).toEqual({ ...getDefaultModules("standard"), vault: true });
  expect(hydrated.app.modules!.vault).toBe(false);
});
