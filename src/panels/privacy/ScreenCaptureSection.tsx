// src/panels/privacy/ScreenCaptureSection.tsx
//
// #5 — Screen-capture detection & own-window protection. Privacy-panel
// section card. Two Paid toggles + a recent-detections list:
//
//   • "Detect screen-capture tools" — arms the Pro sidecar poller that
//     watches for known recorders (OBS, ShareX, Bandicam, ScreenToGif,
//     Snagit, Snipping Tool, …). BEST-EFFORT process-presence heuristic;
//     the copy says a tool is *running*, never that you were recorded.
//   • "Protect this window from capture" — applies
//     SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE) to WinCommander's
//     own window so it renders BLACK in screenshots / recordings /
//     screen-share, while staying visible on-screen. Other apps are
//     unaffected (we can only set affinity on HWNDs we own). Win10 2004+.
//
// All runtime logic lives in useScreenCapture; this file is presentation.
// Design system: .bp6-*, border-radius:0, font-mono for tool names /
// timestamps / confidence badges, design tokens (no hardcoded hex).

import { Switch, Icon, Button, Spinner, Tag } from "@/components/ui/bp";
import { useState } from "react";
import useScreenCapture from "./useScreenCapture";
import SectionCard from "../../components/shared/SectionCard";
import PrivacyEventTable from "./PrivacyEventTable";
import "./ScreenCaptureSection.css";

