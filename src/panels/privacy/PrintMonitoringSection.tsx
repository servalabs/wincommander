// src/panels/privacy/PrintMonitoringSection.tsx
// Unified print-monitoring surface for the Privacy Monitor.
// Local Print Record may contain Windows-supplied print metadata and must stay local.
// Fleet-safe Print Signals projects the existing Argus payload onto aggregate-only fields.

import { useCallback, useEffect, useId, useState, type ReactNode } from "react";
import { Button, Icon, Spinner, Switch, Tag } from "@/components/ui/bp";
import useEntitlements from "@/hooks/useEntitlements";
import { argus, type ArgusCollectorStatus, type ArgusSignalEntry } from "@/hooks/useArgus";
import { printAudit, type PrintAuditEntry, type PrintAuditStatus } from "@/hooks/usePrintAudit";
import SectionCard from "../../components/shared/SectionCard";

interface FleetSafePrintSignal {
  windowStart: string;
  windowEnd: string;
  magnitude: number;
  severity: string;
}

function optionalText(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function formatTime(iso: string): string {
  if (!iso) return "—";
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return iso;
  return parsed.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function toFleetSafePrintSignals(entries: ArgusSignalEntry[]): FleetSafePrintSignal[] {
  return entries
    .filter((entry) => entry.kind === "print" && entry.class === "print_job")
    .map(({ windowStart, windowEnd, magnitude, severity }) => ({
      windowStart,
      windowEnd,
      magnitude,
      severity,
    }));
}

function InfoButton({ label, children }: { label: string; children: ReactNode }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);

  const togglePinned = () => {
    const next = !pinned;
    setPinned(next);
    setOpen(next);
  };

  return (
    <span className="relative inline-flex items-center">
      <button
        type="button"
        aria-label={label}
        aria-expanded={open}
        aria-describedby={open ? id : undefined}
        className="inline-flex h-5 w-5 items-center justify-center rounded border border-[var(--color-border)] text-[var(--color-text-muted)] hover:text-[var(--color-text)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2"
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => { if (!pinned) setOpen(false); }}
        onFocus={() => setOpen(true)}
        onBlur={() => { if (!pinned) setOpen(false); }}
        onClick={(event) => {
          if ((event.nativeEvent as PointerEvent).pointerType === "touch") return;
          togglePinned();
        }}
        onPointerUp={(event) => {
          if (event.pointerType !== "touch") return;
          event.preventDefault();
          togglePinned();
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            setPinned(false);
            setOpen(false);
            event.stopPropagation();
          }
        }}
      >
        <span aria-hidden="true" className="text-[11px] font-bold leading-none">i</span>
      </button>
      {open && (
        <span
          id={id}
          role="tooltip"
          className="absolute left-0 top-7 z-50 w-72 rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-2 text-[11px] leading-5 text-[var(--color-text-secondary)] shadow-lg"
        >
          {children}
        </span>
      )}
    </span>
  );
}

function StateTag({ state }: { state: "CHECKING" | "RECORDING" | "DISABLED" | "UNAVAILABLE" | "ERROR" | "ACTIVE" | "OFF" }) {
  const intent = state === "RECORDING" || state === "ACTIVE" ? "success" : state === "ERROR" ? "danger" : "none";
  return <Tag minimal intent={intent} className="font-mono text-[10px]">{state}</Tag>;
}

