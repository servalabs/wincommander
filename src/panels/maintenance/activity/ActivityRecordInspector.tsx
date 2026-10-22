import { Button, CheckboxControl, Spinner } from "@/components/ui/bp";
import { ACTIVITY_RECORD_SELECTION_LIMIT } from "./activityReview";
import type { MaintenanceActivity } from "./useMaintenanceActivity";

export default function ActivityRecordInspector({ activity }: { activity: MaintenanceActivity }) {
    const { inspected, page, selected, recordsLoading, recordsError, removing } = activity;
    if (!inspected) return null;
    const { source, item } = inspected;
    return <section className="activity-inspector" aria-label={`${item.label} records in ${source.profile ?? source.name}`}>
        <div className="activity-inspector-heading"><strong>{item.label}</strong><span>{source.profile ?? source.name}</span><Button small minimal icon="cross" aria-label="Hide inspected activity records" disabled={removing} onClick={activity.closeInspection} /></div>
        <p>These details may include private activity. They are shown only after you choose Inspect.</p>
        {item.status !== "ready" ? <p role="status">{item.reason ?? "This store is unavailable."}</p> : !item.canSelectRecords ? <p>Individual records are unavailable for this store. You can review clearing the whole data type when supported.</p> : <>
            <p>Review up to {ACTIVITY_RECORD_SELECTION_LIMIT} individual records at a time. “Review all” above covers the entire data type, including unloaded records.</p>
            {recordsError && <p className="activity-error" role="alert">{recordsError}</p>}
            {page && <div className="activity-record-toolbar"><span>{page.records.length.toLocaleString()} of {page.totalCount.toLocaleString()} records loaded · {selected.size.toLocaleString()} selected</span><div>
                <Button small minimal disabled={activity.readOnly || removing || recordsLoading || !page.records.length || !item.canClear} onClick={activity.selectLoaded}>Select up to {ACTIVITY_RECORD_SELECTION_LIMIT} loaded records</Button>
                <Button small minimal disabled={activity.readOnly || removing || !selected.size} onClick={activity.clearSelection}>Clear selection</Button>
            </div></div>}
            <div className="activity-record-list" aria-label="Inspected records">{page?.records.map(record => <label className="activity-record" key={record.id}>
                <CheckboxControl checked={selected.has(record.id)} disabled={activity.readOnly || removing || !item.canClear} ariaLabel={`Select ${record.label}`} onChange={() => activity.toggleRecord(record.id)} />
                <span><strong>{record.label || "Unnamed record"}</strong>{record.detail && <small>{record.detail}</small>}{record.timestamp && <time>{record.timestamp}</time>}</span>
            </label>)}</div>
            {recordsLoading && <div className="activity-loading" role="status"><Spinner size={18} />Loading records…</div>}
            {page && !page.records.length && !recordsLoading && <p>No records were returned by this scan.</p>}
            <div className="activity-record-actions">{page?.hasMore && <Button small outlined disabled={activity.readOnly || removing || recordsLoading} onClick={() => void activity.loadMore()}>Load more records</Button>}
                <Button small intent="danger" disabled={activity.readOnly || removing || recordsLoading || !item.canClear || !selected.size} onClick={() => activity.openReview(source, item, false)}>Review {selected.size.toLocaleString()} selected records</Button>
            </div>
        </>}
    </section>;
}
