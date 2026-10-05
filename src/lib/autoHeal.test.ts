import { expect, test } from "bun:test";
import { createAutoHealer, HEAL_COOLDOWN_MS } from "./autoHeal";
import { getToggleDrift } from "./toggleDrift";
import type { AppSettings } from "../types/settings";
import type { ToggleDef } from "../types/toggles";

const toggle = { id: "test", label: "Test setting", settingsPath: "ideal.privacy.enabled", currentPath: "current.privacy.enabled", enableCmd: "Enable-Test", disableCmd: "Disable-Test" } as ToggleDef;
function fixture() {
  let settings = { app: { firstRunComplete: true, autoHeal: true }, ideal: { privacy: { enabled: true } }, current: { privacy: { enabled: false } } } as unknown as AppSettings;
  let time = 0;
  let current: unknown = false;
  let fail = false;
  let changeWindows = true;
  let active = true;
  const calls: boolean[] = [];
  const notices: string[] = [];
  const verified: boolean[] = [];
  let probes = 0;
  const run = createAutoHealer({
    getSettings: () => active ? settings : undefined,
    toggles: [toggle], canUse: () => true, now: () => time,
    probe: async () => { probes++; return { ...settings, current: { privacy: { enabled: current } } } as unknown as AppSettings; },
    repair: async (_, target) => { calls.push(target); if (fail) throw new Error("Access denied"); if (changeWindows) current = target; },
    failureMessage: error => String(error), notify: (_, message) => { notices.push(message); },
    verified: repair => { verified.push(repair.targetChecked); },
  });
  return { run, calls, notices, verified, probes: () => probes,
    advance: () => { time += HEAL_COOLDOWN_MS; },
    setCurrent: (value: unknown) => { current = value; },
    setFail: (value: boolean) => { fail = value; },
    setActive: (value: boolean) => { active = value; },
    setChangeWindows: (value: boolean) => { changeWindows = value; },
    configure: (patch: object) => { settings = { ...settings, ...patch } as AppSettings; },
  };
}

test("unchanged failed drift retries after cooldown and reports repeated failure once", async () => {
  const f = fixture(); f.setFail(true);
  await f.run(); await f.run();
  expect(f.calls).toEqual([true]);
  f.advance(); await f.run();
  expect(f.calls).toEqual([true, true]);
  expect(f.notices).toHaveLength(1);
  f.setFail(false); f.advance(); await f.run();
  expect(f.verified).toEqual([true]);
  expect(f.probes()).toBe(5);
});

test("command acknowledgement does not confirm a repair while Windows stays unchanged", async () => {
  const f = fixture(); f.setChangeWindows(false);
  await f.run(); f.advance(); await f.run();
  expect(f.calls).toEqual([true, true]);
  expect(f.verified).toEqual([]);
  expect(f.notices).toHaveLength(1);
});

test("unknown and absent observations never become OFF or cause a repair", async () => {
  const f = fixture();
  for (const value of [undefined, null]) {
    f.setCurrent(value); await f.run();
  }
  expect(f.calls).toEqual([]);
});

test("automatic repairs honor current desired OFF and stop when disabled or in decoy", async () => {
  const f = fixture();
  f.configure({ ideal: { privacy: { enabled: false } } }); f.setCurrent(true);
  await f.run(); expect(f.calls).toEqual([false]); expect(f.verified).toEqual([false]);
  f.configure({ app: { firstRunComplete: true, autoHeal: false } }); f.setCurrent(true); f.advance();
  await f.run(); expect(f.calls).toEqual([false]);
  f.setActive(false); await f.run(); expect(f.calls).toEqual([false]);
});

test("managed locks heal only their exact path or children when Auto Heal is OFF", async () => {
  const f = fixture(); f.configure({ app: { firstRunComplete: true, autoHeal: false }, policy: { syncMode: "managed", lockedPaths: ["privacy.enable"] } });
  await f.run(); expect(f.calls).toEqual([]);
  f.configure({ policy: { syncMode: "managed", lockedPaths: ["privacy"] } });
  await f.run(); expect(f.calls).toEqual([true]);
});

test("overlapping scans and disabling during a probe cannot queue duplicate repairs", async () => {
  let active = true;
  const settings = { app: { firstRunComplete: true, autoHeal: true }, ideal: { privacy: { enabled: true } }, current: { privacy: { enabled: false } } } as unknown as AppSettings;
  let finish!: (settings: AppSettings) => void;
  let probes = 0;
  let repairs = 0;
  const run = createAutoHealer({ getSettings: () => active ? settings : undefined, toggles: [toggle], canUse: () => true,
    probe: () => { probes++; return new Promise(resolve => { finish = resolve; }); },
    repair: async () => { repairs++; }, failureMessage: String, notify: () => {}, verified: () => {},
  });
  const first = run(); await run();
  active = false; finish(settings); await first;
  expect(probes).toBe(1); expect(repairs).toBe(0);
});

test("irreversible actions and unknown desired state never count as drift to repair", () => {
  const settings = { ideal: { privacy: {} }, current: { privacy: { enabled: false } } } as unknown as AppSettings;
  expect(getToggleDrift(settings, toggle)).toBeNull();
  expect(getToggleDrift({ ...settings, ideal: { privacy: { enabled: true } } } as unknown as AppSettings, { ...toggle, irreversible: true })).toBeNull();
});

test("disabling while a repair is pending stops subsequent repairs and verification", async () => {
  let active = true;
  const settings = { app: { firstRunComplete: true, autoHeal: true }, ideal: { privacy: { enabled: true } }, current: { privacy: { enabled: false } } } as unknown as AppSettings;
  let finish!: () => void;
  let started!: () => void;
  const pending = new Promise<void>(resolve => { started = resolve; });
  const repairs: string[] = [];
  let probes = 0;
  const verified: unknown[] = [];
  const run = createAutoHealer({
    getSettings: () => active ? settings : undefined, toggles: [toggle, { ...toggle, id: "second" }], canUse: () => true,
    probe: async () => { probes++; return settings; },
    repair: async item => { repairs.push(item.id); started(); await new Promise<void>(resolve => { finish = resolve; }); },
    failureMessage: String, notify: () => {}, verified: value => { verified.push(value); },
  });
  const running = run(); await pending;
  active = false; finish(); await running;
  expect(repairs).toEqual(["test"]); expect(probes).toBe(1); expect(verified).toEqual([]);
});

test("disabling during final readback does not publish a completed repair", async () => {
  let active = true;
  const settings = { app: { firstRunComplete: true, autoHeal: true }, ideal: { privacy: { enabled: true } }, current: { privacy: { enabled: false } } } as unknown as AppSettings;
  let finish!: (value: AppSettings) => void;
  let started!: () => void;
  const pending = new Promise<void>(resolve => { started = resolve; });
  let probes = 0;
  const verified: unknown[] = [];
  const run = createAutoHealer({
    getSettings: () => active ? settings : undefined, toggles: [toggle], canUse: () => true,
    probe: async () => { if (++probes === 1) return settings; started(); return new Promise(resolve => { finish = resolve; }); },
    repair: async () => {}, failureMessage: String, notify: () => {}, verified: value => { verified.push(value); },
  });
  const running = run(); await pending;
  active = false; finish({ ...settings, current: { privacy: { enabled: true } } } as unknown as AppSettings); await running;
  expect(probes).toBe(2); expect(verified).toEqual([]);
});
