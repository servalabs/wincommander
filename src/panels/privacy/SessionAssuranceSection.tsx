// src/panels/privacy/SessionAssuranceSection.tsx
//
// Session Assurance — attention / insider-risk monitoring (Pro).
//
// Backend: commander-pro/src/attention_collector.rs + handlers.rs (M1–M5).
// TODO(fleet): orgId + subjectId are "self" defaults for personal/dev use.
// In fleet-managed mode the fleet server supplies these via managed config.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Icon, Spinner, Switch, Tag } from '@/components/ui/bp';
import useEntitlements from '@/hooks/useEntitlements';
import SectionCard from '../../components/shared/SectionCard';
import { showError } from '../../utils/toast';

// ── Types ──────────────────────────────────────────────────────────────────

interface SessionScore {
  score: number; // 0.0–1.0
  label: string;
  sampledAt: string;
}

interface ActiveAlert {
  alertId: string;
  kind: string;
  severity: 'info' | 'warn' | 'high';
  summary: string;
  firedAt: string;
}

interface MonitorStatus {
  running: boolean;
  sessionId: string | null;
}

interface DetectorConfig {
  checkGaze: boolean;
  checkFaces: boolean;
  checkSecondaryDevice: boolean;
  modelLevel: 'fast' | 'balanced' | 'accurate';
  /** When true, alerts are persisted but on-screen notifications are suppressed. */
  silentMode: boolean;
}

// ── Constants ──────────────────────────────────────────────────────────────

const DEFAULT_CONFIG: DetectorConfig = {
  checkGaze: true,
  checkFaces: true,
  checkSecondaryDevice: false,
  modelLevel: 'balanced',
  silentMode: false,
};

// Personal/dev-mode identity defaults (TODO(fleet): fleet-managed deployments
// override these at runtime via managed config supplied by the fleet server).
const SELF_ORG_ID = 'self';
const SELF_SUBJECT_ID = 'self';
// device_id: use the machine hostname as a stable personal-mode device token.
const SELF_DEVICE_ID =
  (typeof navigator !== 'undefined' && navigator.userAgent) || 'self-device';

// ── Helpers ────────────────────────────────────────────────────────────────

// Backend can stall (e.g. camera acquisition hangs) — race the start call
// against a timeout so the toggle reliably resolves to an error toast
// instead of leaving the switch spinning forever.
const START_MONITOR_TIMEOUT_MS = 10_000;

function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => {
      setTimeout(() => reject(new Error(message)), ms);
    }),
  ]);
}