export default function PrintMonitoringSection() {
  const { canUse } = useEntitlements();
  const paid = canUse("paid");

  const [localStatus, setLocalStatus] = useState<PrintAuditStatus | null>(null);
  const [localEntries, setLocalEntries] = useState<PrintAuditEntry[]>([]);
  const [localLoading, setLocalLoading] = useState(false);
  const [localBusy, setLocalBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  const [fleetStatus, setFleetStatus] = useState<ArgusCollectorStatus | null>(null);
  const [fleetSignals, setFleetSignals] = useState<FleetSafePrintSignal[]>([]);
  const [fleetBusy, setFleetBusy] = useState(false);
  const [fleetError, setFleetError] = useState<string | null>(null);

  const refreshLocal = useCallback(async () => {
    if (!paid) return;
    setLocalLoading(true);
    setLocalError(null);
    try {
      const status = await printAudit.status();
      setLocalStatus(status);
      if (status.channelEnabled) {
        const entries = await printAudit.recent(50);
        setLocalEntries(entries);
      } else {
        setLocalEntries([]);
      }
    } catch (error) {
      setLocalError(String(error));
    } finally {
      setLocalLoading(false);
    }
  }, [paid]);

  const refreshFleet = useCallback(async () => {
    if (!paid) return;
    setFleetError(null);
    try {
      const [status, entries] = await Promise.all([
        argus.printUsbStatus(),
        argus.printUsbRecent(),
      ]);
      setFleetStatus(status);
      setFleetSignals(toFleetSafePrintSignals(entries));
    } catch (error) {
      setFleetError(String(error));
    }
  }, [paid]);

  useEffect(() => {
    if (!paid) return;
    void refreshLocal();
    void refreshFleet();
  }, [paid, refreshLocal, refreshFleet]);

  useEffect(() => {
    if (!paid) return;
    const id = window.setInterval(() => void refreshFleet(), fleetStatus?.running ? 10_000 : 30_000);
    return () => window.clearInterval(id);
  }, [paid, fleetStatus?.running, refreshFleet]);

  const toggleLocal = useCallback(async (enabled: boolean) => {
    setLocalBusy(true);
    setLocalError(null);
    try {
      await printAudit.setEnabled(enabled);
      await refreshLocal();
    } catch (error) {
      setLocalError(String(error));
    } finally {
      setLocalBusy(false);
    }
  }, [refreshLocal]);

  const toggleFleet = useCallback(async (enabled: boolean) => {
    setFleetBusy(true);
    setFleetError(null);
    try {
      if (enabled) await argus.printUsbStart();
      else await argus.printUsbStop();
      await refreshFleet();
    } catch (error) {
      setFleetError(String(error));
    } finally {
      setFleetBusy(false);
    }
  }, [refreshFleet]);

  if (!paid) {
    return (
      <SectionCard title="Print Monitoring" icon="print" headerRight={<Tag minimal className="font-mono text-[10px]">PRO</Tag>}>
        <p className="text-xs text-[var(--shield-text-subtle)] opacity-60">
          Local print records and Fleet-safe aggregate print signals require WinCommander Pro.
        </p>
      </SectionCard>
    );
  }

  const localState = localError
    ? "ERROR"
    : localStatus === null
      ? "CHECKING"
      : !localStatus.channelPresent
        ? "UNAVAILABLE"
        : localStatus.channelEnabled
          ? "RECORDING"
          : "DISABLED";

  const fleetState = fleetError
    ? "ERROR"
    : fleetStatus === null
      ? "CHECKING"
      : fleetStatus.running
        ? "ACTIVE"
        : "OFF";

  return (
    <SectionCard title="Print Monitoring" icon="print" headerRight={<Tag minimal className="font-mono text-[10px]">PRO</Tag>}>
      <div className="flex flex-col gap-5">
        <section aria-labelledby="local-print-record-title" className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <h3 id="local-print-record-title" className="text-sm font-semibold">Local Print Record</h3>
              <InfoButton label="About Local Print Record">
                Windows records these entries in the Microsoft-Windows-PrintService/Operational channel. Enabling that channel requires administrator approval once; Windows records future events after that. This detailed record stays local to this device.
              </InfoButton>
            </div>
            <StateTag state={localState} />
          </div>

          <p className="text-xs leading-5 text-[var(--shield-text-subtle)]">
            Reads real Windows Print Service Event 307 data. Windows-supplied document, printer, and user labels are shown only when present; they are never copied into Fleet-safe signals.
          </p>

          <div className="flex flex-wrap items-center gap-3">
            <Switch
              checked={localStatus?.channelEnabled === true}
              disabled={localBusy || localLoading || !localStatus?.channelPresent}
              onChange={(event) => void toggleLocal((event.target as HTMLInputElement).checked)}
              label={localStatus?.channelEnabled ? "Record local print events" : "Enable local print recording"}
            />
            <Button icon="refresh" minimal small disabled={localBusy || localLoading} onClick={() => void refreshLocal()} aria-label="Refresh local print record">
              Refresh
            </Button>
            {(localBusy || localLoading) && <Spinner size={14} />}
          </div>

          {localError && <div role="alert" className="text-xs text-[var(--color-danger,#f87171)]">{localError}</div>}
          {localStatus?.channelPresent === false && !localError && (
            <div className="text-xs text-[var(--color-text-muted)]">Windows Print Service Operational logging is unavailable on this system.</div>
          )}
          {localStatus?.channelPresent && !localStatus.channelEnabled && !localError && (
            <div className="text-xs text-[var(--color-text-muted)]">Recording is disabled. No local print events are being collected by this channel.</div>
          )}

          {localStatus?.channelEnabled && !localLoading && localEntries.length === 0 && !localError && (
            <div className="text-xs text-[var(--color-text-muted)]">No print jobs are recorded yet.</div>
          )}

          {localStatus?.channelEnabled && localEntries.length > 0 && (
            <div className="flex max-h-80 flex-col gap-1 overflow-y-auto" aria-label="Recent local print records">
              {localEntries.map((entry, index) => {
                const document = optionalText(entry.document);
                const printer = optionalText(entry.printer);
                const user = optionalText(entry.user);
                const jobStatus = optionalText(entry.jobStatus);
                return (
                  <div key={`${entry.timeCreated}-${index}`} className="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-2 text-xs">
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                      <span className="font-mono text-[10px] text-[var(--color-text-muted)]">{formatTime(entry.timeCreated)}</span>
                      <span className="font-semibold">{entry.pages} page{entry.pages === 1 ? "" : "s"}</span>
                      {jobStatus && <Tag minimal className="text-[10px]">{jobStatus}</Tag>}
                    </div>
                    {(document || printer || user) && (
                      <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-[11px]">
                        {document && <><dt className="text-[var(--color-text-muted)]">Document</dt><dd className="break-all">{document}</dd></>}
                        {printer && <><dt className="text-[var(--color-text-muted)]">Printer</dt><dd className="break-all">{printer}</dd></>}
                        {user && <><dt className="text-[var(--color-text-muted)]">User</dt><dd className="break-all">{user}</dd></>}
                      </dl>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section aria-labelledby="fleet-print-signals-title" className="flex flex-col gap-3 border-t border-[var(--color-border)] pt-4">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <h3 id="fleet-print-signals-title" className="text-sm font-semibold">Fleet-safe Print Signals</h3>
              <InfoButton label="About Fleet-safe Print Signals">
                Fleet receives only aggregate print signal fields: signal kind/class, time window, page-count magnitude, and severity. Document names, printer names, usernames, paths, and document contents are never sent to Fleet. The current Pro collector is shared with removable-media collection; this section renders only its print signals.
              </InfoButton>
            </div>
            <StateTag state={fleetState} />
          </div>

          <p className="text-xs leading-5 text-[var(--shield-text-subtle)]">
            Privacy boundary: this view uses the existing Argus/Fleet aggregate signal path and deliberately excludes local print metadata.
          </p>

          <div className="flex flex-wrap items-center gap-3">
            <Switch
              checked={fleetStatus?.running === true}
              disabled={fleetBusy || fleetStatus === null}
              onChange={(event) => void toggleFleet((event.target as HTMLInputElement).checked)}
              label="Fleet-safe signal collector active"
            />
            <Button icon="refresh" minimal small disabled={fleetBusy} onClick={() => void refreshFleet()} aria-label="Refresh Fleet-safe print signals">
              Refresh
            </Button>
            {fleetBusy && <Spinner size={14} />}
          </div>

          {fleetError && <div role="alert" className="text-xs text-[var(--color-danger,#f87171)]">{fleetError}</div>}

          {fleetSignals.length > 0 ? (
            <div className="flex max-h-64 flex-col gap-1 overflow-y-auto" aria-label="Recent Fleet-safe print signals">
              {fleetSignals.map((signal, index) => (
                <div key={`${signal.windowEnd}-${index}`} className="flex flex-wrap items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-2 py-1.5 text-xs">
                  <Icon icon="print" size={11} color="var(--color-text-muted)" />
                  <span className="font-mono text-[10px] text-[var(--color-text-muted)]">{formatTime(signal.windowEnd)}</span>
                  <span>{signal.magnitude} page{signal.magnitude === 1 ? "" : "s"}</span>
                  <Tag minimal className="text-[10px]">{signal.severity}</Tag>
                </div>
              ))}
            </div>
          ) : fleetStatus?.running && !fleetError ? (
            <div className="text-xs text-[var(--color-text-muted)]">No Fleet-safe print signals recorded yet.</div>
          ) : null}
        </section>

        <section data-testid="print-watermarking" aria-labelledby="print-watermarking-title" className="flex flex-col gap-3 border-t border-[var(--color-border)] pt-4">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <h3 id="print-watermarking-title" className="text-sm font-semibold">Watermarking</h3>
              <InfoButton label="About print watermarking">
                Visible watermarks require a controlled print or export pipeline before submission. Invisible or forensic markers are valid only for explicitly supported controlled formats. A normal Windows print-event monitor cannot watermark arbitrary jobs after they have been submitted.
              </InfoButton>
            </div>
            <StateTag state="UNAVAILABLE" />
          </div>

          <div className="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-3 text-xs leading-5">
            <div className="font-semibold">Planned — no controlled print/export pipeline is wired to this monitor.</div>
            <div className="mt-1 text-[var(--color-text-muted)]">Visible watermarking: off by default and unavailable here.</div>
            <div className="text-[var(--color-text-muted)]">Invisible/forensic markers: off by default and unavailable here.</div>
            <div className="mt-1 text-[var(--color-text-muted)]">No watermark switch is shown because this monitor cannot truthfully enforce either capability.</div>
          </div>
        </section>
      </div>
    </SectionCard>
  );
}