interface Props {
  detectionEnabled: boolean;
  protectWindow: boolean;
  /** Forward each screen-capture-tool detection to the Fleet console.
   *  Settings path `notifications.screenCapture.reportToFleet`. */
  reportToFleet?: boolean;
  /** True when the org's fleet policy locks this path — render
   *  visible-but-disabled rather than hiding it. */
  reportToFleetLocked?: boolean;
  /** Only show the Fleet-reporting row when this device is fleet-enrolled. */
  fleetEnabled?: boolean;
  onPatch: (patch: { detectionEnabled?: boolean; protectWindow?: boolean; reportToFleet?: boolean }) => void;
  /** Controlled expand (accordion). Falls back to internal state if omitted. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
}

function formatTime(iso: string): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

export default function ScreenCaptureSection({
  detectionEnabled,
  protectWindow,
  reportToFleet = false,
  reportToFleetLocked = false,
  fleetEnabled = false,
  onPatch,
  expanded: expandedProp,
  onExpandedChange,
}: Props) {
  const [internalExpanded, setInternalExpanded] = useState(false);
  const expanded = expandedProp ?? internalExpanded;
  const setExpanded = (next: boolean) => {
    if (onExpandedChange) onExpandedChange(next);
    else setInternalExpanded(next);
  };
  const { recent, status, busy, error, toggleDetection, toggleProtection, clearRecent } =
    useScreenCapture(detectionEnabled, onPatch);

  const running = !!status?.running;
  const active = detectionEnabled || protectWindow;

  const headerRight = (
    <div className="flex items-center gap-1.5">
      {protectWindow && (
        <Tag minimal intent="success" className="screencap-tag">
          WINDOW SHIELDED
        </Tag>
      )}
      {detectionEnabled && (
        <Tag minimal intent={running ? "primary" : "none"} className="screencap-tag">
          {running ? "DETECTING" : "ARMING…"}
        </Tag>
      )}
      {busy && <Spinner size={12} />}
    </div>
  );

  return (
    <SectionCard title="Screen capture" icon="camera" headerRight={headerRight} armed={active} className="screencap-card">
      {/* ── Header ─────────────────────────────────────────────── */}
      <div className="screencap-head">
        <p className="screencap-sub">
          Best-effort detection of running screen-recording tools, plus a strong
          guarantee that <em>this</em> window renders black in screenshots and
          screen-share.
        </p>
      </div>

      {/* ── Toggle: detection ──────────────────────────────────── */}
      <div className="screencap-row">
        <Switch
          checked={detectionEnabled}
          disabled={busy}
          onChange={(e) => void toggleDetection(e.currentTarget.checked)}
          aria-label="Detect screen-capture tools"
        />
        <div className="screencap-row-text">
          <span className="screencap-row-label">Detect screen-capture tools</span>
          <span className="screencap-row-help">
            Watches for known recorders (OBS, ShareX, Bandicam, ScreenToGif,
            Snagit, the Snipping Tool). Best-effort: a tool being open means it
            <em> could</em> capture your screen — it does not mean a screenshot
            was taken. Renamed tools are not detected.
          </span>
        </div>
      </div>

      {/* ── Toggle: protection ─────────────────────────────────── */}
      <div className="screencap-row">
        <Switch
          checked={protectWindow}
          disabled={busy}
          onChange={(e) => void toggleProtection(e.currentTarget.checked)}
          aria-label="Protect this window from capture"
        />
        <div className="screencap-row-text">
          <span className="screencap-row-label">Protect this window from capture</span>
          <span className="screencap-row-help">
            WinCommander's window appears black in screenshots, recordings, and
            screen-share. Other apps are unaffected. Requires Windows 10 2004+.
          </span>
        </div>
      </div>

      {fleetEnabled && (
        <div className="screencap-row">
          <Switch
            checked={reportToFleet}
            disabled={busy || reportToFleetLocked}
            onChange={(e) => onPatch({ reportToFleet: e.currentTarget.checked })}
            aria-label="Report screen-capture detections to Fleet"
          />
          <div className="screencap-row-text">
            <span className="screencap-row-label">
              Report to Fleet
              {reportToFleetLocked && <Icon icon="lock" size={10} style={{ marginLeft: 6, verticalAlign: 'middle' }} />}
            </span>
            <span className="screencap-row-help">
              {reportToFleetLocked
                ? "Set by your Fleet administrator — cannot be changed on this device."
                : "Forward each screen-capture-tool detection to your organization's Fleet console."}
            </span>
          </div>
        </div>
      )}

      {error && (
        <div role="alert" className="screencap-error">
          <Icon icon="warning-sign" size={12} />
          <span>{error}</span>
        </div>
      )}

      {/* ── Recent detections (collapsible) ────────────────────── */}
      {detectionEnabled && (
        <div className="screencap-recent-wrap">
          <button
            type="button"
            className="screencap-disclosure"
            onClick={() => setExpanded(!expanded)}
            aria-expanded={expanded}
          >
            <span className="screencap-disclosure-label">
              Recent detections ({recent.length})
            </span>
            <Icon
              icon={expanded ? "chevron-up" : "chevron-down"}
              size={12}
              color="var(--color-text-muted)"
            />
          </button>

          {expanded && (
            <>
              {recent.length > 0 && (
                <div className="screencap-recent-actions">
                  <Button minimal small icon="trash" onClick={() => void clearRecent()} aria-label="Clear screen-capture detections">
                    Clear
                  </Button>
                </div>
              )}
              {recent.length === 0 ? (
                <p className="screencap-empty">
                  {running
                    ? "No capture tools seen yet — the detector is silent until one appears."
                    : "Arm detection to start watching."}
                </p>
              ) : (
                <PrivacyEventTable title="Screen-capture detections" columns={["Time", "Confidence", "Tool", "Process"]} rows={recent.map((h, i) => ({ id: `${h.detectedAt}-${i}`, search: `${h.confidence} ${h.tool} ${h.processName}`, sort: [h.detectedAt, h.confidence, h.tool, h.processName], cells: [formatTime(h.detectedAt), <Tag minimal intent={h.confidence === "high" ? "warning" : "none"}>{h.confidence.toUpperCase()}</Tag>, h.tool, h.processName] }))} />
              )}
            </>
          )}
        </div>
      )}
    </SectionCard>
  );
}
