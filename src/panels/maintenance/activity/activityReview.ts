import type { ActivityArea, ActivityItem, ActivityRecord, ActivityRecordPage, ActivityScan, ActivitySource } from "@/types/generated/activity";

export const ACTIVITY_RECORD_SELECTION_LIMIT = 500;

export interface ActivityReview {
    area: ActivityArea;
    scanId: string;
    sourceName: string;
    profile: string | null;
    location: string;
    itemId: string;
    itemLabel: string;
    itemKind: string;
    count: number;
    recordIds: string[];
    records: ActivityRecord[];
    allRecords: boolean;
    sensitive: boolean;
}

export function isSensitiveActivity(item: Pick<ActivityItem, "kind" | "label">): boolean {
    return /sign[ -]?in|log[ -]?in|cookie|session|authenticat|password|credential|account/i.test(`${item.kind} ${item.label}`);
}

export function groupActivitySources(sources: ActivitySource[]) {
    const groups = new Map<string, ActivitySource[]>();
    for (const source of sources) groups.set(source.name, [...groups.get(source.name) ?? [], source]);
    return [...groups].map(([name, profiles]) => ({ name, profiles }));
}

export function createActivityReview(area: ActivityArea, scan: ActivityScan, sourceId: string, itemId: string, loaded: ActivityRecord[], selected: Set<string>, allRecords: boolean): ActivityReview {
    const source = scan.sources.find(value => value.id === sourceId);
    const item = source?.items.find(value => value.id === itemId);
    if (!source || !item) throw new Error("This item is not part of the current scan. Scan again.");
    if (item.status !== "ready" || !item.canClear || item.count <= 0) throw new Error(item.reason ?? "This activity store cannot be cleared.");
    if (!allRecords && selected.size > ACTIVITY_RECORD_SELECTION_LIMIT) throw new Error(`Select up to ${ACTIVITY_RECORD_SELECTION_LIMIT} individual records at a time, or review clearing the whole data type.`);
    const records = loaded.filter(record => selected.has(record.id));
    if (!allRecords && (!item.canSelectRecords || !records.length || records.length !== selected.size)) throw new Error("Inspect and select available records before reviewing removal.");
    return { area, scanId: scan.scanId, sourceName: source.name, profile: source.profile, location: source.location,
        itemId, itemLabel: item.label, itemKind: item.kind, count: allRecords ? item.count : records.length,
        recordIds: allRecords ? [] : records.map(record => record.id), records: allRecords ? [] : records,
        allRecords, sensitive: isSensitiveActivity(item) };
}

export function appendActivityRecords(current: ActivityRecordPage | null, next: ActivityRecordPage, scanId: string, itemId: string, expectedOffset: number): ActivityRecordPage {
    if (next.scanId !== scanId || next.itemId !== itemId || next.offset !== expectedOffset) throw new Error("Activity changed while loading records. Scan again before making changes.");
    if (current && (current.scanId !== scanId || current.itemId !== itemId)) throw new Error("These records belong to another activity store.");
    const records = [...new Map([...(current?.records ?? []), ...next.records].map(record => [record.id, record])).values()];
    return { ...next, offset: 0, records };
}