function formatRelative(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '';
  const diffSec = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const m = Math.floor(diffSec / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

function severityColor(sev: string): string {
  if (sev === 'high') return 'var(--color-danger, #f87171)';
  if (sev === 'warn') return 'var(--color-warning, #fbbf24)';
  return 'var(--color-text-muted)';
}

function scoreColor(score: number): string {
  if (score >= 0.75) return 'var(--color-success)';
  if (score >= 0.45) return 'var(--color-warning, #fbbf24)';
  return 'var(--color-danger, #f87171)';
}

// ── Component ──────────────────────────────────────────────────────────────

export default function SessionAssuranceSection() {
  const { canUse } = useEntitlements();
  // Monitor
  const [monitorStatus, setMonitorStatus] = useState<MonitorStatus | null>(null);
  const [score, setScore] = useState<SessionScore | null>(null);
  const [alerts, setAlerts] = useState<ActiveAlert[]>([]);
  const [monitorBusy, setMonitorBusy] = useState(false);
  const [monitorError, setMonitorError] = useState<string | null>(null);

  // Detector config
  const [config, setConfig] = useState<DetectorConfig>(DEFAULT_CONFIG);
  const [configExpanded, setConfigExpanded] = useState(false);

  const isRunning = monitorStatus?.running === true;

  // ── Data refresh ───────────────────────────────────────────────────────

  const refreshMonitor = useCallback(async () => {
    try {
      const [status, sc, al] = await Promise.all([
        invoke<MonitorStatus>('session_monitor_status', {
          subjectId: SELF_SUBJECT_ID,
          orgId: SELF_ORG_ID,
        }),
        invoke<SessionScore>('get_session_score', {
          orgId: SELF_ORG_ID,
          subjectId: SELF_SUBJECT_ID,
        }).catch(() => null),
        invoke<ActiveAlert[]>('get_active_alerts', {
          orgId: SELF_ORG_ID,
        }).catch(() => [] as ActiveAlert[]),
      ]);
      setMonitorStatus(status);
      setScore(sc);
      setAlerts(Array.isArray(al) ? al : []);
    } catch (e) {
      setMonitorError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshMonitor();
    const id = setInterval(() => void refreshMonitor(), isRunning ? 8_000 : 30_000);
    return () => clearInterval(id);
  }, [isRunning, refreshMonitor]);

  // ── Monitor actions ────────────────────────────────────────────────────

  const toggleMonitor = useCallback(async (on: boolean) => {
    setMonitorBusy(true);
    setMonitorError(null);
    try {
      if (on) {
        await withTimeout(
          invoke('start_session_monitor', {
            subjectId: SELF_SUBJECT_ID,
            orgId: SELF_ORG_ID,
            deviceId: SELF_DEVICE_ID,
            modelLevel: config.modelLevel,
            checkGaze: config.checkGaze,
            checkFaces: config.checkFaces,
            checkSecondaryDevice: config.checkSecondaryDevice,
            silentMode: config.silentMode,
          }),
          START_MONITOR_TIMEOUT_MS,
          'Session monitor start timed out — the backend did not respond in time.',
        );
      } else {
        await invoke('stop_session_monitor', {
          subjectId: SELF_SUBJECT_ID,
          orgId: SELF_ORG_ID,
        });
      }
      await refreshMonitor();
    } catch (e) {
      const message = String(e instanceof Error ? e.message : e);
      setMonitorError(message);
      showError(message);
    } finally {
      setMonitorBusy(false);
    }
  }, [refreshMonitor, config]);

  // ── Status pill ────────────────────────────────────────────────────────

  const highAlertCount = alerts.filter((a) => a.severity === 'high').length;
  let statusPill: React.ReactNode = null;
  if (highAlertCount > 0) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-danger,#f87171)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/40 flex-shrink-0 font-mono">
        {highAlertCount} ALERT{highAlertCount !== 1 ? 'S' : ''}
      </span>
    );
  } else if (isRunning) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        MONITORING
      </span>
    );
  } else {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-text-muted)]/10 text-[var(--color-text-muted)] border border-[var(--color-border)] flex-shrink-0 font-mono">
        OFF
      </span>
    );
  }

  // Session Assurance (webcam attention / insider-risk monitoring) is Pro.
  // Gate the whole section behind the paid entitlement — matching the sibling
  // Argus sections. Placed after all hooks so hook order stays stable.
  if (!canUse('paid')) {
    return (
      <SectionCard title="Session Assurance" icon="eye-open" headerRight={<Tag minimal intent="none" className="font-mono text-[10px] flex-shrink-0">PRO</Tag>}>
        <p className="text-xs text-[var(--shield-text-subtle)] opacity-50">
          Attention / insider-risk monitoring (gaze, face, and
          secondary-device checks). Requires Pro.
        </p>
      </SectionCard>
    );
  }

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <SectionCard title="Session Assurance" icon="eye-open" headerRight={statusPill} armed={isRunning || highAlertCount > 0}>
      {/* ── Monitor controls ── */}
      {(
        <div className="flex flex-col gap-3">
          <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[360px]">
            Attention and insider-risk monitoring via webcam. Checks gaze direction, secondary faces,
            and secondary devices during a session. Biometric samples are processed on-device and
            never stored raw.
          </p>
          {/* Start / Stop row */}
          <div className="flex items-center gap-3 flex-wrap">
            <Switch
              checked={isRunning}
              disabled={monitorBusy}
              onChange={(e) => void toggleMonitor((e.target as HTMLInputElement).checked)}
              label="Session monitoring active"
            />
            <Button
              icon="refresh"
              minimal
              small
              disabled={monitorBusy}
              onClick={() => void refreshMonitor()}
            >
              Refresh
            </Button>
            {monitorBusy && <Spinner size={14} />}
          </div>

          {monitorError && (
            <div className="font-mono text-xs text-[var(--color-danger)]">{monitorError}</div>
          )}

          {/* Score */}
          {score && (
            <div className="flex items-center gap-2">
              <span className="text-xs text-[var(--shield-text-subtle)]">Session score</span>
              <span
                className="font-mono text-sm font-semibold"
                style={{ color: scoreColor(score.score) }}
              >
                {Math.round(score.score * 100)}%
              </span>
              {score.label && (
                <span className="text-xs opacity-60">{score.label}</span>
              )}
              <span className="text-[10px] font-mono opacity-40 ml-auto">
                {formatRelative(score.sampledAt)}
              </span>
            </div>
          )}

          {/* Active alerts */}
          {alerts.length > 0 && (
            <div className="flex flex-col gap-1">
              <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                Active alerts ({alerts.length})
              </span>
              <div className="flex flex-col gap-1 max-h-[180px] overflow-y-auto">
                {[...alerts].reverse().map((a, i) => (
                  <div
                    key={`${a.alertId}-${i}`}
                    className="flex items-start gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border"
                    style={{ borderColor: severityColor(a.severity) + '40' }}
                  >
                    <span
                      className="text-[10px] px-1.5 py-0.5 rounded flex-shrink-0 font-mono border"
                      style={{
                        color: severityColor(a.severity),
                        borderColor: severityColor(a.severity) + '50',
                        background: severityColor(a.severity) + '15',
                      }}
                    >
                      {a.severity.toUpperCase()}
                    </span>
                    <span className="flex-1 text-xs text-[var(--shield-text-subtle)]">
                      {a.summary}
                    </span>
                    <span className="text-[10px] font-mono text-[var(--shield-text-muted)] flex-shrink-0">
                      {formatRelative(a.firedAt)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {alerts.length === 0 && isRunning && (
            <div className="text-xs opacity-60">No alerts — session looks clean.</div>
          )}

          {/* Detector config accordion */}
          <div className="border-t border-[var(--shield-inner-border)] pt-3">
            <button
              type="button"
              className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
              onClick={() => setConfigExpanded((v) => !v)}
            >
              <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                Detector config
              </span>
              <Icon
                icon={configExpanded ? 'chevron-up' : 'chevron-down'}
                size={12}
                color="var(--shield-text-muted)"
              />
            </button>

            {configExpanded && (
              <div className="mt-3 flex flex-col gap-3">
                <div className="flex flex-col gap-2">
                  <Switch
                    checked={config.checkGaze}
                    label="Gaze direction check"
                    onChange={(e) =>
                      setConfig((c) => ({ ...c, checkGaze: (e.target as HTMLInputElement).checked }))
                    }
                  />
                  <Switch
                    checked={config.checkFaces}
                    label="Secondary faces check"
                    onChange={(e) =>
                      setConfig((c) => ({ ...c, checkFaces: (e.target as HTMLInputElement).checked }))
                    }
                  />
                  <Switch
                    checked={config.checkSecondaryDevice}
                    label="Secondary device / screen check"
                    onChange={(e) =>
                      setConfig((c) => ({
                        ...c,
                        checkSecondaryDevice: (e.target as HTMLInputElement).checked,
                      }))
                    }
                  />
                  <Switch
                    checked={config.silentMode}
                    label="Silent mode — track but suppress on-screen alerts"
                    onChange={(e) =>
                      setConfig((c) => ({
                        ...c,
                        silentMode: (e.target as HTMLInputElement).checked,
                      }))
                    }
                  />
                </div>

                <div className="flex items-center gap-3">
                  <span className="text-xs text-[var(--shield-text-subtle)]">Model level</span>
                  <select
                    className="text-xs rounded border px-2 py-1 bg-[var(--color-bg-secondary)] border-[var(--color-border)] text-[var(--shield-text-subtle)] focus:outline-none"
                    value={config.modelLevel}
                    onChange={(e) =>
                      setConfig((c) => ({
                        ...c,
                        modelLevel: e.target.value as DetectorConfig['modelLevel'],
                      }))
                    }
                  >
                    <option value="fast">Fast (lower accuracy)</option>
                    <option value="balanced">Balanced</option>
                    <option value="accurate">Accurate (higher CPU)</option>
                  </select>
                </div>

                <p className="text-[10px] text-[var(--shield-text-muted)] opacity-70">
                  Config changes apply on next session start. Detectors not active until monitoring is started above.
                </p>
              </div>
            )}
          </div>
        </div>
      )}
    </SectionCard>
  );
}
