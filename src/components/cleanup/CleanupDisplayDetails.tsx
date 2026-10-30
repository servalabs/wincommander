import { Button, Icon } from "@/components/ui/bp";
import { useEffect, useMemo, useRef } from "react";
import type { CleanupCategory } from "../../panels/cleanup/cleanupCategories";
import type { CleanupScheduleRecord } from "../../panels/cleanup/cleanupScheduleReadback";
import type { CleanupDisplayMode } from "../../panels/cleanup/cleanupDisplay";
import { isManualOnlyCleanupSchedule } from "../../panels/cleanup/cleanupManualPolicy";
import { getCleanupScanFacts } from "../../panels/cleanup/cleanupScanFacts";
import CleanupRecordPreview, { getSafeCleanupPreview } from "./CleanupRecordPreview";
import { cleanupClearExplanation, cleanupCountUnit, cleanupDisplayDescription, cleanupDisplayName, cleanupVerificationText, hasLimitedCleanupCoverage, summarizeCleanupGroups, summarizeCleanupResult, type CleanupVerification } from "../../panels/cleanup/cleanupDisplayModel";

type DetailCategory = Pick<CleanupCategory, "id" | "label" | "description" | "icon" | "severity" | "systemWide" | "scopeAware" | "clearDataKey" | "confirmMessage" | "regeneratesNote" | "schedulable">;

