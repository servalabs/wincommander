import { useState } from "react";
import { Button, Dialog, CheckboxControl } from "@/components/ui/bp";
import type { ActivityReview } from "./activityReview";
import type { MaintenanceActivity } from "./useMaintenanceActivity";

export default function ActivityReviewDialog({ activity }: { activity: MaintenanceActivity }) {
    const { review, removing } = activity;
    const [acknowledgedReview, setAcknowledgedReview] = useState<ActivityReview | null>(null);
    const acknowledged = review !== null && acknowledgedReview === review;
    return <Dialog isOpen={Boolean(review)} icon="trash" title="Review activity removal" onClose={activity.cancelReview} className="activity-review-dialog">
        {review && <div className="activity-review"><dl><dt>Source</dt><dd>{review.sourceName}{review.profile ? ` · ${review.profile}` : ""}</dd><dt>Data type</dt><dd>{review.itemLabel}</dd><dt>Location</dt><dd>{review.location || "Unavailable"}</dd><dt>Selection</dt><dd>{review.allRecords ? `All ${review.count.toLocaleString()} scanned records of this type` : `${review.count.toLocaleString()} selected records`}</dd></dl>
            {review.allRecords ? <p>This clears this entire data type in the selected profile, including records you have not loaded for inspection.</p> : <ul className="activity-review-records">{review.records.map(record => <li key={record.id}>{record.label}</li>)}</ul>}
            {/download/i.test(review.itemKind) && <p>Downloaded files are kept. Only download-history records are removed.</p>}
            <p>This cannot be undone. After the action, WinCommander scans again before offering another change.</p>
            {review.sensitive && <label className="activity-sensitive-confirm"><CheckboxControl checked={acknowledged} disabled={removing} ariaLabel="Acknowledge clearing sensitive sign-in activity" onChange={() => setAcknowledgedReview(current => current === review ? null : review)} /><span>I understand this can sign me out and remove the selected sign-in or session records.</span></label>}
            <div className="activity-review-actions"><Button minimal disabled={removing} onClick={activity.cancelReview}>Keep records</Button><Button intent="danger" loading={removing} disabled={activity.readOnly || removing || review.sensitive && !acknowledged} onClick={() => { if (!review.sensitive || acknowledged) void activity.confirmRemoval(); }}>Clear reviewed records</Button></div>
        </div>}
    </Dialog>;
}
