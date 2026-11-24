import { describe, expect, test } from "bun:test";
import {
  DEFAULT_COVER_NAME,
  resolveCoverName,
  runStealthSteps,
  stealthParts,
  stealthPending,
  stealthSetupError,
  stealthStatus,
  stealthSummary,
  type StealthState,
} from "./stealthMode";

const none: StealthState = { lock: false, cover: false, backend: false };
const all: StealthState = { lock: true, cover: true, backend: true };

describe("stealth order", () => {
  test("turning on ends with the calculator lock", () => {
    expect(stealthPending(none, true, "on")).toEqual(["cover", "backend", "lock"]);
  });

  test("turning off releases the lock last", () => {
    expect(stealthPending(all, true, "off")).toEqual(["backend", "cover", "lock"]);
  });

  test("only steps that are not done yet run", () => {
    expect(stealthPending({ ...none, lock: true, backend: true }, true, "on")).toEqual(["cover"]);
    expect(stealthPending({ ...none, backend: true }, true, "off")).toEqual(["backend"]);
  });

  test("an unpaid install skips cover identity on, but still undoes a lapsed one", () => {
    expect(stealthParts(false)).toEqual(["backend", "lock"]);
    expect(stealthPending(none, false, "on")).toEqual(["backend", "lock"]);
    expect(stealthPending({ ...none, cover: true }, false, "off")).toEqual(["cover"]);
  });
});

describe("stealth summary", () => {
  test("reports off, partial and on", () => {
    expect(stealthSummary(none, true)).toEqual({ mode: "off", active: 0, total: 3 });
    expect(stealthSummary({ ...none, lock: true }, true)).toEqual({ mode: "partial", active: 1, total: 3 });
    expect(stealthSummary(all, true)).toEqual({ mode: "on", active: 3, total: 3 });
  });

  test("two protections are enough without a paid plan", () => {
    expect(stealthSummary({ lock: true, cover: false, backend: true }, false).mode).toBe("on");
  });
});

describe("stealth status pill", () => {
  test("names the state and how many protections are on", () => {
    expect(stealthStatus(stealthSummary(none, true), false)).toEqual({ label: "Off", tone: "off" });
    expect(stealthStatus(stealthSummary({ ...none, lock: true }, true), false)).toEqual({ label: "Partly on (1 of 3)", tone: "partial" });
    expect(stealthStatus(stealthSummary(all, true), false)).toEqual({ label: "On", tone: "on" });
  });

  test("an unpaid install counts two protections", () => {
    expect(stealthStatus(stealthSummary({ ...none, lock: true }, false), false).label).toBe("Partly on (1 of 2)");
  });

  test("shows Working while a step runs, whatever the state", () => {
    expect(stealthStatus(stealthSummary(all, true), true)).toEqual({ label: "Working…", tone: "busy" });
  });
});

describe("stealth setup check", () => {
  const base = { hasRealPin: false, pin: "4829" };

  test("accepts a valid PIN", () => {
    expect(stealthSetupError(base)).toBeNull();
  });

  test("requires a calculator PIN that the calculator can type", () => {
    expect(stealthSetupError({ ...base, pin: "" })).toContain("at least 4 digits");
    expect(stealthSetupError({ ...base, pin: "0482" })).toContain("can't start with 0");
    expect(stealthSetupError({ ...base, pin: "1234" })).toContain("sequence");
  });

  test("does not ask for a PIN that already exists", () => {
    expect(stealthSetupError({ ...base, hasRealPin: true, pin: "" })).toBeNull();
  });

});

describe("cover name", () => {
  test("a blank or whitespace-only name falls back to Calculator, never to the real app name", () => {
    expect(DEFAULT_COVER_NAME).toBe("Calculator");
    expect(resolveCoverName("")).toBe("Calculator");
    expect(resolveCoverName("   ")).toBe("Calculator");
  });

  test("a typed name is used trimmed", () => {
    expect(resolveCoverName("  Notes ")).toBe("Notes");
  });
});

describe("runStealthSteps", () => {
  test("runs every step in order", async () => {
    const seen: string[] = [];
    const result = await runStealthSteps(["cover", "backend"], async (id) => { seen.push(id); return true; });
    expect(seen).toEqual(["cover", "backend"]);
    expect(result).toEqual({ done: ["cover", "backend"], failed: null });
  });

  test("stops at the first step that reports failure", async () => {
    const seen: string[] = [];
    const result = await runStealthSteps(["cover", "backend", "lock"], async (id) => { seen.push(id); return id !== "backend"; });
    expect(seen).toEqual(["cover", "backend"]);
    expect(result).toEqual({ done: ["cover"], failed: "backend" });
  });

  test("a step that throws counts as a failure", async () => {
    const result = await runStealthSteps(["backend", "lock"], async () => { throw new Error("boom"); });
    expect(result).toEqual({ done: [], failed: "backend" });
  });
});
