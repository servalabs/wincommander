import { Icon } from "@/components/ui/bp";
import type { CleanupCategory } from "../../panels/cleanup/cleanupCategories";
import { cleanupClearExplanation, cleanupDisplayDescription, cleanupDisplayName, cleanupScheduleReason, cleanupVerificationText, isCleanupScheduleUnavailable, summarizeCleanupGroups, summarizeCleanupResult, type CleanupVerification } from "../../panels/cleanup/cleanupDisplayModel";
import type { CleanupDisplayMode } from "../../panels/cleanup/cleanupDisplay";
import CleanupScheduleControl from "./CleanupScheduleControl";

const DEFAULT_EXCLUSIONS = new Set([
  "wlanProfiles", "browserFootprints", "notepadState", "wslData", "dockerDesktopData",
  "virtualMachineArtifacts", "developerCaches", "credentialManager", "sshState", "passwordManagerCaches",
]);

export interface CleanupReadableCardProps {
  mode: CleanupDisplayMode;
  category: CleanupCategory;
  count: number;
  preview: string[];
  rawData?: unknown;
  groupedItems?: Array<{ title: string; count: number; items: string[]; rawData?: unknown }>;
  verification?: CleanupVerification;
  loading: boolean;
  clearing: boolean;
  error?: string;
  excludedFromBulk?: boolean;
  onClear: () => void;
  onClearAllUsers?: () => void;
  clearScopeLabel?: string;
  clearDisabled?: boolean;
  onViewDetails?: () => void;
  onLoad?: () => void;
  scheduleMinutes?: number | null;
  scheduleState?: "checking" | "verified" | "unavailable";
  scheduleBusy?: boolean;
  onSetSchedule?: (minutes: number) => boolean | Promise<boolean>;
  onClearSchedule?: () => boolean | Promise<boolean>;
  onRequestScheduleAccess?: () => void;
}

export default function CleanupReadableCard(props: CleanupReadableCardProps) {
  const { mode, category, count, preview, rawData, groupedItems, verification, loading, clearing, error, excludedFromBulk, onClear,
    onClearAllUsers, clearScopeLabel, clearDisabled, onViewDetails, onLoad, scheduleMinutes,
    scheduleState, scheduleBusy, onSetSchedule, onClearSchedule, onRequestScheduleAccess } = props;
  const name = cleanupDisplayName(category, mode);
  const summary = groupedItems?.length && !error && !loading ? summarizeCleanupGroups(count, groupedItems) : summarizeCleanupResult({ id: category.id, count, items: preview, raw: rawData, error, loading });
  const canClear = summary.state === "found" && !!category.clearDataKey && !clearDisabled && !clearing;
  const scope = category.systemWide ? "This device" : (clearScopeLabel ?? "Current Windows user");
  const scheduleBlocked = isCleanupScheduleUnavailable(category.id);
  const existingSchedule = scheduleState === "verified" && typeof scheduleMinutes === "number" && scheduleMinutes > 0;
  const scheduleAvailable = !!category.schedulable && !scheduleBlocked && (!!onSetSchedule || !!onRequestScheduleAccess);
  const showScheduleControl = scheduleAvailable && (scheduleState === "verified" || (!onSetSchedule && !!onRequestScheduleAccess));
  const scheduleLabel = existingSchedule ? `Scheduled every ${scheduleMinutes} min${scheduleBlocked || !category.schedulable ? "; new schedules unavailable" : ""}` :
    !category.clearDataKey ? "View only" : !scheduleAvailable ? "Schedule unavailable" :
    scheduleState === "checking" ? "Checking schedule" : scheduleState === "unavailable" ? "Schedule status unavailable" :
      scheduleState !== "verified" ? "Schedule status not verified" :
        typeof scheduleMinutes === "number" && scheduleMinutes > 0 ? `Scheduled every ${scheduleMinutes} min` : "Schedule off";

  return (
    <article className="cleanup-readable-card" data-cleanup-trace-card data-cleanup-category={category.id}
      data-cleanup-state={summary.state} aria-label={`${name}: ${summary.message}`} aria-busy={loading || clearing}>
      <div className="cleanup-readable-card__heading">
        <span className="cleanup-readable-card__icon" style={{ color: category.color }}><Icon icon={category.icon as never} size={16} /></span>
        <div><h3>{name}</h3>{mode === "advanced" && name !== category.label ? <small>{category.label}</small> : null}</div>
        <span className={`cleanup-readable-card__status cleanup-readable-card__status--${summary.state}`}>{summary.message}</span>
      </div>
      <p className="cleanup-readable-card__description">{cleanupDisplayDescription(category, mode)}</p>
      <p className="cleanup-readable-card__next">{summary.nextStep}</p>
      <div className="cleanup-readable-card__facts">
        <span><strong>Scope:</strong> {scope}</span>
        {DEFAULT_EXCLUSIONS.has(category.id) ? <span><strong>Default exclusion</strong>{excludedFromBulk ? " · skipped by Clear all" : " · check bulk settings"}</span> :
          excludedFromBulk ? <span><strong>Excluded</strong> from Clear all</span> : null}
        <span><strong>Schedule:</strong> {scheduleLabel}</span>
        {category.clearDataKey && (!category.schedulable || scheduleBlocked) ? <span>{cleanupScheduleReason(category.id)}</span> : null}
      </div>
      {category.clearDataKey ? <p className="cleanup-readable-card__warning"><strong>Clear:</strong> {cleanupClearExplanation(category, mode)}</p> : null}
      {verification ? <p className="cleanup-readable-card__next"><strong>Last result:</strong> {cleanupVerificationText(verification)}</p> : null}
      {mode === "advanced" ? <p className="cleanup-readable-card__technical">{summary.count === null ? "Count unavailable" : `${summary.count} reported`} · {summary.sampleCount} preview {category.id === "amcache" ? (summary.sampleCount === 1 ? "sample" : "samples") : (summary.sampleCount === 1 ? "row" : "rows")} · coverage {summary.state === "partial" ? "incomplete" : "not reported"}</p> : null}
      <div className="cleanup-readable-card__actions">
        {onLoad ? <button type="button" onClick={onLoad} disabled={loading} aria-label={`${summary.state === "not-scanned" ? "Scan" : "Rescan"} ${name}`}>{summary.state === "not-scanned" ? "Scan" : "Rescan"}</button> : null}
        {onViewDetails && summary.state !== "not-scanned" ? <button type="button" onClick={onViewDetails}>Details</button> : null}
        {category.clearDataKey ? <button type="button" className="cleanup-readable-card__clear" onClick={onClear} disabled={!canClear}>Clear{clearScopeLabel ? ` for ${clearScopeLabel}` : ""}</button> : null}
        {onClearAllUsers ? <button type="button" className="cleanup-readable-card__clear" onClick={onClearAllUsers} disabled={!canClear}>Clear for all users</button> : null}
        {existingSchedule && (scheduleBlocked || !category.schedulable) && onClearSchedule ? <button type="button" onClick={() => void onClearSchedule()} disabled={scheduleBusy}>Turn off schedule</button> : null}
        {showScheduleControl ? <span className="cleanup-readable-card__schedule"><span>Schedule</span><CleanupScheduleControl categoryLabel={name} minInterval={category.minIntervalMinutes ?? 5} scheduleMinutes={scheduleState === "verified" ? scheduleMinutes : null} scheduleBusy={scheduleBusy} onSetSchedule={onSetSchedule} onClearSchedule={onClearSchedule} onRequestScheduleAccess={onRequestScheduleAccess} /></span> : null}
      </div>
    </article>
  );
}
