import { useEffect, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Bell, BellOff } from "lucide-react";
import { useMetricAlerts, type MetricKey } from "../../hooks/useMetricAlerts";

interface MetricAlertRowProps {
  metric: MetricKey;
  label: string;
  unit: string;
  /**
   * When the alert is off, attempting to use its disabled controls gives a
   * short cue on the bell. The bell itself always remains a normal on/off
   * control.
   */
  buzzWhenInputDisabled?: boolean;
}

export function shouldBuzzMetricAlertInput(
  enabled: boolean,
  buzzWhenInputDisabled: boolean,
): boolean {
  return !enabled && buzzWhenInputDisabled;
}

/**
 * MetricAlertRow — reusable control for one metric alert (CPU / Upload /
 * Download). A bell toggles the alert on/off; the number field sets the
 * threshold in the metric's unit. Reads/writes the shared useMetricAlerts
 * store so every instance stays in sync and the backend is the single source.
 */
export default function MetricAlertRow({
  metric,
  label,
  unit,
  buzzWhenInputDisabled = false,
}: MetricAlertRowProps) {
  const { config, update } = useMetricAlerts();
  const m = config?.[metric];
  const [draft, setDraft] = useState<string>("");
  const [secDraft, setSecDraft] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [isShaking, setIsShaking] = useState(false);

  useEffect(() => {
    if (m) setDraft(String(m.threshold));
    // Only re-sync the draft when the persisted threshold changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [m?.threshold]);

  useEffect(() => {
    if (m) setSecDraft(String(m.sustainedSecs));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [m?.sustainedSecs]);

  if (!m) return null;

  const commit = async (patch: Parameters<typeof update>[1]) => {
    setBusy(true);
    try { await update(metric, patch); } finally { setBusy(false); }
  };

  const commitThreshold = () => {
    const n = parseFloat(draft);
    if (Number.isFinite(n) && n > 0 && n !== m.threshold) commit({ threshold: n });
    else setDraft(String(m.threshold));
  };

  const commitSeconds = () => {
    const n = parseInt(secDraft, 10);
    if (Number.isFinite(n) && n >= 1 && n !== m.sustainedSecs) commit({ sustainedSecs: Math.min(600, n) });
    else setSecDraft(String(m.sustainedSecs));
  };

  const buzzBell = () => {
    // Restart the short cue on consecutive attempts without changing settings.
    setIsShaking(false);
    requestAnimationFrame(() => setIsShaking(true));
  };

  const handleDisabledInputPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (busy || !shouldBuzzMetricAlertInput(m.enabled, buzzWhenInputDisabled)) return;
    // Disabled native inputs do not receive focus. Prevent the pointer action
    // at the wrapper so the off state remains unchanged while the bell cues
    // the operator about why the field cannot be edited yet.
    event.preventDefault();
    buzzBell();
  };

  return (
    <div className="metric-alert-row">
      <button
        type="button"
        className={`metric-alert-bell ${m.enabled ? "on" : "off"} ${isShaking ? "is-shaking" : ""}`}
        onAnimationEnd={() => setIsShaking(false)}
        onClick={() => void commit({ enabled: !m.enabled })}
        disabled={busy}
        title={m.enabled
          ? `${label} alert on — click to mute`
          : `${label} alert off — click to enable`}
        aria-label={m.enabled ? `Mute ${label} alert` : `Enable ${label} alert`}
      >
        {m.enabled ? <Bell size={12} /> : <BellOff size={12} />}
      </button>
      <span className="metric-alert-label">{label}</span>
      <div className="metric-alert-controls">
      <span className="metric-alert-over">over</span>
      <div className="metric-alert-input" onPointerDown={handleDisabledInputPointerDown}>
        <input
          type="number"
          min={metric === "cpu" ? 1 : 0.1}
          max={metric === "cpu" ? 100 : undefined}
          step={metric === "cpu" ? 1 : 0.5}
          value={draft}
          disabled={busy || !m.enabled}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitThreshold}
          onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }}
        />
        <span className="metric-alert-unit">{unit}</span>
      </div>
      {/* Hold time — only fire after the value stays over the limit this long. */}
      <span className="metric-alert-over">for</span>
      <div className="metric-alert-input" onPointerDown={handleDisabledInputPointerDown}>
        <input
          type="number"
          min={1}
          max={600}
          step={1}
          value={secDraft}
          disabled={busy || !m.enabled}
          onChange={(e) => setSecDraft(e.target.value)}
          onBlur={commitSeconds}
          onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }}
          title="Only alert after the value stays over the limit for this many seconds"
        />
        <span className="metric-alert-unit">s</span>
      </div>
      </div>
    </div>
  );
}
