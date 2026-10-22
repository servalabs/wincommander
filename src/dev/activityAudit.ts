import type { ActivityArea, ActivityItem, ActivityRecord, ActivitySource } from "../types/generated/activity";

const removed = new Set<string>();
const kinds = [
  ["browsingHistory", "Browsing history", "Visits saved by this profile."],
  ["searchHistory", "Search history", "Search terms saved by this profile."],
  ["downloadHistory", "Download history", "Download records only; downloaded files are kept."],
  ["addressBarShortcuts", "Address-bar shortcuts", "Suggestions saved in the address bar."],
  ["cookies", "Cookies and sessions", "Optional: removes selected cookie metadata and encrypted session records."],
  ["savedLogins", "Saved sign-in entries", "Count only; related credential metadata is not supported for removal."],
] as const;

function records(itemId: string): ActivityRecord[] {
  return [1, 2].filter(index => !removed.has(`${itemId}-record-${index}`)).map(index => ({
    id: `${itemId}-record-${index}`, label: itemId.includes("searchHistory") ? `Example search ${index}` : `Example record ${index}`,
    detail: `https://example.invalid/record-${index}`, timestamp: "2026-08-01T12:00:00Z",
  }));
}

function sources(area: ActivityArea): ActivitySource[] {
  const rows: Array<[string, string, string | null]> = area === "browser" ? [
    ["chrome-default", "Google Chrome", "Default"], ["chrome-work", "Google Chrome", "Work"],
    ["edge-default", "Microsoft Edge", "Default"], ["edge-work", "Microsoft Edge", "Work"],
  ] : [["office", "Microsoft Office", null], ["adobe", "Adobe Reader", null], ["vscode", "Visual Studio Code", null]];
  return rows.map(([id, name, profile]) => {
    const types = area === "browser" ? kinds : [["recentDocuments", "Recently opened records", "Recent-file records only; documents, recovery data and trust settings are kept."]] as const;
    const items: ActivityItem[] = types.map(([kind, label, description]) => {
      const itemId = `${id}-${kind}`;
      return { id: itemId, kind, label, description, count: records(itemId).length, status: "ready", reason: kind === "savedLogins" ? "Related credential metadata handling is not supported." : null, canSelectRecords: kind !== "savedLogins", canClear: kind !== "savedLogins" };
    });
    return { id, name, profile, location: `C:\\Audit\\Activity\\${id}`, items };
  });
}

/** These synthetic records exist only in the isolated UI audit entrypoint. */
export function activityAuditResponse(command: string, args: Record<string, unknown>): unknown {
  if (!command.startsWith("activity_")) return undefined;
  const area: ActivityArea = args.area === "applications" ? "applications" : "browser";
  const itemId = String(args.itemId ?? ""), scanId = `audit-activity-${area}`;
  if (command === "activity_scan") return { scanId, userScope: "Current Windows user · Audit user", sources: sources(area), complete: true, notices: [] };
  const rows = records(itemId);
  if (command === "activity_records") return { scanId, itemId, records: rows, totalCount: rows.length, offset: 0, hasMore: false };
  if (command === "activity_remove") {
    const ids = args.allRecords ? rows.map(row => row.id) : Array.isArray(args.recordIds) ? args.recordIds : [];
    const selected = rows.filter(row => ids.includes(row.id));
    selected.forEach(row => removed.add(row.id));
    return { removedCount: selected.length, remainingCount: records(itemId).length, errors: [] };
  }
  return undefined;
}
