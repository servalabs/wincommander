import { Button, Icon } from "@/components/ui/bp";
import type { ActivityItem } from "@/types/generated/activity";
import { groupActivitySources, isSensitiveActivity } from "./activityReview";
import type { MaintenanceActivity } from "./useMaintenanceActivity";
import ActivityRecordInspector from "./ActivityRecordInspector";
import { appIcons } from "@/assets/apps";
import { browserLogos } from "@/assets/privacy";
import { resolveBrowserIconUrl } from "@/panels/privacy/browserIcons";

function sourceIcon(name: string): string | undefined {
    return resolveBrowserIconUrl(name, browserLogos)
        ?? (name === "Visual Studio Code" ? appIcons["vscode.svg"] : /Adobe/.test(name) ? appIcons["acrobat-reader.svg"] : undefined);
}

function itemIcon(item: ActivityItem) {
    return isSensitiveActivity(item) ? "lock" as const : /download/i.test(item.kind) ? "download" as const : /search/i.test(item.kind) ? "search" as const : "history" as const;
}

export default function ActivitySourceList({ activity }: { activity: MaintenanceActivity }) {
    const groups = groupActivitySources(activity.scan?.sources ?? []);
    return <div className="activity-source-list">{groups.map(group => <section className="activity-app" key={group.name} aria-label={`${group.name} activity`}>
        <header className="activity-app-heading"><span className="activity-app-icon">{sourceIcon(group.name) ? <img src={sourceIcon(group.name)} alt="" width={23} height={23} /> : <Icon icon={activity.area === "browser" ? "globe" : "applications"} size={23} />}</span><div><h3>{group.name}</h3><p>{group.profiles.length.toLocaleString()} detected {group.profiles.length === 1 ? "source" : "sources"}</p></div></header>
        {group.profiles.map(source => <details className="activity-profile" key={source.id} open>
            <summary><Icon icon="user" size={15} /><span>{source.profile ?? source.name}</span><small>{source.items.filter(item => item.status === "ready").length} available data types</small><Icon icon="chevron-down" size={14} /></summary>
            <div className="activity-profile-location" title={source.location}>{source.location || "Location unavailable"}</div>
            <div className="activity-items">{source.items.map(item => {
                const inspected = activity.inspected?.source.id === source.id && activity.inspected.item.id === item.id;
                const ready = item.status === "ready";
                return <div key={item.id} className={`activity-item${inspected ? " activity-item-active" : ""}`}>
                    <div className="activity-item-row"><Icon icon={itemIcon(item)} size={20} /><div className="activity-item-copy"><strong>{item.label}{isSensitiveActivity(item) && <span className="activity-sensitive">Sensitive · optional</span>}</strong><p>{item.description}</p>
                        {/download/i.test(item.kind) && <small>Clearing download history removes browser records, not downloaded files.</small>}
                        {!ready && <small className="activity-unavailable">{item.reason ?? "This store is unavailable."}</small>}
                        {ready && !item.canClear && <small>{item.reason ?? (item.count === 0 ? "No records to clear." : "Read-only: clearing this data type is unavailable.")}</small>}
                    </div><span className="activity-item-count">{ready ? item.count.toLocaleString() : "Unavailable"}<small>{ready ? "scanned records" : ""}</small></span>
                    <div className="activity-item-actions"><Button small minimal icon="eye-open" disabled={activity.removing || activity.scanning || !ready || !item.canSelectRecords || item.count <= 0} aria-label={`${inspected ? "Hide" : "Inspect"} ${item.label} in ${source.profile ?? source.name}`} onClick={() => inspected ? activity.closeInspection() : void activity.inspect(source, item)}>{inspected ? "Hide" : "Inspect"}</Button>
                        <Button small outlined intent="danger" disabled={activity.removing || activity.scanning || activity.readOnly || !ready || !item.canClear || item.count <= 0} aria-label={`Review clearing all ${item.label} in ${source.profile ?? source.name}`} onClick={() => activity.openReview(source, item, true)}>Review all</Button></div>
                    </div>{inspected && <ActivityRecordInspector activity={activity} />}
                </div>;
            })}{!source.items.length && <p className="activity-profile-empty">No supported activity stores were found for this source.</p>}</div>
        </details>)}
    </section>)}</div>;
}
