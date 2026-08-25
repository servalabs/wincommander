// src/components/dashboard/NetworkTrafficCard.tsx
//
// Paid dashboard card: live network throughput (up/down) with a rolling
// sparkline and independent Upload / Download alert thresholds. The Rust
// sampler (net_traffic_alert.rs) emits `metrics://network` every second; the
// app-level useNetworkTrafficListener keeps that stream alive even when the
// Dashboard is not mounted. This card only renders the shared store and tunes
// the reusable per-metric alerts via useMetricAlerts.

import { useState, useCallback } from "react";
import { Bell, BellOff, ArrowUp, ArrowDown, ChevronUp, ChevronDown } from "lucide-react";
import { useMetricAlerts } from "../../hooks/useMetricAlerts";
import { useNetworkTraffic } from "../../hooks/useNetworkTraffic";
import MetricAlertRow from "./MetricAlertRow";

interface NetworkTrafficCardProps {
  /** Whether the card body (readout / sparkline / settings) is expanded.
   *  Part of the dashboard's at-most-two-open card group. Default true so the
   *  card still works if rendered standalone. */
  expanded?: boolean;
  onToggle?: () => void;
  /** Controlled by the dashboard so only one right-panel alert drawer is open. */
  drawerOpen?: boolean;
  onDrawerOpenChange?: (open: boolean) => void;
}

const BYTES_PER_MB = 1_000_000;

function fmtSpeed(bytesPerSec: number): { value: string; unit: string } {
  const mb = bytesPerSec / BYTES_PER_MB;
  if (mb >= 1) return { value: mb < 10 ? mb.toFixed(1) : Math.round(mb).toString(), unit: "MB/s" };
  const kb = bytesPerSec / 1000;
  return { value: kb < 10 ? kb.toFixed(1) : Math.round(kb).toString(), unit: "KB/s" };
}

export default function NetworkTrafficCard({
  expanded = true,
  onToggle,
  drawerOpen: controlledDrawerOpen,
  onDrawerOpenChange,
}: NetworkTrafficCardProps) {
  const [uncontrolledDrawerOpen, setUncontrolledDrawerOpen] = useState(false);
  const drawerOpen = controlledDrawerOpen ?? uncontrolledDrawerOpen;
  const setDrawerOpen = onDrawerOpenChange ?? setUncontrolledDrawerOpen;
  const { config: alerts } = useMetricAlerts();
  const { sample, upHistory, downHistory } = useNetworkTraffic();

  const up = fmtSpeed(sample.upBytesPerSec);
  const down = fmtSpeed(sample.downBytesPerSec);
  const upThresholdBytes = (alerts?.upload.threshold ?? Infinity) * BYTES_PER_MB;
  const downThresholdBytes = (alerts?.download.threshold ?? Infinity) * BYTES_PER_MB;
  const upOn = !!alerts?.upload.enabled;
  const downOn = !!alerts?.download.enabled;
  const alertsOn = upOn || downOn;

  const footText = useCallback(() => {
    if (!alerts) return "";
    const parts: string[] = [];
    if (upOn) parts.push(`↑ ${alerts.upload.threshold} MB/s`);
    if (downOn) parts.push(`↓ ${alerts.download.threshold} MB/s`);
    return parts.length ? `Alert at ${parts.join(" · ")}` : "Alerting off";
  }, [alerts, upOn, downOn]);

  return (
    <div className="nettraffic-card">
      <div className="nettraffic-header">
        <button
          type="button"
          className="nettraffic-card-toggle"
          onClick={onToggle}
          aria-expanded={expanded}
        >
          <span>NETWORK TRAFFIC</span>
          {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
        </button>
        <div className="nettraffic-header-actions">
          <button
            type="button"
            className={`nettraffic-cog ${alertsOn ? "on" : ""}`}
            onClick={() => setDrawerOpen(!drawerOpen)}
            title="Configure upload / download alerts"
            aria-label="Network traffic alert"
          >
            {alertsOn ? <Bell size={13} /> : <BellOff size={13} />}
          </button>
        </div>
      </div>

      {/* Speed readout is always visible; the graph + config only show when
          the card is expanded (owner request). */}
      <div className="nettraffic-stat-pair">
        <div className="nettraffic-stat nettraffic-stat--up">
          <div className="nettraffic-stat-label"><ArrowUp size={9} /> UPLOAD</div>
          <div className="nettraffic-stat-value">{up.value}<span className="nettraffic-unit"> {up.unit}</span></div>
        </div>
        <div className="nettraffic-stat nettraffic-stat--down">
          <div className="nettraffic-stat-label"><ArrowDown size={9} /> DOWNLOAD</div>
          <div className="nettraffic-stat-value">{down.value}<span className="nettraffic-unit"> {down.unit}</span></div>
        </div>
      </div>

      {/* Alert-config drawer is reachable regardless of the card's own
          expand/collapse state (mirrors System Info's alert drawer). */}
      {drawerOpen && (
        <div className="nettraffic-drawer">
          <div className="nettraffic-drawer-heading">
            <span>Traffic alerts</span>
            <span>{alertsOn ? "Monitoring sustained peaks" : "Both alerts are off"}</span>
          </div>
          <MetricAlertRow
            metric="upload"
            label="Upload"
            unit="MB/s"
            buzzWhenInputDisabled
            reportToFleetMetrics={["upload", "download"]}
            reportToFleetOnLabelRow
          />
          <MetricAlertRow metric="download" label="Download" unit="MB/s" buzzWhenInputDisabled showReportToFleet={false} />
        </div>
      )}

      {expanded && (<>
      <div className="nettraffic-chart-group">
        <div className="nettraffic-chart">
          <div className="nettraffic-chart-label">Upload</div>
          <div className="nettraffic-spark">
            {upHistory.map((v, i) => {
              const over = upOn && v > upThresholdBytes;
              return (
                <div
                  key={i}
                  className={`nettraffic-bar nettraffic-bar--up ${over ? "over" : ""}`}
                  style={{ height: `${Math.max(3, (v / Math.max(...upHistory, 1)) * 100)}%` }}
                />
              );
            })}
            {upHistory.length === 0 && <div className="nettraffic-spark-empty">sampling…</div>}
          </div>
        </div>

        <div className="nettraffic-chart">
          <div className="nettraffic-chart-label">Download</div>
          <div className="nettraffic-spark">
            {downHistory.map((v, i) => {
              const over = downOn && v > downThresholdBytes;
              return (
                <div
                  key={i}
                  className={`nettraffic-bar nettraffic-bar--down ${over ? "over" : ""}`}
                  style={{ height: `${Math.max(3, (v / Math.max(...downHistory, 1)) * 100)}%` }}
                />
              );
            })}
            {downHistory.length === 0 && <div className="nettraffic-spark-empty">sampling…</div>}
          </div>
        </div>
      </div>

      <div className="nettraffic-foot">{footText()}</div>
      </>)}
    </div>
  );
}
