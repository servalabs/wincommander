import { describe, expect, test } from "bun:test";
import {
  runStealthSteps,
  stealthParts,
  stealthPending,
  stealthSetupError,
  stealthSummary,
  type StealthState,
} from "./stealthMode";

const none: StealthState = { lock: false, cover: false, hide: false, backend: false };
const all: StealthState = { lock: true, cover: true, hide: true, backend: true };

describe("stealth order", () => {
  test("turning on ends with the calculator lock", () => {
    expect(stealthPending(none, true, "on")).toEqual(["cover", "backend", "hide", "lock"]);
  });

  test("turning off shows the app again before the lock is released", () => {
    expect(stealthPending(all, true, "off")).toEqual(["hide", "backend", "cover", "lock"]);
  });

  test("only steps that are not done yet run", () => {
    expect(stealthPending({ ...none, lock: true, backend: true }, true, "on")).toEqual(["cover", "hide"]);
    expect(stealthPending({ ...none, hide: true }, true, "off")).toEqual(["hide"]);
  });

  test("an unpaid install skips cover identity on, but still undoes a lapsed one", () => {
    expect(stealthParts(false)).toEqual(["backend", "hide", "lock"]);
    expect(stealthPending(none, false, "on")).toEqual(["backend", "hide", "lock"]);
    expect(stealthPending({ ...none, cover: true }, false, "off")).toEqual(["cover"]);
  });
});

describe("stealth summary", () => {
  test("reports off, partial and on", () => {
    expect(stealthSummary(none, true)).toEqual({ mode: "off", active: 0, total: 4 });
    expect(stealthSummary({ ...none, lock: true, hide: true }, true)).toEqual({ mode: "partial", active: 2, total: 4 });
    expect(stealthSummary(all, true)).toEqual({ mode: "on", active: 4, total: 4 });
  });

  test("three protections are enough without a paid plan", () => {
    expect(stealthSummary({ lock: true, cover: false, hide: true, backend: true }, false).mode).toBe("on");
  });
});

describe("stealth setup check", () => {
  const base = { hasRealPin: false, pin: "4829", coverPending: true, coverName: "Notes" };

  test("accepts a valid PIN and a cover name", () => {
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

  test("requires a cover name only while cover identity still has to be set", () => {
    expect(stealthSetupError({ ...base, coverName: "  " })).toContain("cover name");
    expect(stealthSetupError({ ...base, coverPending: false, coverName: "" })).toBeNull();
  });
});

describe("runStealthSteps", () => {
  test("runs every step in order", async () => {
    const seen: string[] = [];
    const result = await runStealthSteps(["cover", "hide"], async (id) => { seen.push(id); return true; });
    expect(seen).toEqual(["cover", "hide"]);
    expect(result).toEqual({ done: ["cover", "hide"], failed: null });
  });

  test("stops at the first step that reports failure", async () => {
    const seen: string[] = [];
    const result = await runStealthSteps(["cover", "backend", "lock"], async (id) => { seen.push(id); return id !== "backend"; });
    expect(seen).toEqual(["cover", "backend"]);
    expect(result).toEqual({ done: ["cover"], failed: "backend" });
  });

  test("a step that throws counts as a failure", async () => {
    const result = await runStealthSteps(["hide", "lock"], async () => { throw new Error("boom"); });
    expect(result).toEqual({ done: [], failed: "hide" });
  });
});
