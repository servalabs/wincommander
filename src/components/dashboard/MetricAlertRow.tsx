import { useEffect, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Bell, BellOff, Lock } from "lucide-react";
import { useMetricAlerts, type MetricKey } from "../../hooks/useMetricAlerts";
import { useAppState } from "../../context/AppContext";

/** Fleet settings dot-path per metric, matching the fleet-server contract
 *  ("upload"/"download" both collapse to the server's single networkUsage
 *  alert type, so they share one lock/report path). */
const FLEET_REPORT_PATH: Record<MetricKey, string> = {
  cpu: "notifications.cpuUsage.reportToFleet",
  ram: "notifications.ramUsage.reportToFleet",
  upload: "notifications.networkUsage.reportToFleet",
  download: "notifications.networkUsage.reportToFleet",
};

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
  /** Place the Fleet reporting switch at the end of the metric label row. */
  reportToFleetOnLabelRow?: boolean;
  /** Metric reports controlled by this switch; upload/download share one switch. */
  reportToFleetMetrics?: readonly MetricKey[];
  /** Omit the switch when another row owns the shared report setting. */
  showReportToFleet?: boolean;
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
  reportToFleetOnLabelRow = false,
  reportToFleetMetrics,
  showReportToFleet = true,
}: MetricAlertRowProps) {
  const { config, update, updateMetrics } = useMetricAlerts();
  const { appSettings } = useAppState();
  const m = config?.[metric];
  const [draft, setDraft] = useState<string>("");
  const [secDraft, setSecDraft] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [isShaking, setIsShaking] = useState(false);

  const fleetEnabled = appSettings?.app?.fleet?.enabled === true;
  const reportingMetrics = reportToFleetMetrics ?? [metric];
  const fleetPaths = reportingMetrics.map((reportingMetric) => FLEET_REPORT_PATH[reportingMetric]);
  const lockedPaths = appSettings?.policy?.lockedPaths ?? [];
  const allDeviceAlertsRequired = appSettings?.ideal?.security?.requireAllDeviceAlertsInFleet === true;
  const masterFleetAlertsLocked = lockedPaths.some(
    (p) => p.trim().length > 0 && (
      "security.requireAllDeviceAlertsInFleet".startsWith(p)
      || "ideal.security.requireAllDeviceAlertsInFleet".startsWith(p)
    ),
  );
  // Tolerate both the bare dot-path convention the fleet server contract uses
  // ("notifications.cpuUsage.reportToFleet") and this app's registry
  // convention of prefixing generic toggle paths with "ideal." — whichever
  // the connected server actually publishes, this still locks correctly.
  const reportToFleetLocked = (allDeviceAlertsRequired && masterFleetAlertsLocked) || lockedPaths.some(
    (p) => p.trim().length > 0 && fleetPaths.some(
      (fleetPath) => fleetPath.startsWith(p) || `ideal.${fleetPath}`.startsWith(p),
    )
  );

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

  const reportLabel = reportingMetrics.includes("upload") || reportingMetrics.includes("download")
    ? "network"
    : reportingMetrics.includes("cpu") && reportingMetrics.includes("ram")
      ? "system usage"
      : label;
  const reportToFleetEnabled = reportingMetrics.every(
    (reportingMetric) => config?.[reportingMetric]?.reportToFleet === true,
  );
  const fleetReportControl = fleetEnabled && showReportToFleet ? (
    <label
      className="metric-alert-report-fleet"
      title={reportToFleetLocked
        ? "Set by your Fleet administrator — this device must report alerts to Fleet."
        : "Forward this alert to your organization's Fleet console when it fires."}
    >
      <input
        type="checkbox"
        checked={reportToFleetEnabled}
        disabled={busy || reportToFleetLocked}
        onChange={(e) => {
          setBusy(true);
          void updateMetrics(reportingMetrics, { reportToFleet: e.target.checked })
            .finally(() => setBusy(false));
        }}
        aria-label={`Report ${reportLabel} alerts to Fleet`}
      />
      <span>Report to Fleet</span>
      {reportToFleetLocked && <Lock size={10} />}
    </label>
  ) : null;

  return (
    <div className={`metric-alert-row ${reportToFleetOnLabelRow && showReportToFleet ? "metric-alert-row--report-on-label" : ""}`}>
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
      {reportToFleetOnLabelRow && fleetReportControl}
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
      {!reportToFleetOnLabelRow && fleetReportControl}
      </div>
    </div>
  );
}
