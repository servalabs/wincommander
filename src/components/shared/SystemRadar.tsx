import type { CSSProperties } from "react";
import StatusPill, { type StatusPillTone } from "./StatusPill";
import "./SystemRadar.css";

export interface SystemRadarFinding {
  id: string;
  label: string;
  status: "ready" | "attention" | "blocked" | "muted";
  detail?: string;
}

interface SystemRadarProps {
  score: number;
  findings: SystemRadarFinding[];
  title?: string;
  className?: string;
}

const STATUS_TONE: Record<SystemRadarFinding["status"], StatusPillTone> = {
  ready: "success",
  attention: "warning",
  blocked: "danger",
  muted: "neutral",
};

function clampScore(score: number): number {
  return Math.max(0, Math.min(100, Math.round(score)));
}

export default function SystemRadar({
  score,
  findings,
  title = "Health",
  className = "",
}: SystemRadarProps) {
  const safeScore = clampScore(score);

  return (
    <section className={`system-radar ${className}`.trim()} aria-label={title}>
      <div
        className="system-radar__ring"
        style={{ "--system-radar-score": `${safeScore}%` } as CSSProperties}
      >
        <strong>{safeScore}</strong>
        <span>{title}</span>
      </div>
      <div className="system-radar__findings">
        {findings.map((finding) => (
          <div className="system-radar__finding" key={finding.id}>
            <div>
              <strong>{finding.label}</strong>
              {finding.detail ? <span>{finding.detail}</span> : null}
            </div>
            <StatusPill tone={STATUS_TONE[finding.status]}>{finding.status}</StatusPill>
          </div>
        ))}
      </div>
    </section>
  );
}
