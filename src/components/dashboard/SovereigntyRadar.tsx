import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { ScanReport, ScanFinding } from "../startup/WizardAnimations";
import "./SovereigntyRadar.css";
import AnimatedNumber from "../shared/AnimatedNumber";

// ── Reference-style SVG sonar ───────────────────────────────────────────────
// Replaces the canvas threat-radar on the dashboard with the V2 reference's
// clean sonar: concentric rings + cross/ticks, a subtle conic sweep, a centered
// score core, and domain nodes (Privacy / Performance / Annoyances / Updates)
// placed around the rings with ✓ / ! labels + a legend beneath.
//
// Findings are aggregated by category into a fixed set of domains so the radar
// stays uncluttered no matter how many individual fixes the scan surfaces.

type DomainStatus = "ok" | "warn" | "danger";

interface DomainNode {
  key: string;
  label: string;
  status: DomainStatus;
  count: number;
  /** Actual findings aggregated into this domain — used for tooltip + click drill-down. */
  findings: ScanFinding[];
  /** Angle in degrees, 0 = +x axis, clockwise. */
  angle: number;
}

export interface SovereigntyRadarProps {
  phase: "idle" | "scanning" | "complete";
  report: ScanReport | null;
  /** 0–100 health score shown in the core. */
  score: number;
  /** Band colour for the core (from useSovereigntyScore). */
  scoreColor: string;
  pendingAppUpdates?: number;
  /** Number of required panel engines/dependencies that aren't installed. */
  missingEngineCount?: number;
  /** When true, force the all-clear "OPTIMAL" core label. */
  optimal?: boolean;
  /**
   * Called when the user clicks a domain node. The argument is the node key
   * (category string: "privacy" | "performance" | "annoyance" | "updates" | "engines").
   * Pass null/undefined to omit — nodes render as non-interactive divs.
   *
   * KT: This is how the dashboard wires radar → NeedsAttention drill-down:
   * parent holds `categoryFilter` state; click sets it; NeedsAttention
   * receives it and shows only matching findings.
   */
  onNodeClick?: (key: string) => void;
  /** Exposes completion to the Fix All tour when this is its fallback anchor. */
  tourState?: "done";
}

// Domains sit on the diagonals so they never overlap the cross lines.
const DOMAIN_DEFS: { key: string; label: string; match: (f: ScanFinding) => boolean; angle: number }[] = [
  { key: "privacy", label: "Privacy", match: (f) => f.category === "privacy", angle: -45 },
  { key: "performance", label: "Performance", match: (f) => f.category === "performance", angle: 45 },
  { key: "annoyance", label: "Annoyances", match: (f) => f.category === "annoyance", angle: 135 },
];

function bandLabel(score: number): string {
  if (score >= 80) return "PROTECTED";
  if (score >= 60) return "GUARDED";
  if (score >= 40) return "EXPOSED";
  return "AT RISK";
}

function statusFor(findings: ScanFinding[]): DomainStatus {
  if (findings.length === 0) return "ok";
  return findings.some((f) => f.severity === "critical") ? "danger" : "warn";
}