export default function CleanupDisplayDetails({ category, mode, count, items, rawData, groupedItems, error, clearing, clearDisabled, clearLabel, onClose, onClear, scheduleState, scheduleMinutes, scheduleDisabled, scheduleRecord, verification, scopeLabel, accountLabel, accountName, excludedFromBulk = false }: {
  category: DetailCategory; mode: CleanupDisplayMode; count: number; items: string[]; rawData?: unknown; error?: string;
  clearing: boolean; clearDisabled?: boolean; clearLabel: string; onClose: () => void; onClear?: () => void;
  scheduleState?: "checking" | "verified" | "unavailable"; scheduleMinutes?: number | null;
  scheduleDisabled?: boolean;
  scheduleRecord?: CleanupScheduleRecord;
  excludedFromBulk?: boolean;
  verification?: CleanupVerification;
  groupedItems?: Array<{ title: string; username?: string; count: number; items: string[]; rawData?: unknown }>;
  scopeLabel?: string;
  accountLabel?: string;
  accountName?: string;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    dialog?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); closeRef.current(); return; }
      if (event.key !== "Tab" || !dialog) return;
      const controls = Array.from(dialog.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), summary, [href], [tabindex]:not([tabindex="-1"])'))
        .filter(element => element.getClientRects().length > 0);
      if (!controls.length) { event.preventDefault(); dialog.focus(); return; }
      const index = controls.indexOf(document.activeElement as HTMLElement);
      if (event.shiftKey && index <= 0) { event.preventDefault(); controls[controls.length - 1].focus(); }
      else if (!event.shiftKey && (index === -1 || index === controls.length - 1)) { event.preventDefault(); controls[0].focus(); }
    };
    document.addEventListener("keydown", handleKey);
    return () => { document.removeEventListener("keydown", handleKey); previous?.focus(); };
  }, []);
  const summary = groupedItems?.length && !error ? summarizeCleanupGroups(count, groupedItems) : summarizeCleanupResult({ id: category.id, count, items, raw: rawData, error });
  const previewData = useMemo(() => getSafeCleanupPreview(category.id, rawData).length || !items.length
    ? rawData : { records: items }, [category.id, rawData, items]);
  const name = cleanupDisplayName(category, mode);
  const raw = rawData && typeof rawData === "object" && !Array.isArray(rawData) ? rawData as Record<string, unknown> : undefined;
  const scanFacts = getCleanupScanFacts(rawData, summary.state === "error");
  const groupedLimited = groupedItems?.some(group => hasLimitedCleanupCoverage(category.id, group.rawData));
  const limitedScope = hasLimitedCleanupCoverage(category.id, raw) || groupedLimited ? (category.id === "walFiles"
    ? "Checks two app-data locations, up to five folder levels deep."
    : category.id === "recallDb" ? "Checks four specified activity-data locations."
      : category.id === "dockerDesktopData" ? "Checks default Docker disk and log locations; custom disk locations are not included."
        : category.id === "srumData" ? "Checks database file metadata; historical usage records are not decoded."
        : category.id === "searchIndex" ? "Checks selected Windows Search index and log files; other Search data is outside this scan." : "Checks selected locations only.") : null;
  const coverage = summary.state === "partial" ? "Incomplete" : limitedScope ? "Limited to selected locations" : raw?.coverage === "complete" ? "Reported complete" : typeof raw?.coverage === "string" && raw.coverage.trim() ? raw.coverage : "Not reported";
  const receipt = cleanupVerificationText(verification);
  const verifiedAt = verification?.verifiedAt && Number.isFinite(Date.parse(verification.verifiedAt)) ? new Date(verification.verifiedAt).toLocaleString() : null;
  const dateValue = typeof raw?.scannedAt === "string" ? raw.scannedAt : typeof raw?.scanTime === "string" ? raw.scanTime : "";
  const scanTime = dateValue && Number.isFinite(Date.parse(dateValue)) ? new Date(dateValue).toLocaleString() : "Not reported";
  const taskStatus = scheduleState === "checking" ? "Checking" : scheduleState !== "verified" || (scheduleRecord && scheduleRecord.payloadVerified !== true) ? "Not verified" :
    scheduleDisabled ? "Disabled" : scheduleMinutes ? `Enabled every ${scheduleMinutes} min${!category.schedulable ? "; new schedules unavailable" : ""}` :
      category.schedulable ? "Off" : "Not available for this card";
  const simpleScheduleStatus = scheduleState === "verified" && scheduleRecord?.requiresTaskSchedulerRepair ? "This task has changed and cannot be safely managed here. Review it in Windows Task Scheduler."
    : scheduleState === "verified" && scheduleRecord && scheduleRecord.payloadVerified !== true ? `${scheduleDisabled ? "A disabled task exists" : "A task exists"}, but its cleanup script or Windows task settings could not be verified. Remove and recreate the schedule.`
    : scheduleState === "verified" && scheduleDisabled ? "An existing schedule is disabled. It will not run."
    : scheduleState === "verified" && scheduleMinutes
    ? `Scheduled every ${scheduleMinutes} minutes${!category.schedulable ? ". New schedules are unavailable for this cleanup." : "."}`
    : scheduleState === "verified" ? (category.schedulable ? "No automatic cleanup is scheduled." : "Scheduling is not available for this cleanup.")
      : scheduleState === "checking" ? "Checking whether automatic cleanup is scheduled."
        : "Schedule status could not be verified.";
  const parsedLastRun = scheduleRecord?.lastRun ? new Date(scheduleRecord.lastRun) : null;
  const hasLastRun = parsedLastRun && Number.isFinite(parsedLastRun.getTime()) && parsedLastRun.getFullYear() >= 2000;
  const lastRun = hasLastRun ? parsedLastRun.toLocaleString() : "No run reported";
  const lastResult = scheduleRecord?.lastResult;
  const resultText = lastResult === null || lastResult === undefined ? "Not reported"
    : lastResult === 0x41303 ? "Windows reports that the task has not run"
    : lastResult === 0 ? (hasLastRun ? "Windows reports the task completed (0)" : "Windows result 0; run time unavailable")
    : `Windows task result: ${lastResult} (0x${(lastResult >>> 0).toString(16).toUpperCase()})`;
  return (
    <div ref={dialogRef} tabIndex={-1} className="trace-dialog" role="dialog" aria-modal="true" aria-label={`${name} details`}>
      <div className="trace-dialog__panel cleanup-readable-detail">
        <header className="trace-dialog__header">
          <div className={`trace-dialog__icon trace-dialog__icon--${category.severity}`}><Icon icon={category.icon as never} size={18} /></div>
          <div><h2>{name}</h2><p>{cleanupDisplayDescription(category, mode)}</p></div>
          <div className="trace-dialog__actions"><Button icon="cross" minimal aria-label={`Dismiss ${name} details`} onClick={onClose} /></div>
        </header>
        <div className={`cleanup-readable-detail__result cleanup-readable-detail__result--${summary.state}`} role="status">
          <strong>{groupedItems?.length && summary.state === "found" && summary.count !== null ? `${summary.count} ${cleanupCountUnit(category.id, summary.count)} reported` : summary.message}</strong><span>{summary.nextStep}</span>
        </div>
        <div className="cleanup-readable-detail__body">
          {summary.state === "inconsistent" ? <p>The total and category counts disagree. The result cannot be confirmed until a new scan succeeds.</p> : null}
          {summary.state === "partial" ? <p>Some sources were not checked. The result is incomplete.</p> : null}
          {limitedScope ? <p><strong>Scan scope:</strong> {limitedScope}</p> : null}
          {groupedItems?.length ? <p>Only profiles detected in the supported Windows user folders are shown. Other profiles may not appear.</p> : null}
          {!groupedItems?.length && scanFacts.length ? <section aria-label="Scan details"><h3>Scan details</h3><dl className="cleanup-readable-detail__facts">{scanFacts.map((fact, index) => <div key={`${fact.label}-${index}`}><dt>{fact.label}</dt><dd>{fact.value}</dd></div>)}</dl></section> : null}
          {!groupedItems?.length && (["found", "partial", "limited", "inconsistent"].includes(summary.state) || getSafeCleanupPreview(category.id, previewData).length > 0 || (!!accountName && raw?.truncated === true)) ? <CleanupRecordPreview categoryId={category.id} rawData={previewData} mode={mode} accountLabel={accountLabel} accountName={accountName} /> : null}
          {summary.count !== null && summary.count > 0 && summary.sampleCount === 0 && !groupedItems?.length && !getSafeCleanupPreview(category.id, previewData).length ? <p>No preview samples were supplied. The scan still reported {summary.count} {cleanupCountUnit(category.id, summary.count)}.</p> : null}
          {summary.count !== null && summary.count > 0 && summary.sampleCount !== 0 && !groupedItems?.length && getSafeCleanupPreview(category.id, previewData).length === 0 ? <p>No preview details are available; the scan total remains above.</p> : null}
          {category.clearDataKey ? <p className="cleanup-readable-detail__warning"><strong>What Clear does:</strong> {cleanupClearExplanation(category, mode)}</p> : null}
          {excludedFromBulk ? <p><strong>Bulk cleanup:</strong> This card is currently skipped by Clear all.</p> : null}
          {category.schedulable && isManualOnlyCleanupSchedule(category.id) ? <p><strong>Auto-set:</strong> This card is skipped. You can schedule it individually from the card.</p> : null}
          <div className="cleanup-readable-detail__context"><span><strong>Scope:</strong> {scopeLabel ?? (groupedItems?.length ? "Reported Windows accounts" : category.systemWide ? "This device" : category.scopeAware ? "Selected Windows user" : "Current context")}</span><span><strong>Schedule:</strong> {simpleScheduleStatus}</span></div>
          {scheduleState === "verified" && scheduleRecord ? <section aria-label="Scheduled task history"><h3>Scheduled task history</h3>
            <p><strong>Schedule scope:</strong> {scheduleRecord.scope === "system" ? "This device" : "Current Windows user"}</p><p><strong>Last run:</strong> {lastRun}</p><p><strong>Windows result:</strong> {resultText}</p>
            {scheduleRecord.cleanupOutcome ? <p><strong>Last recorded cleanup:</strong> {scheduleRecord.cleanupOutcome.success ? "Completed" : "Incomplete"} at {new Date(scheduleRecord.cleanupOutcome.attemptedUtc).toLocaleString()}. Removed: {scheduleRecord.cleanupOutcome.removed}; deferred: {scheduleRecord.cleanupOutcome.deferred}; failed: {scheduleRecord.cleanupOutcome.failed}.</p> : <p><strong>Cleanup outcome:</strong> No verified result is available for this schedule.</p>}
            <p>A saved schedule and Windows task result do not confirm which items were removed. Rescan this card to check.</p>
          </section> : null}
          {mode === "simple" && verification ? <p><strong>Last check:</strong> {receipt}</p> : null}
          {category.regeneratesNote ? <p>{category.regeneratesNote}</p> : null}
          {mode === "advanced" ? <>
            <details className="cleanup-readable-detail__disclosure"><summary>Scan information and verification</summary><dl className="cleanup-readable-detail__facts">
              <div><dt>Source</dt><dd>{cleanupDisplayDescription(category, "advanced")}</dd></div>
              <div><dt>Scope</dt><dd>{scopeLabel ?? (groupedItems?.length ? "Reported Windows accounts" : category.systemWide ? "This device" : category.scopeAware ? "Selected Windows user" : "Current context")}</dd></div>
              <div><dt>Scan time</dt><dd>{scanTime}</dd></div>
              <div><dt>Scan coverage</dt><dd>{coverage}</dd></div>
              <div><dt>Task status</dt><dd>{taskStatus}</dd></div>
              <div><dt>Verification</dt><dd>{receipt}{verifiedAt ? ` ${verifiedAt}` : ""}</dd></div>
              <div><dt>Preview</dt><dd>{summary.sampleCount === null ? "Not reported" : <>{summary.sampleCount} {category.id === "amcache" ? (summary.sampleCount === 1 ? "sample" : "samples") : (summary.sampleCount === 1 ? "row" : "rows")}{summary.previewLimited ? `; limited${summary.previewLimit !== null ? ` to ${summary.previewLimit}` : ""}` : " reported"}</>}</dd></div>
            </dl></details>
            {summary.categories.length && ["found", "empty", "inconsistent"].includes(summary.state) ? <section aria-label="Record categories"><h3>Record categories</h3>
              <div className="cleanup-readable-detail__table"><table><thead><tr><th>Category</th><th>Records</th><th>Preview samples</th></tr></thead><tbody>
                {summary.categories.map((row, index) => <tr key={`${row.label}-${index}`}><td>{row.label}</td><td>{row.count < 0 ? "Unavailable" : row.count}</td><td>{row.samples ?? "Not reported"}</td></tr>)}
              </tbody></table></div><p>Preview samples are a selection and may be fewer than the full record count.</p></section> : null}
            {!summary.categories.length && summary.datasets.length ? <section aria-label="Reported datasets"><h3>Reported datasets</h3><ul>{summary.datasets.map(dataset => <li key={dataset.label}>{dataset.label}: {dataset.rows} preview {dataset.rows === 1 ? "row" : "rows"}</li>)}</ul><p>Preview rows can be fewer than the total found.</p></section> : null}
            <p className="cleanup-readable-detail__privacy">Record details are based on the data returned by this scan.</p>
          </> : null}
          {groupedItems?.length ? <section aria-label="Account breakdown"><h3>Account breakdown</h3><p>Only reported accounts are shown.</p>
            {groupedItems.map((group, index) => {
              const failed = group.count < 0 || summarizeCleanupResult({ id: category.id, count: group.count, items: group.items, raw: group.rawData }).state === "error";
              const facts = getCleanupScanFacts(group.rawData, failed);
              const truncated = !!group.rawData && typeof group.rawData === "object" && !Array.isArray(group.rawData) && (group.rawData as Record<string, unknown>).truncated === true;
              const hasRawPreview = getSafeCleanupPreview(category.id, group.rawData).length > 0 || truncated;
              return <div className="cleanup-readable-detail__account" key={`${group.title}-${index}`}><h4>{group.title}: {group.count >= 0 ? `${group.count} reported` : "Unavailable"}</h4>
              {facts.length ? <dl className="cleanup-readable-detail__facts">{facts.map((fact, factIndex) => <div key={`${fact.label}-${factIndex}`}><dt>{fact.label}</dt><dd>{fact.value}</dd></div>)}</dl> : null}
              {group.count < 0 ? <p>This account could not be checked completely.</p> : null}
              {hasRawPreview || group.items.length > 0 ? <CleanupRecordPreview categoryId={category.id} rawData={hasRawPreview ? group.rawData : { records: group.items }} mode={mode} accountLabel={group.title} accountName={group.username} /> : group.count > 0 ? <p>Preview details are unavailable for this account.</p> : null}
            </div>})}
          </section> : null}
        </div>
        <footer className="trace-dialog__footer"><Button text="Close" minimal onClick={onClose} />
          {onClear ? <Button text={clearLabel} intent="danger" loading={clearing} disabled={clearDisabled || clearing || summary.state !== "found"} onClick={onClear} /> : null}
        </footer>
      </div>
    </div>
  );
}
