import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "./cleanupCategories";
import { cleanupModeForPersona } from "./cleanupDisplay";
import { cleanupDisplayName, deriveCleanupVerification, summarizeCleanupGroups, summarizeCleanupResult } from "./cleanupDisplayModel";
import CleanupDisplayDetails from "../../components/cleanup/CleanupDisplayDetails";

const category = ALL_CATEGORIES.find(item => item.id === "amcache")!;
const base = { id: "amcache", count: 15, items: [], raw: { total: 15, entries: [{ category: "InventoryApplication", count: 15, sample: [] }] } };

describe("System Cleanup display projection", () => {
  test("uses the existing persona for presentation", () => {
    expect(cleanupModeForPersona("casual")).toBe("simple");
    expect(cleanupModeForPersona("secure")).toBe("advanced");
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
    expect(summarizeCleanupResult({ id: "x", count: 0, raw: { total: 15 } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "amcache", count: 15, raw: { total: 15, entries: [] } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 0, raw: { total: 0, entries: [], complete: false } }).state).toBe("partial");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 0, raw: { total: 0, entries: [], status: "error" } }).state).toBe("error");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, raw: { total: 15, entries: [] } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, items: [], raw: { total: 15 } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "ntfsJournals", count: 1, raw: { total: 2, journals: [{ present: true }, { present: false }] } }).state).toBe("found");
    expect(summarizeCleanupResult({ id: "dnsCache", count: 15, raw: { total: 15, entries: [], previewLimit: 0 } }).state).toBe("found");
  });

  test("distinguishes full records from preview samples and rejects inconsistent category totals", () => {
    const valid = summarizeCleanupResult(base);
    expect(valid.state).toBe("found");
    expect(valid.count).toBe(15);
    expect(valid.sampleCount).toBe(0);
    expect(summarizeCleanupResult({ ...base, raw: { total: 15, entries: [{ category: "InventoryApplication", count: 0, sample: [] }] } }).state).toBe("inconsistent");
    expect(summarizeCleanupResult({ ...base, raw: { total: 15, entries: [{ category: "InventoryApplication", sample: [] }] } }).state).toBe("unavailable");
    expect(summarizeCleanupResult({ id: "amcache", count: 0, raw: { total: 0, entries: [{ category: "InventoryApplication", count: 0, sample: [] }] } }).state).toBe("empty");
    expect(summarizeCleanupResult({ id: "ntUserTraces", count: 3, raw: { total: 3, sections: [{ name: "RunMRU", count: 2, entries: [{}, {}] }, { name: "TypedPaths", count: 1, entries: [{}] }] } }).state).toBe("found");
    expect(summarizeCleanupResult({ id: "ntUserTraces", count: 3, raw: { total: 3, sections: [{ name: "RunMRU", count: 3, entries: [] }] } }).state).toBe("found");
    expect(summarizeCleanupResult({ id: "ntUserTraces", count: 12, raw: { total: 12, sections: [{ name: "RunMRU", count: 0, entries: [] }] } }).state).toBe("inconsistent");
    expect(summarizeCleanupResult({ id: "wlanProfiles", count: 1, items: [], raw: { profiles: [{}] } }).sampleCount).toBe(1);
  });

  test("renders available records and captured scalar fields in both views", () => {
    const raw = { ...base.raw, password: "secret-password", entries: [{ category: "InventoryApplication", count: 15, sample: [{ path: "C:\\Private\\secret.txt" }] }] };
    for (const mode of ["simple", "advanced"] as const) {
      const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode={mode} count={15} items={["private preview"]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
      expect(html).toContain("15 records");
      expect(html).toContain("secret-password");
      expect(html).toContain("secret.txt");
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

  test("browser and Wi-Fi summaries show all available fields", () => {
    for (const [id, raw] of [
      ["browserFootprints", { browsers: [{ profilePath: "C:\\Private\\Browser", password: "secret-value" }], total: 1 }],
      ["wlanProfiles", { profiles: [{ name: "PrivateNetwork", key: "secret-value" }], total: 1 }],
    ] as const) {
      const item = ALL_CATEGORIES.find(candidate => candidate.id === id)!;
      const html = renderToStaticMarkup(<CleanupDisplayDetails category={item} mode="advanced" count={1} items={[]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
      expect(html).toContain("Reported datasets");
      expect(html).toContain(id === "wlanProfiles" ? "PrivateNetwork" : "Private\\Browser");
      expect(html).toContain("secret-value");
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
    expect(html).toContain("private trace");
    const casual = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="simple" count={3} items={[]} groupedItems={groups} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(casual).toContain("3 records");
    expect(casual).toContain("Account A");
    expect(casual).toContain("private trace");
    expect(casual).toContain("private trace");
  });

  test("distinguishes a saved schedule, a never-run task and Windows execution from verified cleanup", () => {
    const record = { enabled: true, managedByAutoSet: false, scope: "current-user" as const, intervalMinutes: 60, lastRun: "1999-11-30T00:00:00Z", lastResult: 0x41303, payloadVerified: true };
    const props = { category, mode: "simple" as const, count: 0, items: [], rawData: { total: 0, entries: [] }, clearing: false, clearLabel: "Clear", onClose: () => {}, scheduleState: "verified" as const, scheduleMinutes: 60 };
    const never = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleRecord={record} />);
    expect(never).toContain("No run reported");
    expect(never).toContain("task has not run");
    const ran = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleRecord={{ ...record, lastRun: "2026-10-30T10:30:00Z", lastResult: 0 }} />);
    expect(ran).toContain("task completed (0)");
    expect(ran).toContain("do not confirm which items were removed");
    const incomplete = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleState="verified" scheduleRecord={{ ...record, cleanupOutcome: { attemptedUtc: "2026-10-30T10:31:00Z", success: false, removed: 2, deferred: 0, failed: 1 } }} />);
    expect(incomplete).toContain("Last recorded cleanup:");
    expect(incomplete).toContain("Incomplete");
    expect(incomplete).toContain("Removed: 2; deferred: 0; failed: 1");
    const limited = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleRecord={{ ...record, cleanupOutcome: { attemptedUtc: "2026-10-30T10:31:00Z", success: false, removed: 2, deferred: 0, failed: 0, coverageIncomplete: true } }} />);
    expect(limited).toContain("Incomplete");
    expect(limited).toContain("Removed: 2; deferred: 0; failed: 0");
    expect(limited).toContain("Coverage was limited; some locations were not checked.");
    const unknown = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleState="unavailable" scheduleRecord={record} />);
    expect(unknown).not.toContain("Scheduled task history");
    const missingScript = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleRecord={{ ...record, payloadVerified: false }} />);
    const altered = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleRecord={{ ...record, payloadVerified: false, requiresTaskSchedulerRepair: true }} />);
    expect(altered).toContain("Review it in Windows Task Scheduler");
    expect(altered).not.toContain("Remove and recreate the schedule");
    const unknownOwner = renderToStaticMarkup(<CleanupDisplayDetails {...props} scheduleRecord={{ ...record, scope: "unknown", payloadVerified: false, requiresTaskSchedulerRepair: true }} />);
    expect(unknownOwner).toContain("Owner could not be determined");
    expect(unknownOwner).toContain("Review it in Windows Task Scheduler");
    expect(unknownOwner).not.toContain("Schedule scope:</strong> Current Windows user");
    expect(missingScript).toContain("cleanup script or Windows task settings could not be verified");
  });

  test("incomplete and failed details never render a clean result in either view", () => {
    for (const mode of ["simple", "advanced"] as const) {
      for (const state of [{ error: "denied", rawData: undefined }, { error: undefined, rawData: { status: "partial" } }]) {
        const html = renderToStaticMarkup(<CleanupDisplayDetails mode={mode} category={category} count={0} items={[]} clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} {...state} />);
        expect(html).not.toContain("No items found");
        expect(html).toContain("disabled");
      }
    }
  });

  test("shows bulk exclusion and schedule state in the casual details", () => {
    const excluded = ALL_CATEGORIES.find(item => item.id === "notepadState")!;
    const html = renderToStaticMarkup(<CleanupDisplayDetails mode="simple" category={excluded} count={0} items={[]} rawData={{ total: 0, files: [] }} excludedFromBulk scheduleState="verified" scheduleMinutes={60} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("skipped by Clear all");
    expect(html).toContain("Scheduled every 60 minutes");
    expect(html).toContain("No items found");
    const blocked = ALL_CATEGORIES.find(item => item.id === "cloudPlaceholders")!;
    const existing = renderToStaticMarkup(<CleanupDisplayDetails mode="advanced" category={blocked} count={-1} items={[]} scheduleState="verified" scheduleMinutes={60} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(existing).toContain("Enabled every 60 min; new schedules unavailable");
  });

  test("every catalog entry has safe casual and security detail views", () => {
    expect(ALL_CATEGORIES.length).toBe(84);
    for (const item of ALL_CATEGORIES) {
      expect(cleanupDisplayName(item, "simple").trim()).not.toBe("");
      expect(cleanupDisplayName(item, "advanced").trim()).not.toBe("");
      for (const mode of ["simple", "advanced"] as const) {
        const raw = item.id === "amcache" ? { total: 1, entries: [{ category: "InventoryApplication", count: 1, sample: [] }] } : { total: 1, entries: [{ privateValue: "fixture-secret" }] };
        const html = renderToStaticMarkup(<CleanupDisplayDetails mode={mode} category={item} count={1} items={["fixture-secret"]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
        const escapedName = cleanupDisplayName(item, mode).replaceAll("&", "&amp;").replaceAll("'", "&#x27;");
        expect(html).toContain(escapedName);
        if (item.id !== "amcache") expect(html).toContain("fixture-secret");
      }
    }
  });

  test("reports supplied post-clear receipts without exposing their contents", () => {
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={0} items={[]} rawData={{ total: 0, entries: [{ category: "InventoryApplication", count: 0, sample: [] }] }} verification={{ status: "cleared" }} clearing={false} clearLabel="Clear" onClose={() => {}} />);
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
