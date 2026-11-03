import { expect, test } from "bun:test";
import { shortcutWarningsAfterReconciliation, syncSharedShortcutClaim } from "./sharedShortcutClaims";

test("reports denied and incomplete shared shortcut changes", async () => {
  expect((await syncSharedShortcutClaim("hide", true, async () => { throw new Error("forbidden"); })).warning).toContain("forbidden");
  expect((await syncSharedShortcutClaim("calculator", true, async () => ({ status: "partial" }))).warning).toContain("could not be confirmed");
  expect((await syncSharedShortcutClaim("hide", true, async () => ({ status: "partial", warnings: ["Collision"] }))).warning).toBe("Collision");
  expect((await syncSharedShortcutClaim("hide", true, async () => ({ status: "restored" }))).warning).toContain("still visible");
});

test("only clears the managed-link warning after service confirmation", () => {
  const warning = "Shared WinCommander shortcuts need service reconciliation.";
  expect(shortcutWarningsAfterReconciliation([warning, "Other shared link"], null)).toEqual(["Other shared link"]);
  expect(shortcutWarningsAfterReconciliation([warning], "Service denied")).toEqual([warning]);
});

test("accepts either confirmed aggregate state when another claim remains", async () => {
  expect((await syncSharedShortcutClaim("hide", false, async () => ({ status: "hidden" }))).warning).toBeNull();
  expect((await syncSharedShortcutClaim("calculator", false, async () => ({ status: "restored" }))).warning).toBeNull();
});

test("uses only confirmed per-surface managed replacements", async () => {
  expect((await syncSharedShortcutClaim("calculator", true, async () => ({ status: "hidden", managed: { desktop: true, start: false } }))).managed).toEqual({ desktop: true, start: false });
  const partial = await syncSharedShortcutClaim("calculator", true, async () => ({ status: "partial", managed: { desktop: true, start: false }, warnings: ["Start collision"] }));
  expect(partial.managed).toEqual({ desktop: true, start: false });
  expect(partial.warning).toBe("Start collision");
  expect((await syncSharedShortcutClaim("calculator", true, async () => { throw new Error("forbidden"); })).managed).toEqual({ desktop: false, start: false });
  expect((await syncSharedShortcutClaim("calculator", true, async () => ({ status: "hidden", managed: { desktop: "true" as unknown as boolean } }))).managed).toEqual({ desktop: false, start: false });
});

test("both toggle orders accept a remaining claim and a final restore", async () => {
  for (const first of ["calculator", "hide"] as const) {
    const second = first === "calculator" ? "hide" : "calculator";
    const claims = { calculator: false, hide: false };
    const send = async (kind: keyof typeof claims) => ({ status: claims[kind] || claims[first] || claims[second] ? "hidden" : "restored" });
    claims[first] = true;
    expect((await syncSharedShortcutClaim(first, true, send)).warning).toBeNull();
    claims[second] = true;
    expect((await syncSharedShortcutClaim(second, true, send)).warning).toBeNull();
    claims[first] = false;
    expect((await syncSharedShortcutClaim(first, false, send)).warning).toBeNull();
    claims[second] = false;
    expect((await syncSharedShortcutClaim(second, false, send)).warning).toBeNull();
  }
});
