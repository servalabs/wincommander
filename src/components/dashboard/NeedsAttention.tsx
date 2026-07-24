import { useState, type ReactNode } from "react";
import { ChevronDown, Zap, X, Loader2 } from "lucide-react";
import type { ScanFinding } from "../startup/WizardAnimations";
import "./NeedsAttention.css";

// "Needs Attention" — the per-finding detail list the dashboard shows beneath
// the radar. The radar gives the at-a-glance posture; this lets the user read
// exactly WHAT each issue is, fix them individually, fix everything at once, or
// ignore the ones they don't care about (persisted so they stay dismissed).

const SEVERITY_CLASS: Record<ScanFinding["severity"], string> = {
  critical: "danger",
  warning: "warn",
  info: "info",
};

const CATEGORY_LABEL: Record<ScanFinding["category"], string> = {
  privacy: "Privacy",
  performance: "Performance",
  annoyance: "Annoyance",
  engines: "Engines",
  updates: "Updates",
};

export interface NeedsAttentionProps {
  findings: ScanFinding[];
  /** IDs currently being fixed (spinner + disabled). */
  busyIds: Set<string>;
  onFixOne: (f: ScanFinding) => void;
  onFixAll: () => void;
  onIgnore: (f: ScanFinding) => void;
  ignoredCount: number;
  onResetIgnored: () => void;
  /**
   * When set, only show findings from this category (set by clicking a radar
   * node). The header shows the active filter and a clear affordance.
   * KT: Null/undefined means show all — this is always the default state.
   */
  categoryFilter?: string | null;
  onClearFilter?: () => void;
  /** Controlled expand state (optional). Lets the dashboard hide the tagline +
   *  view toggle ONLY while this list is expanded. Falls back to internal state. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
  /** Rendered at the bottom of the expanded list — the dashboard drops the
   *  "Update All Apps" button + the disk-cleanup chip in here so they live
   *  INSIDE the card while it's open (owner request). */
  expandedFooter?: ReactNode;
}

export default function NeedsAttention({
  findings,
  busyIds,
  onFixOne,
  onFixAll,
  onIgnore,
  ignoredCount,
  onResetIgnored,
  categoryFilter,
  onClearFilter,
  expanded: expandedProp,
  onExpandedChange,
  expandedFooter,
}: NeedsAttentionProps) {
  // KT: Expanded by default so the user immediately sees what needs fixing
  // rather than having to click the header to discover pending issues.
  // Controlled when `expanded`/`onExpandedChange` are passed (dashboard lifts
  // this to drive its tagline/view-toggle hide behavior).
  const [internalExpanded, setInternalExpanded] = useState(true);
  const expanded = expandedProp ?? internalExpanded;
  const setExpanded = (next: boolean) => {
    if (onExpandedChange) onExpandedChange(next);
    else setInternalExpanded(next);
  };
  const handleToggle = () => {
    if (categoryFilter && onClearFilter) onClearFilter();
    setExpanded(!expanded);
  };

  if (findings.length === 0) {
    // Nothing active — only surface the "restore ignored" affordance.
    if (ignoredCount === 0) return null;
    return (
      <div className="needs-attention is-clear" role="status" aria-live="polite">
        <span className="na-clear-msg"><span aria-hidden="true">✓ </span>All clear</span>
        <button className="na-reset" onClick={onResetIgnored}>
          Restore {ignoredCount} ignored
        </button>
      </div>
    );
  }

  // When a radar node is clicked, show only its category.
  const visibleFindings = categoryFilter
    ? findings.filter((f) => f.category === categoryFilter)
    : findings;

  const anyBusy = busyIds.size > 0;
  const hasVisibleFindings = visibleFindings.length > 0;

    // data-tour: primary spotlight cutout for the Fix All tour step. This is
    // the real ~560px centred card (list + Fix All button), NOT the
    // full-width .dashboard-fix-actions wrapper — so the tour callout places
    // cleanly to its right instead of being forced onto the highlighted
    // region (2026-07-20).
  return (
    <div className={`needs-attention ${expanded ? "is-expanded" : ""}`} data-tour="dashboard-fix-region">
      <div className="na-head">
        <button
          className="na-toggle"
          onClick={handleToggle}
          aria-expanded={expanded}
        >
          <span className="na-count">{visibleFindings.length}</span>
          <span className="na-title">
            {categoryFilter
              ? CATEGORY_LABEL[categoryFilter as ScanFinding["category"]] ?? categoryFilter
              : `need${findings.length === 1 ? "s" : ""} attention`}
          </span>
          <ChevronDown size={15} className="na-chevron" />
        </button>
        {categoryFilter && onClearFilter && (
          <button className="na-btn na-btn--clear" onClick={onClearFilter} title="Show all findings">
            <X size={12} />
          </button>
        )}
        <button className="na-fix-all" data-tour="dashboard-fix-all" onClick={onFixAll} disabled={anyBusy || !hasVisibleFindings}>
          <Zap size={14} />
          Fix all
        </button>
      </div>

      {expanded && (
        <>
        <ul className="na-list">
          {!hasVisibleFindings && (
            <li className="na-foot">No items in this category.</li>
          )}
          {visibleFindings.map((f) => {
            const busy = busyIds.has(f.id);
            return (
              <li className={`na-item sev-${SEVERITY_CLASS[f.severity]}`} key={f.id}>
                <span className="na-dot" />
                <div className="na-body">
                  <div className="na-label">
                    {f.label}
                    {f.drift ? <span className="na-drift">DRIFT</span> : null}
                    <span className="na-cat">{CATEGORY_LABEL[f.category]}</span>
                  </div>
                  {f.impact ? <div className="na-impact">{f.impact}</div> : null}
                </div>
                <div className="na-actions">
                  <button
                    className="na-btn na-btn--fix"
                    onClick={() => onFixOne(f)}
                    disabled={anyBusy}
                    title="Apply this fix"
                  >
                    {busy ? <Loader2 size={13} className="na-spin" /> : "Fix"}
                  </button>
                  <button
                    className="na-btn na-btn--ignore"
                    onClick={() => onIgnore(f)}
                    disabled={busy}
                    title="Ignore — hide this from the list"
                  >
                    <X size={11} />
                    Ignore
                  </button>
                </div>
              </li>
            );
          })}
          {ignoredCount > 0 && (
            <li className="na-foot">
              <button className="na-reset" onClick={onResetIgnored}>
                Restore {ignoredCount} ignored
              </button>
            </li>
          )}
        </ul>
        {/* Footer (Update All Apps + Clean chip) sits OUTSIDE the scrollable
            list so it's never clipped at the bottom of the 240px scroll area. */}
        {expandedFooter && (
          <div className="na-expanded-footer">{expandedFooter}</div>
        )}
        </>
      )}
    </div>
  );
}
