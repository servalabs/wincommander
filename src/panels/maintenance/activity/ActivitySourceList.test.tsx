import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import ActivitySourceList from "./ActivitySourceList";
import type { MaintenanceActivity } from "./useMaintenanceActivity";
import type { ActivityItem, ActivitySource } from "@/types/generated/activity";

const item: ActivityItem = { id: "history", kind: "history", label: "Browsing history", description: "Visited pages", count: 1, status: "ready", reason: null, canSelectRecords: true, canClear: true };
const source: ActivitySource = { id: "profile", name: "Chrome", profile: "Default", location: "C:\\Example\\Default", items: [item] };
function state(overrides: Partial<MaintenanceActivity> = {}): MaintenanceActivity {
    return { area: "browser", readOnly: false, scan: { scanId: "scan", userScope: "Current user", sources: [source], complete: true, notices: [] }, scanning: false, error: "", inspected: null,
        page: { scanId: "scan", itemId: "history", records: [{ id: "one", label: "https://private.example/search?q=personal", detail: "Private search term", timestamp: null }], totalCount: 1, offset: 0, hasMore: false },
        recordsLoading: false, recordsError: "", selected: new Set(), review: null, removing: false, result: null, resultLabel: "", operationError: "",
        refresh() {}, changeArea() {}, async inspect() {}, async loadMore() {}, openReview() {}, async confirmRemoval() {}, closeInspection() {}, cancelReview() {}, toggleRecord() {}, clearSelection() {}, selectLoaded() {}, ...overrides };
}

test("source summaries do not reveal loaded URLs or search records before explicit inspection", () => {
    const html = renderToStaticMarkup(<ActivitySourceList activity={state()} />);
    expect(html).toContain("Chrome");
    expect(html).toContain("Default");
    expect(html).not.toContain("private.example");
    expect(html).not.toContain("Private search term");
});

test("explicit inspection reveals records as plain text and leaves every record unselected", () => {
    const html = renderToStaticMarkup(<ActivitySourceList activity={state({ inspected: { source, item } })} />);
    expect(html).toContain("private.example");
    expect(html).toContain("Private search term");
    expect(html).not.toContain("href=");
    expect(html).toContain('data-state="unchecked"');
    expect(html).not.toContain('data-state="checked"');
});

test("unavailable stores explain their reason and expose no enabled clearing action", () => {
    const locked = { ...item, status: "unavailable", reason: "Profile locked", canClear: false };
    const html = renderToStaticMarkup(<ActivitySourceList activity={state({ scan: { scanId: "scan", userScope: "Current user", sources: [{ ...source, items: [locked] }], complete: false, notices: [] } })} />);
    expect(html).toContain("Profile locked");
    const button = html.match(/<button[^>]*aria-label="Review clearing all Browsing history in Default"[^>]*>/)?.[0];
    expect(button).toContain("disabled");
});

test("optional sign-in records are visibly sensitive without preselecting or exposing account records", () => {
    const sensitive = { ...item, kind: "sign-in", label: "Sign-in records" };
    const html = renderToStaticMarkup(<ActivitySourceList activity={state({ scan: { scanId: "scan", userScope: "Current user", sources: [{ ...source, items: [sensitive] }], complete: true, notices: [] } })} />);
    expect(html).toContain("Sensitive · optional");
    expect(html).not.toContain('data-state="checked"');
    expect(html).not.toContain("private.example");
});

test("count-only sources show measured counts and explain why record actions are unavailable", () => {
    const countOnly = { ...item, kind: "savedLogins", label: "Saved sign-ins", count: 7, canSelectRecords: false, canClear: false, reason: "Related credential metadata is unsupported." };
    const html = renderToStaticMarkup(<ActivitySourceList activity={state({ scan: { scanId: "scan", userScope: "Current user", sources: [{ ...source, items: [countOnly] }], complete: true, notices: [] } })} />);
    expect(html).toContain("Related credential metadata is unsupported.");
    expect(html).toContain("7");
    expect(html.match(/<button[^>]*aria-label="Inspect Saved sign-ins in Default"[^>]*>/)?.[0]).toContain("disabled");
    expect(html.match(/<button[^>]*aria-label="Review clearing all Saved sign-ins in Default"[^>]*>/)?.[0]).toContain("disabled");
});
