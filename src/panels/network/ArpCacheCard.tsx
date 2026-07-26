// Neighbour cache (ARP) — one card for what used to be three disconnected
// fragments: a counts card, a separate rows card, and an unanchored floating
// "Clear dynamic entries" button. Header + reading + rows + footer action.
import { useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import EmptyState from "../../components/shared/EmptyState";
import {
  ARP_SCAN_TTL_MS,
  describeArpClear,
  findSuspiciousMacs,
  formatArpTableForClipboard,
  summarizeArpScan,
} from "../../lib/arpDiagnostics";
import { ArpEntriesTable } from "./ArpEntriesTable";
import { DangerConfirmDialog } from "./DangerConfirmDialog";
import { InfoPopover, InfoTip } from "./InfoTip";
import { MaintenanceNotice, TableSkeleton } from "./MaintenanceNotice";
import type { useArpMaintenance } from "./useArpMaintenance";

const TTL_MINUTES = Math.round(ARP_SCAN_TTL_MS / 60_000);

function ageLabel(ageMs: number): string {
  const minutes = Math.floor(ageMs / 60_000);
  if (minutes < 1) return "just now";
  if (minutes === 1) return "1 min ago";
  return `${minutes} min ago`;
}

export function ArpCacheCard({ arp }: { arp: ReturnType<typeof useArpMaintenance> }) {
  const [confirmClear, setConfirmClear] = useState(false);
  const [copied, setCopied] = useState(false);

  const scan = arp.scan;
  const summary = useMemo(() => (scan ? summarizeArpScan(scan) : null), [scan]);
  const suspiciousCount = useMemo(
    () => (scan ? findSuspiciousMacs(scan.entries).size : 0),
    [scan],
  );

  const copyTable = async () => {
    if (!scan) return;
    await navigator.clipboard.writeText(formatArpTableForClipboard(scan.entries)).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <CardTitle className="flex items-center gap-1.5">
              Neighbour cache (ARP)
              <InfoTip
                label="What the neighbour cache is"
                content="Windows' short-term map of which IP address belongs to which piece of hardware on this network. Every PC keeps one; it is not a setting you configure."
              />
            </CardTitle>
            <CardDescription className="mt-1">
              Read the IP-to-hardware map, then clear the entries Windows learned by itself. Static
              entries are always preserved.
            </CardDescription>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {scan ? (
              <>
                <span className="text-[11px] text-[var(--text-mute)]">
                  Scanned {ageLabel(arp.ageMs)}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={arp.busy}
                  onClick={() => void copyTable()}
                >
                  <Icon icon={copied ? "tick" : "duplicate"} />
                  {copied ? "Copied" : "Copy"}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={arp.busy}
                  onClick={() => void arp.inspect()}
                >
                  <Icon icon="refresh" className={arp.inspecting ? "animate-spin" : undefined} />
                  Rescan
                </Button>
              </>
            ) : null}
            <InfoPopover title="Neighbour cache (ARP)">
              <p>
                Before this PC can send a packet to another device on the same network it has to know
                that device's hardware (MAC) address. It asks once, then remembers the answer here.
                That remembered list is the ARP cache — also called the neighbour cache.
              </p>
              <p>
                <strong className="text-[var(--text)]">Learned</strong> entries were discovered
                automatically and expire on their own.{" "}
                <strong className="text-[var(--text)]">Static</strong> entries are Windows' own
                multicast and broadcast plumbing and are never removed.
              </p>
              <p>
                Clearing the learned entries is safe and self-healing — Windows re-asks and rebuilds
                the list within seconds. It is worth doing when a device is stuck on an old IP after
                a router swap, when Windows reports a duplicate IP, or when one machine on the
                network is unreachable while the internet still works. Live connections may pause for
                a moment while the list rebuilds.
              </p>
            </InfoPopover>
          </div>
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-3">
        {arp.error && !scan ? (
          <MaintenanceNotice
            tone="danger"
            headline={arp.error.title}
            action={
              <Button variant="outline" size="sm" disabled={arp.busy} onClick={() => void arp.inspect()}>
                <Icon icon="refresh" />
                Try again
              </Button>
            }
          >
            {arp.error.hint}
          </MaintenanceNotice>
        ) : null}

        {!scan && arp.inspecting ? (
          <TableSkeleton label="Reading the neighbour table…" />
        ) : null}

        {!scan && !arp.inspecting && !arp.error ? (
          <EmptyState
            icon="search"
            title="Neighbour cache not read yet"
            hint="Reads the local IP-to-hardware map so you can spot stale entries and clear the ones Windows learned by itself."
            action={
              <Button variant="primary" onClick={() => void arp.inspect()}>
                <Icon icon="search" />
                Inspect neighbour cache
              </Button>
            }
          />
        ) : null}

        {scan && summary ? (
          <>
            {arp.isStale ? (
              <MaintenanceNotice
                tone="warning"
                headline="This snapshot has expired"
                action={
                  <Button variant="primary" size="sm" disabled={arp.busy} onClick={() => void arp.inspect()}>
                    <Icon icon="refresh" />
                    Rescan
                  </Button>
                }
              >
                Windows only accepts a clear against a scan taken in the last {TTL_MINUTES} minutes.
                Rescan before clearing.
              </MaintenanceNotice>
            ) : (
              <MaintenanceNotice tone={summary.intent} headline={summary.headline}>
                {summary.detail}
              </MaintenanceNotice>
            )}

            <div className="flex flex-wrap items-center gap-2">
              <Badge tone="neutral">{scan.entries.length} entries</Badge>
              <Badge tone="neutral">{scan.dynamicEntries} learned</Badge>
              <Badge tone="accent">{scan.entries.length - scan.dynamicEntries} static</Badge>
              {suspiciousCount > 0 ? (
                <Badge tone="warning">{suspiciousCount} duplicate MAC</Badge>
              ) : null}
            </div>

            {scan.entries.length > 0 ? (
              <ArpEntriesTable entries={scan.entries} />
            ) : (
              <EmptyState
                icon="globe-network"
                title="No neighbours cached"
                hint="This PC has not exchanged traffic with anything on the local network yet, or every adapter is down. Nothing to clear."
              />
            )}

            {arp.error ? (
              <MaintenanceNotice tone="danger" headline={arp.error.title}>
                {arp.error.hint}
              </MaintenanceNotice>
            ) : null}

            {arp.result ? (
              <MaintenanceNotice tone="success" headline="Cache cleared">
                {describeArpClear(arp.result)}
              </MaintenanceNotice>
            ) : null}

            <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--border)] pt-3">
              <p className="text-[11px] leading-relaxed text-[var(--text-mute)]">
                {scan.dynamicEntries > 0
                  ? "Clearing removes only the learned entries. Windows rebuilds them automatically."
                  : "Nothing to clear — every entry here is static."}
              </p>
              <Button
                variant="danger"
                size="sm"
                disabled={arp.busy || arp.isStale || scan.dynamicEntries === 0}
                onClick={() => setConfirmClear(true)}
              >
                <Icon icon={arp.clearing ? "refresh" : "trash"} className={arp.clearing ? "animate-spin" : undefined} />
                {arp.clearing
                  ? "Clearing…"
                  : `Clear ${scan.dynamicEntries} learned ${scan.dynamicEntries === 1 ? "entry" : "entries"}`}
              </Button>
            </div>
          </>
        ) : null}
      </CardContent>

      <DangerConfirmDialog
        open={confirmClear}
        onOpenChange={setConfirmClear}
        onConfirm={() => {
          setConfirmClear(false);
          void arp.clear();
        }}
        title="Clear learned neighbour entries?"
        intro="This is a troubleshooting step, not a security change — Windows rebuilds the list on its own."
        consequences={[
          `${scan?.dynamicEntries ?? 0} automatically-learned ${(scan?.dynamicEntries ?? 0) === 1 ? "entry is" : "entries are"} removed`,
          "Static entries configured by Windows are left alone",
          "Live connections can pause for a moment while Windows re-asks for hardware addresses",
          "Nothing is deleted from disk and no setting changes — the list rebuilds within seconds",
        ]}
        actionLabel="Clear cache"
        countdownSeconds={0}
      />
    </Card>
  );
}
