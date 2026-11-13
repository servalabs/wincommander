import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { getSafeCleanupPreview } from "@/components/cleanup/CleanupRecordPreview";
import { cleanupCardPreview } from "@/components/cleanup/cleanupPreviewPresentation";
import type { CleanupCategory } from "./cleanupCategories";
import { summarizeCleanupResult } from "./cleanupDisplayModel";
import type { CardData } from "./useCleanupScan";

/** Read-only monitoring retains the same scan and details handlers as the cleanup workspace. */
export default function MonitoringCard({ category, data, onScan, onViewDetails }: {
    category: CleanupCategory;
    data: CardData;
    onScan: () => void;
    onViewDetails?: () => void;
}) {
    const summary = summarizeCleanupResult({ id: category.id, count: data.count, raw: data.raw, error: data.error, loading: data.loading });
    const resultAvailable = summary.state !== "not-scanned" && summary.state !== "scanning";
    const status = summary.state === "found" && summary.count !== null
        ? `${summary.count.toLocaleString()} ${summary.count === 1 ? "process" : "processes"} observed`
        : summary.message;
    const preview = resultAvailable && summary.state === "found"
        ? cleanupCardPreview(getSafeCleanupPreview(category.id, data.raw)) : [];

    return <article className="monitoring-card" aria-label={category.label} aria-busy={data.loading}>
        <div className="monitoring-card__header">
            <span className="monitoring-card__icon"><Icon icon={category.icon} size={18} /></span>
            <div className="monitoring-card__title"><strong>{category.label}</strong><span>Read-only monitor</span></div>
            <Tooltip><TooltipTrigger asChild><button type="button" className="monitoring-card__info" aria-label={`About ${category.label}`}><Icon icon="info-sign" size={14} /></button></TooltipTrigger><TooltipContent side="bottom" className="max-w-[260px] text-xs">{category.description}</TooltipContent></Tooltip>
        </div>
        <div className="monitoring-card__body" aria-live="polite">
            <div className={`monitoring-card__status monitoring-card__status--${summary.state}`}>
                <strong>{status}</strong>
                {summary.state === "partial" || summary.state === "error" || summary.state === "unavailable" ? <span>{summary.nextStep}</span> : null}
            </div>
            {preview.length > 0 && <ul className="monitoring-card__preview" aria-label="Sample observed processes">{preview.map((item, index) => <li key={`${index}:${item}`} title={item}>{item}</li>)}</ul>}
        </div>
        <div className="monitoring-card__actions">
            <Button variant="outline" size="sm" disabled={data.loading} onClick={onScan}><Icon icon="refresh" />{data.loading ? "Scanning…" : resultAvailable ? "Rescan" : "Scan"}</Button>
            {onViewDetails && resultAvailable && <Button variant="outline" size="sm" onClick={onViewDetails}><Icon icon="eye-open" />View details</Button>}
        </div>
    </article>;
}