export default function SovereigntyRadar({
  phase,
  report,
  score,
  scoreColor,
  pendingAppUpdates = 0,
  missingEngineCount = 0,
  optimal,
  onNodeClick,
  tourState,
}: SovereigntyRadarProps) {
  const scanning = phase !== "complete" || report === null;

  const nodes = useMemo<DomainNode[]>(() => {
    const findings = report?.findings ?? [];
    const domainNodes: DomainNode[] = DOMAIN_DEFS.map((d) => {
      const hits = findings.filter(d.match);
      return { key: d.key, label: d.label, count: hits.length, findings: hits, status: statusFor(hits), angle: d.angle };
    });
    // Updates node includes app updates plus Pro sidecar repair/update findings.
    const updateFindings = findings.filter((f) => f.category === "updates");
    const updateCount = Math.max(pendingAppUpdates, updateFindings.length);
    domainNodes.push({
      key: "updates",
      label: "Updates",
      count: updateCount,
      findings: updateFindings,
      status: updateCount > 0 ? "warn" : "ok",
      angle: 225,
    });
    // Engines node — shown when required panel dependencies are not installed.
    // Positioned at 270° (straight up) to sit on the top tick mark.
    if (missingEngineCount > 0) {
      const engineFindings = findings.filter((f) => f.category === "engines");
      domainNodes.push({
        key: "engines",
        label: "Engines",
        count: missingEngineCount,
        findings: engineFindings,
        status: "danger",
        angle: 270,
      });
    }
    return domainNodes;
  }, [report, pendingAppUpdates, missingEngineCount]);

  // Track prior status per node so we can "pop" a node once it gets fixed
  // (warn/danger → ok). The set holds node keys currently mid fixed-pop; each
  // is cleared after one animation cycle so the class is re-addable later.
  const prevStatusRef = useRef<Record<string, DomainStatus>>({});
  const [fixedKeys, setFixedKeys] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    if (scanning) {
      // While scanning the nodes are unmounted; reset tracking so the first
      // post-scan render establishes a fresh baseline (no spurious pops).
      prevStatusRef.current = {};
      return;
    }
    const prev = prevStatusRef.current;
    const justFixed: string[] = [];
    for (const n of nodes) {
      const before = prev[n.key];
      if (before && before !== "ok" && n.status === "ok") justFixed.push(n.key);
      prev[n.key] = n.status;
    }
    if (justFixed.length === 0) return;

    setFixedKeys((cur) => {
      const next = new Set(cur);
      for (const k of justFixed) next.add(k);
      return next;
    });
    const t = window.setTimeout(() => {
      setFixedKeys((cur) => {
        const next = new Set(cur);
        for (const k of justFixed) next.delete(k);
        return next;
      });
    }, 700);
    return () => window.clearTimeout(t);
  }, [nodes, scanning]);

  const allClear = optimal ?? (!scanning && nodes.every((n) => n.status === "ok"));
  const coreLabel = scanning ? "SCANNING" : allClear ? "OPTIMAL" : bandLabel(score);

  // Node positions: polar → percentage offsets from centre. 0.66 keeps the
  // dots on a consistent inner-of-outer ring; 48% is the usable half-extent.
  const RING_FRAC = 0.66;

  // When everything is clear (after Fix All, or a clean scan) the radar leans
  // green to signal "secure" — the sweep, core, and a soft halo all shift to the
  // success colour instead of the score-band accent. Red/amber still read as
  // "things can be better" while findings remain.
  const radarAccent = allClear ? "var(--color-success)" : scoreColor;

  return (
    <div className={`sov-radar ${scanning ? "is-scanning" : ""}${allClear ? " is-optimal" : ""}`} aria-label="System health radar">
      <div
        className="sov-radar__scope"
        style={{ "--sov-accent": radarAccent } as CSSProperties}
        // Secondary spotlight cutout for the Fix All tour step — the disc
        // itself (max-width 540px, centred), not the full-width .sov-radar
        // column, so the tour ring/hole hugs the radar (see
        // dashboard-tour-fix-all).
        data-tour="dashboard-radar"
        data-tour-state={tourState}
      >
        <svg viewBox="0 0 200 200" className="sov-radar__svg" aria-hidden="true">
          <circle className="ring faint" cx="100" cy="100" r="92" />
          <circle className="ring" cx="100" cy="100" r="66" />
          <circle className="ring spin" cx="100" cy="100" r="53" />
          <circle className="ring" cx="100" cy="100" r="40" />
          <line className="cross" x1="100" y1="8" x2="100" y2="192" />
          <line className="cross" x1="8" y1="100" x2="192" y2="100" />
          {[0, 90, 180, 270].map((deg) => {
            const r = (deg * Math.PI) / 180;
            const x1 = 100 + Math.cos(r) * 88;
            const y1 = 100 + Math.sin(r) * 88;
            const x2 = 100 + Math.cos(r) * 92;
            const y2 = 100 + Math.sin(r) * 92;
            return <line key={deg} className="tick" x1={x1} y1={y1} x2={x2} y2={y2} />;
          })}
        </svg>

        <div className="sov-radar__sweep-wrap">
          <div className="sov-radar__sweep" />
        </div>

        {/* Centre read-out */}
        <div className="sov-radar__core">
          <div className="score" style={{ color: scanning ? "var(--text-mute)" : radarAccent }}>
            <strong><AnimatedNumber value={Math.max(0, Math.min(100, score))} /></strong>
            <sup>%</sup>
          </div>
          <div className="core-lbl">{coreLabel}</div>
        </div>

        {/* Domain nodes — clickable when onNodeClick is wired and the node
            has pending issues. Hovering shows a tooltip listing the specific
            finding labels so users know exactly what's flagged per domain. */}
        {!scanning &&
          nodes.map((n, i) => {
            const r = (n.angle * Math.PI) / 180;
            const left = 50 + Math.cos(r) * RING_FRAC * 48;
            const top = 50 + Math.sin(r) * RING_FRAC * 48;
            // Staggered entrance + ping cadence offset per node index.
            const delay = `${(i * 90) / 1000}s`;
            const fixed = fixedKeys.has(n.key);
            const isClickable = onNodeClick != null && n.status !== "ok";
            // Build tooltip: list finding labels (max 5) so user knows what's flagged.
            const tipLines = n.findings.slice(0, 5).map((f) => f.label);
            if (n.findings.length > 5) tipLines.push(`+${n.findings.length - 5} more`);
            const title = tipLines.length > 0 ? tipLines.join("\n") : undefined;
            return (
              <div
                key={n.key}
                role={isClickable ? "button" : undefined}
                tabIndex={isClickable ? 0 : undefined}
                className={`sov-radar__node ${n.status}${fixed ? " fixed-pulse" : ""}${isClickable ? " is-clickable" : ""}`}
                style={{ left: `${left}%`, top: `${top}%`, "--d": delay } as CSSProperties}
                title={title}
                onClick={isClickable ? () => onNodeClick!(n.key) : undefined}
                onKeyDown={isClickable ? (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onNodeClick!(n.key); } } : undefined}
              >
                <span className="ping" aria-hidden="true" />
                <span className="nlbl">
                  {n.label}
                  {n.count > 0 ? ` ${n.count}` : ""}
                </span>
              </div>
            );
          })}
      </div>

      {/* Legend removed (owner 2026-06-11): the same per-domain info is
          already rendered as node labels around the radar ring, so the
          duplicated strip below the disc was just stealing vertical space.
          That space now goes to the disc itself (see .sov-radar__scope
          max-width bump in the css). */}
    </div>
  );
}
