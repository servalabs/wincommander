import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "./cleanupCategories";
import { parseCleanupDisplayPreference } from "./cleanupDisplay";
import { cleanupDisplayName, deriveCleanupVerification, summarizeCleanupGroups, summarizeCleanupResult } from "./cleanupDisplayModel";
import CleanupReadableCard from "../../components/cleanup/CleanupReadableCard";
import CleanupDisplayDetails from "../../components/cleanup/CleanupDisplayDetails";

const category = ALL_CATEGORIES.find(item => item.id === "amcache")!;
const base = { id: "amcache", count: 15, items: [], raw: { total: 15, entries: [{ category: "InventoryApplication", count: 15, sample: [] }] } };

describe("System Cleanup display projection", () => {
  test("uses a display-only preference and rejects unknown stored values", () => {
    expect(parseCleanupDisplayPreference("simple")).toBe("simple");
    expect(parseCleanupDisplayPreference("advanced")).toBe("advanced");
    expect(parseCleanupDisplayPreference("admin")).toBeNull();
  });

  test("keeps unknown, failed, partial and empty states distinct", () => {
    expect(summarizeCleanupResult({ id: "x", count: -1 }).state).toBe("not-scanned");
    expect(summarizeCleanupResult({ id: "x", count: 0 }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "x", count: 0, error: "denied" }).state).toBe("error");
    const partial = summarizeCleanupResult({ id: "x", count: 0, raw: { status: "partial" } });
    expect(partial.state).toBe("partial");
    expect(partial.count).toBeNull();
    expect(summarizeCleanupResult({ id: "x", count: 0, raw: { total: 0, entries: [] } }).state).toBe("empty");
    expect(summarizeCleanupResult({ id: "x", count: 2, raw: { total: 2 }, loading: true }).state).toBe("scanning");
    expect(summarizeCleanupResult({ id: "x", count: 0, raw: { total: 15 } }).state).toBe("inconsistent");
    expect(summarizeCleanupResult({ id: "amcache", count: 15, raw: { total: 15, entries: [] } }).state).toBe("inconsistent");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 0, raw: { total: 0, entries: [], complete: false } }).state).toBe("partial");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 0, raw: { total: 0, entries: [], status: "error" } }).state).toBe("error");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, raw: { total: 15, entries: [] } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, items: [], raw: { total: 15 } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, raw: { total: 99, entries: [{}] } }).state).toBe("inconsistent");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, raw: { total: 15, entries: [], previewLimit: 0 } }).state).toBe("found");
  });

  test("distinguishes full records from preview samples and rejects inconsistent category totals", () => {
    const valid = summarizeCleanupResult(base);
    expect(valid.state).toBe("found");
    expect(valid.count).toBe(15);
    expect(valid.sampleCount).toBe(0);
    expect(summarizeCleanupResult({ ...base, raw: { total: 15, entries: [{ category: "InventoryApplication", count: 0, sample: [] }] } }).state).toBe("inconsistent");
  });

  test("renders the same findings in both views without leaking raw content", () => {
    const raw = { ...base.raw, password: "secret-password", entries: [{ category: "InventoryApplication", count: 15, sample: [{ path: "C:\\Private\\secret.txt" }] }] };
    for (const mode of ["simple", "advanced"] as const) {
      const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode={mode} count={15} items={["private preview"]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
      expect(html).toContain("15 records");
      expect(html).not.toContain("secret-password");
      expect(html).not.toContain("secret.txt");
      expect(html).not.toContain("private preview");
      if (mode === "advanced") expect(html).toContain("Preview samples");
    }
  });

  test("advanced details explain a zero-sample preview and unavailable verification", () => {
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={15} items={[]} rawData={base.raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("15 records");
    expect(html).toContain("0 samples");
    expect(html).toContain("No verification receipt reported");
    expect(html).toContain("Not reported");
  });

  test("browser and Wi-Fi summaries show dataset counts without private names or paths", () => {
    for (const [id, raw] of [
      ["browserFootprints", { browsers: [{ profilePath: "C:\\Private\\Browser", password: "secret-value" }], total: 1 }],
      ["wlanProfiles", { profiles: [{ name: "PrivateNetwork", key: "secret-value" }], total: 1 }],
    ] as const) {
      const item = ALL_CATEGORIES.find(candidate => candidate.id === id)!;
      const html = renderToStaticMarkup(<CleanupDisplayDetails category={item} mode="advanced" count={1} items={[]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
      expect(html).toContain("Reported datasets");
      expect(html).not.toContain("PrivateNetwork");
      expect(html).not.toContain("Private\\Browser");
      expect(html).not.toContain("secret-value");
    }
  });

  test("all-user details keep account counts and reject missing or inconsistent coverage", () => {
    const groups = [{ title: "Account A", count: 2, items: ["private trace"] }, { title: "Account B", count: 1, items: [] }];
    expect(summarizeCleanupGroups(3, groups).state).toBe("found");
    expect(summarizeCleanupGroups(4, groups).state).toBe("inconsistent");
    expect(summarizeCleanupGroups(-1, groups).state).toBe("partial");
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={3} items={[]} groupedItems={groups} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Account A");
    expect(html).toContain("2 reported");
    expect(html).not.toContain("private trace");
    const card = renderToStaticMarkup(<CleanupReadableCard mode="simple" category={category} count={3} preview={[]} groupedItems={groups} loading={false} clearing={false} onClear={() => {}} />);
    expect(card).toContain("3 items reported");
    expect(card).not.toContain("private trace");
    const missing = renderToStaticMarkup(<CleanupReadableCard mode="simple" category={category} count={2} preview={[]} groupedItems={[groups[0], { title: "Account B", count: -1, items: [] }]} loading={false} clearing={false} onClear={() => {}} />);
    expect(missing).toContain("Account scan incomplete");
    expect(missing).toContain("disabled");
  });

  test("incomplete and failed cards never render a clean result in either view", () => {
    for (const mode of ["simple", "advanced"] as const) {
      for (const state of [{ error: "denied", rawData: undefined }, { error: undefined, rawData: { status: "partial" } }]) {
        const html = renderToStaticMarkup(<CleanupReadableCard mode={mode} category={category} count={0} preview={[]} loading={false} clearing={false} onClear={() => {}} {...state} />);
        expect(html).not.toContain("No items found");
        expect(html).toContain("disabled");
      }
    }
  });

  test("shows exclusions, schedule state and safe post-clear result on a card", () => {
    const excluded = ALL_CATEGORIES.find(item => item.id === "notepadState")!;
    const html = renderToStaticMarkup(<CleanupReadableCard mode="simple" category={excluded} count={0} preview={[]} rawData={{ total: 0, files: [] }} loading={false} clearing={false} excludedFromBulk scheduleState="verified" scheduleMinutes={60} onClear={() => {}} onLoad={() => {}} onSetSchedule={() => true} />);
    expect(html).toContain("Default exclusion");
    expect(html).toContain("skipped by Clear all");
    expect(html).toContain("Scheduled every 60 min");
    expect(html).toContain("No items found");
    expect(html).toContain("disabled");
    const unknown = renderToStaticMarkup(<CleanupReadableCard mode="simple" category={excluded} count={0} preview={[]} rawData={{ total: 0, files: [] }} loading={false} clearing={false} onClear={() => {}} onSetSchedule={() => true} />);
    expect(unknown).toContain("Schedule status not verified");
    expect(unknown).not.toContain("Schedule a wipe for");
    const blocked = ALL_CATEGORIES.find(item => item.id === "wlanProfiles")!;
    const existing = renderToStaticMarkup(<CleanupReadableCard mode="simple" category={blocked} count={-1} preview={[]} loading={false} clearing={false} scheduleState="verified" scheduleMinutes={60} onClear={() => {}} onClearSchedule={() => true} />);
    expect(existing).toContain("Scheduled every 60 min; new schedules unavailable");
    expect(existing).toContain("Turn off schedule");
  });

  test("every catalog entry has a display name in both views", () => {
    expect(ALL_CATEGORIES.length).toBe(84);
    for (const item of ALL_CATEGORIES) {
      expect(cleanupDisplayName(item, "simple").trim()).not.toBe("");
      expect(cleanupDisplayName(item, "advanced").trim()).not.toBe("");
      if (item.actionOnly) continue;
      for (const mode of ["simple", "advanced"] as const) {
        const raw = item.id === "amcache" ? { total: 1, entries: [{ category: "InventoryApplication", count: 1, sample: [] }] } : { total: 1, entries: [{ privateValue: "fixture-secret" }] };
        const html = renderToStaticMarkup(<CleanupReadableCard mode={mode} category={item} count={1} preview={["fixture-secret"]} rawData={raw} loading={false} clearing={false} onClear={() => {}} onLoad={() => {}} />);
        expect(html).toContain(`data-cleanup-category="${item.id}"`);
        expect(html).not.toContain("fixture-secret");
      }
    }
  });

  test("reports supplied post-clear receipts without exposing their contents", () => {
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={0} items={[]} rawData={{ total: 0, entries: [] }} verification={{ status: "cleared" }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("A fresh scan found no remaining items.");
    expect(html).not.toContain("No verification receipt reported");
  });

  test("only reports a verified clear after a successful, complete fresh scan", () => {
    const empty = summarizeCleanupResult({ id: "dnsCache", count: 0, raw: { total: 0, entries: [] } });
    const partial = summarizeCleanupResult({ id: "dnsCache", count: 0, raw: { status: "partial", entries: [] } });
    const input = { id: "dnsCache", before: 3, result: empty, backendSucceeded: true, deferred: false };
    expect(deriveCleanupVerification(input)).toBe("cleared");
    expect(deriveCleanupVerification({ ...input, backendSucceeded: false })).toBe("failed");
    expect(deriveCleanupVerification({ ...input, result: partial })).toBe("unverified");
    expect(deriveCleanupVerification({ ...input, deferred: true })).toBe("deferred");
    expect(deriveCleanupVerification({ ...input, deferred: true, backendSucceeded: false })).toBe("failed");
    expect(deriveCleanupVerification({ ...input, id: "ntfsJournals" })).toBe("unverified");
  });
});
