// src/panels/privacy/GlassesMonitorSection.tsx
//
// Smart-glasses monitor — background BLE scan for camera-capable glasses
// (Ray-Ban/Oakley Meta, Snap Spectacles, Vuzix, HeyCyan clones). The
// persisted ON/OFF lives in privacy.glassesMonitor and useGlassesMonitor
// reconciles the Rust watcher; this card only shows status and recent hits.

import { Switch, Button, Tag } from "@/components/ui/bp";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { clearGlassesMonitorRecent, getGlassesMonitorRecent, getGlassesMonitorStatus, type GlassesDetection, type GlassesMonitorStatus } from "../../hooks/useGlassesMonitor";
import SectionCard from "../../components/shared/SectionCard";
import PrivacyEventTable from "./PrivacyEventTable";

interface Props {
  enabled: boolean;
  onPatch: (patch: { enabled: boolean }) => void;
}

const VENDOR_LABELS: Record<string, string> = {
  meta: "Meta / Ray-Ban Meta",
  luxottica: "Ray-Ban / Oakley Meta",
  snap: "Snap Spectacles",
  vuzix: "Vuzix",
  heycyan: "HeyCyan camera glasses",
};

function formatTime(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? iso
    : date.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function statusTag(enabled: boolean, status: GlassesMonitorStatus | null, hits: number) {
  if (!enabled) return { intent: "none" as const, text: "OFF" };
  if (status?.radio === "radio_unavailable") return { intent: "warning" as const, text: "BLUETOOTH OFF" };
  if (status?.radio === "error") return { intent: "warning" as const, text: "DEGRADED" };
  if (hits > 0) return { intent: "danger" as const, text: `DETECTED · ${hits}` };
  if (status?.radio === "scanning") return { intent: "success" as const, text: "SCANNING" };
  return { intent: "none" as const, text: "STARTING…" };
}

export default function GlassesMonitorSection({ enabled, onPatch }: Props) {
  const [status, setStatus] = useState<GlassesMonitorStatus | null>(null);
  const [recent, setRecent] = useState<GlassesDetection[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextRecent] = await Promise.all([
        getGlassesMonitorStatus(),
        getGlassesMonitorRecent(),
      ]);
      setStatus(nextStatus);
      setRecent(nextRecent);
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
    if (!enabled) return;
    const timer = setInterval(() => void refresh(), 10_000);
    const unlisten = listen("glasses-detected", () => void refresh());
    return () => {
      clearInterval(timer);
      void unlisten.then((stop) => stop());
    };
  }, [enabled, refresh]);

  const clearRecent = async () => {
    await clearGlassesMonitorRecent().catch(() => undefined);
    void refresh();
  };

  const tag = statusTag(enabled, status, recent.length);

  return (
    <SectionCard
      title="Smart glasses nearby"
      icon="eye-open"
      headerRight={<Tag minimal intent={tag.intent} className="font-mono">{tag.text}</Tag>}
      armed={enabled}
    >
      <div className="flex items-start justify-between gap-4">
        <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[380px]">
          Scans Bluetooth in the background and warns when camera-capable glasses (Ray-Ban/Oakley
          Meta, Snap Spectacles, Vuzix) are within roughly 10 m. A warning means such glasses are
          likely nearby, not that you are being recorded. Glasses that are switched off are not seen.
        </p>
        <Switch
          checked={enabled}
          onChange={(e) => onPatch({ enabled: e.currentTarget.checked })}
          aria-label="Enable smart-glasses detection"
        />
      </div>

      {enabled && status?.radio === "radio_unavailable" && (
        <p role="alert" className="mt-2 text-xs text-[var(--warn)]">
          Bluetooth is off, missing, or blocked by policy, so nothing can be detected. Scanning
          resumes automatically when Bluetooth is available.
        </p>
      )}

      {enabled && (
        <div className="mt-4 border-t border-[var(--shield-inner-border)] pt-4">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-semibold text-[var(--shield-text-primary)]">
              Recent detections ({recent.length})
            </span>
            {recent.length > 0 && (
              <Button minimal small icon="trash" onClick={() => void clearRecent()} aria-label="Clear smart-glasses detections">
                Clear
              </Button>
            )}
          </div>
          {recent.length === 0 ? (
            <p className="mt-2 text-xs text-[var(--shield-text-muted)]">
              No smart glasses seen since WinCommander started.
            </p>
          ) : (
            <PrivacyEventTable
              title="Smart-glasses detections"
              columns={["Time", "Glasses", "Confidence", "Signal"]}
              rows={recent.map((hit, i) => ({
                id: `${hit.detectedAt}-${i}`,
                search: `${hit.vendor} ${hit.confidence}`,
                sort: [hit.detectedAt, hit.vendor, hit.confidence, String(hit.rssi)],
                cells: [
                  formatTime(hit.detectedAt),
                  VENDOR_LABELS[hit.vendor] ?? hit.vendor,
                  <Tag minimal intent={hit.confidence === "high" ? "danger" : "warning"}>{hit.confidence.toUpperCase()}</Tag>,
                  `${hit.rssi} dBm`,
                ],
              }))}
            />
          )}
        </div>
      )}
    </SectionCard>
  );
}
