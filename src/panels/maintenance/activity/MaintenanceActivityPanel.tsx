import { Button, Icon, Spinner } from "@/components/ui/bp";
import type { ActivityArea } from "@/types/generated/activity";
import { useMaintenanceActivity } from "./useMaintenanceActivity";
import ActivitySourceList from "./ActivitySourceList";
import ActivityReviewDialog from "./ActivityReviewDialog";
import ActivityLegacySummary from "./ActivityLegacySummary";
import ActivityLegacySchedules from "./ActivityLegacySchedules";
import "./MaintenanceActivity.css";
import useEntitlements from "@/hooks/useEntitlements";
import { useAuthMode } from "@/context/AuthModeContext";

export default function MaintenanceActivityPanel({ initialArea = "browser" }: { initialArea?: ActivityArea }) {
    const { isInvestigator } = useEntitlements();
    const { mode: authMode } = useAuthMode();
    const activity = useMaintenanceActivity(initialArea, isInvestigator || authMode === "decoy");
    const areas: { id: ActivityArea; label: string }[] = [{ id: "browser", label: "Browser Activity" }, { id: "applications", label: "Application Activity" }];
    function moveTab(key: string) {
        if (activity.removing || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(key)) return false;
        const next = key === "Home" ? "browser" : key === "End" ? "applications" : activity.area === "browser" ? "applications" : "browser";
        activity.changeArea(next);
        requestAnimationFrame(() => document.getElementById(`activity-tab-${next}`)?.focus());
        return true;
    }
    if (authMode === "decoy") return <p role="status">Activity review is unavailable in Decoy mode.</p>;
    const sources = activity.scan?.sources ?? [];
    const readyItems = sources.flatMap(source => source.items).filter(item => item.status === "ready");
    return <div className="maintenance-activity">
        <header className="activity-heading"><div><span className="activity-eyebrow">CURRENT WINDOWS USER</span><h2>Activity</h2><p>Review browser and application records before clearing them. Storage caches stay in Deep Cleanup.</p></div>
            <Button icon="refresh" intent="primary" disabled={activity.scanning || activity.removing} onClick={activity.refresh}>{activity.scanning ? "Scanning…" : "Scan again"}</Button></header>
        <div className="activity-tabs" role="tablist" aria-label="Activity area">{areas.map(area => <button key={area.id} type="button" role="tab" id={`activity-tab-${area.id}`} aria-selected={activity.area === area.id} aria-controls="maintenance-activity-content" tabIndex={activity.area === area.id ? 0 : -1} disabled={activity.removing}
            onClick={() => activity.changeArea(area.id)} onKeyDown={event => { if (moveTab(event.key)) event.preventDefault(); }}>{area.label}</button>)}</div>
        {isInvestigator && <p className="activity-notice" role="status">Investigator mode: records can be inspected; removal is disabled.</p>}
        <div role="tabpanel" id="maintenance-activity-content" aria-labelledby={`activity-tab-${activity.area}`} aria-busy={activity.scanning || activity.removing} className="activity-content">
            {activity.result && <div className="activity-result" role="status"><Icon icon={activity.result.errors.length ? "warning-sign" : "tick-circle"} size={18} /><div><strong>{activity.result.removedCount.toLocaleString()} records removed · {activity.resultLabel}</strong><p>{activity.result.remainingCount.toLocaleString()} records remain in that data type. {activity.scanning ? "Refreshing the scan…" : activity.scan ? "The scan has been refreshed." : "Scan again before making another change."}</p>{activity.result.errors.map((error, index) => <p key={index}>{error}</p>)}</div></div>}
            {activity.operationError && <p className="activity-error" role="alert">{activity.operationError}</p>}
            {activity.error && <div className="activity-error" role="alert"><strong>Activity scan unavailable</strong><p>{activity.error}</p></div>}
            {activity.scanning ? <div className="activity-empty" role="status"><Spinner size={30} /><h3>Finding activity stores</h3><p>Reading supported browser profiles and application records for this Windows user.</p></div> : activity.scan ? <>
                <div className="activity-scan-summary"><span><Icon icon="user" size={16} />{activity.scan.userScope}</span><span>{sources.length.toLocaleString()} sources · {readyItems.length.toLocaleString()} available data types</span></div>
                {!activity.scan.complete && <p className="activity-notice" role="status">Partial scan. Unavailable or unsupported stores are not included in the available-record totals.</p>}
                {activity.scan.notices.map((notice, index) => <p className="activity-notice" role="status" key={index}>{notice}</p>)}
                {sources.length ? <ActivitySourceList activity={activity} /> : <div className="activity-empty"><Icon icon="history" size={35} /><h3>No supported activity stores found</h3><p>No activity records are selected or changed.</p></div>}
            </> : null}
            {!activity.scanning && activity.area === "browser" && activity.error && <ActivityLegacySummary />}
            <ActivityLegacySchedules />
        </div>
        <ActivityReviewDialog activity={activity} />
    </div>;
}
