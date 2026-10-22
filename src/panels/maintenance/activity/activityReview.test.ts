import { expect, test } from "bun:test";
import type { ActivityItem, ActivityRecord, ActivityRecordPage, ActivityScan } from "@/types/generated/activity";
import { appendActivityRecords, createActivityReview, groupActivitySources, isSensitiveActivity } from "./activityReview";

const item: ActivityItem = { id: "history-1", kind: "history", label: "Browsing history", description: "Visited pages", count: 9, status: "ready", reason: null, canSelectRecords: true, canClear: true };
const scan: ActivityScan = { scanId: "current-scan", userScope: "Current Windows user", complete: true, notices: [], sources: [{ id: "chrome-default", name: "Chrome", profile: "Default", location: "C:\\Example\\Default", items: [item] }] };
const records: ActivityRecord[] = [{ id: "record-1", label: "Page one", detail: null, timestamp: null }, { id: "record-2", label: "Page two", detail: null, timestamp: null }];

function errorMessage(action: () => unknown): string { try { action(); return ""; } catch (cause) { return String(cause); } }

function review(selected: Set<string>, all = false, state = scan) { return createActivityReview("browser", state, "chrome-default", "history-1", records, selected, all); }

test("selected-record review contains only exact loaded IDs and its scanned source", () => {
    const result = review(new Set(["record-2"]));
    expect(result.recordIds).toEqual(["record-2"]);
    expect(result.records.map(record => record.label)).toEqual(["Page two"]);
    expect([result.scanId, result.profile, result.count, result.allRecords]).toEqual(["current-scan", "Default", 1, false]);
});

test("empty selection and IDs not loaded for inspection cannot become removal plans", () => {
    expect(errorMessage(() => review(new Set()))).toContain("Inspect and select");
    expect(errorMessage(() => review(new Set(["record-1", "unloaded-id"])))).toContain("Inspect and select");
});

test("whole-type review is explicit and carries no individual IDs or private record details", () => {
    const result = review(new Set(["record-2"]), true);
    expect(result.recordIds).toEqual([]);
    expect(result.records).toEqual([]);
    expect(result.count).toBe(9);
    expect(result.allRecords).toBe(true);
});

test("stale source IDs and unavailable or read-only stores cannot produce removal plans", () => {
    expect(errorMessage(() => createActivityReview("browser", scan, "another-source", item.id, records, new Set(["record-1"]), false))).toContain("current scan");
    for (const [replacement, expected] of [[{ ...item, status: "unavailable", reason: "Profile locked" }, "Profile locked"], [{ ...item, canClear: false }, "cannot be cleared"], [{ ...item, canSelectRecords: false }, "Inspect and select"]] as const) {
        const state = { ...scan, sources: [{ ...scan.sources[0], items: [replacement] }] };
        expect(errorMessage(() => review(new Set(["record-1"]), false, state))).toContain(expected);
    }
});

test("whole-store-only types cannot use per-record selection but remain reviewable as a whole", () => {
    const state = { ...scan, sources: [{ ...scan.sources[0], items: [{ ...item, canSelectRecords: false }] }] };
    expect(review(new Set(), true, state).allRecords).toBe(true);
});

test("sign-in and session data are explicitly sensitive while ordinary history remains separate", () => {
    expect(isSensitiveActivity(item)).toBe(false);
    expect(isSensitiveActivity({ kind: "sign-in-records", label: "Saved accounts" })).toBe(true);
    expect(isSensitiveActivity({ kind: "cookies", label: "Cookies and site sessions" })).toBe(true);
    expect(isSensitiveActivity({ kind: "accounts", label: "Saved accounts" })).toBe(true);
});

test("profiles group under their browser without merging individual profile identities", () => {
    const sources = [scan.sources[0], { ...scan.sources[0], id: "chrome-work", profile: "Profile 1" }, { ...scan.sources[0], id: "edge", name: "Edge" }];
    const groups = groupActivitySources(sources);
    expect(groups.map(group => [group.name, group.profiles.length])).toEqual([["Chrome", 2], ["Edge", 1]]);
    expect(groups[0].profiles.map(profile => profile.id)).toEqual(["chrome-default", "chrome-work"]);
});

const page: ActivityRecordPage = { scanId: scan.scanId, itemId: item.id, records: [records[0]], totalCount: 9, offset: 0, hasMore: true };

test("record pages from another scan, store, or position are rejected", () => {
    expect(errorMessage(() => appendActivityRecords(null, { ...page, scanId: "expired" }, scan.scanId, item.id, 0))).toContain("Activity changed");
    expect(errorMessage(() => appendActivityRecords(null, { ...page, itemId: "other-store" }, scan.scanId, item.id, 0))).toContain("Activity changed");
    expect(errorMessage(() => appendActivityRecords(null, { ...page, offset: 10 }, scan.scanId, item.id, 0))).toContain("Activity changed");
});

test("loaded records append once while preserving honest backend total and paging status", () => {
    const result = appendActivityRecords(page, { ...page, offset: 1, records, hasMore: true }, scan.scanId, item.id, 1);
    expect(result.records.map(record => record.id)).toEqual(["record-1", "record-2"]);
    expect(result.totalCount).toBe(9);
    expect(result.hasMore).toBe(true);
});

test("selected-record plans enforce the same 500-record cap as the backend", () => {
    const many = Array.from({ length: 501 }, (_, index) => ({ id: `record-${index}`, label: `Record ${index}`, detail: null, timestamp: null }));
    const state = { ...scan, sources: [{ ...scan.sources[0], items: [{ ...item, count: 501 }] }] };
    const plan = createActivityReview("browser", state, "chrome-default", item.id, many, new Set(many.slice(0, 500).map(record => record.id)), false);
    expect(plan.recordIds.length).toBe(500);
    expect(errorMessage(() => createActivityReview("browser", state, "chrome-default", item.id, many, new Set(many.map(record => record.id)), false))).toContain("up to 500");
    expect(createActivityReview("browser", state, "chrome-default", item.id, many, new Set(), true).count).toBe(501);
});
