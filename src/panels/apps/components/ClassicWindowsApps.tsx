import { useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../../../components/ui/alert-dialog";
import { Badge } from "../../../components/ui/badge";
import { Button } from "../../../components/ui/button";
import { Icon } from "../../../components/ui/icon";
import { useAIControlOperations } from "../../../hooks/useAIControlOperations";
import {
  type ClassicWindowsAppsCheckState,
  useClassicWindowsAppsStatus,
} from "../../../hooks/useClassicWindowsAppsStatus";
import type { AIControlOperationId, AIControlStatus } from "../../../hooks/useBackend";

interface ClassicApp {
  operation: AIControlOperationId;
  statusKey: keyof AIControlStatus["classicApps"];
  name: string;
  description: string;
  icon: "image" | "application";
}

const CLASSIC_APPS: ClassicApp[] = [
  {
    operation: "classic-photo-viewer",
    statusKey: "photoViewer",
    name: "Windows Photo Viewer",
    description: "Enable the familiar Windows viewer for common image formats.",
    icon: "image",
  },
  {
    operation: "photos-legacy",
    statusKey: "photosLegacy",
    name: "Photos Legacy",
    description: "Install Microsoft's previous Photos experience from Microsoft Store.",
    icon: "application",
  },
];

function formatCheckedTime(lastCheckedAt: Date | undefined): string {
  if (!lastCheckedAt) return "Checking now";
  return `Last checked ${lastCheckedAt.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`;
}

function statusBadge(checkState: ClassicWindowsAppsCheckState, isAvailable: boolean) {
  if (checkState === "checking") {
    return <Badge tone="neutral"><Icon icon="refresh" size={12} className="animate-spin" />Checking</Badge>;
  }
  if (checkState === "failed") {
    return <Badge tone="danger"><Icon icon="warning-sign" size={12} />Check failed</Badge>;
  }
  if (isAvailable) {
    return <Badge tone="success"><Icon icon="tick" size={12} />Available</Badge>;
  }
  return <Badge tone="neutral"><Icon icon="cross" size={12} />Not installed</Badge>;
}

export default function ClassicWindowsApps() {
  const tools = useAIControlOperations();
  const appStatus = useClassicWindowsAppsStatus();
  const [pending, setPending] = useState<ClassicApp>();

  const install = async () => {
    const app = pending;
    setPending(undefined);
    if (!app) return;
    const succeeded = await tools.run(app.operation, `Install ${app.name}`);
    if (succeeded) await appStatus.check();
  };

  return (
    <div className="flex flex-col gap-3 px-4 py-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="text-sm font-semibold text-[var(--text)]">Classic Windows apps</p>
          <p className="mt-1 text-xs text-[var(--text-dim)]">Bring back familiar Microsoft photo experiences using components supplied by Windows or Microsoft Store.</p>
          <p className="mt-1 text-[11px] text-[var(--text-mute)]">{formatCheckedTime(appStatus.lastCheckedAt)}</p>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={appStatus.checkState === "checking" || Boolean(tools.busyOperation)}
          onClick={() => void appStatus.check()}
        >
          <Icon icon="refresh" className={appStatus.checkState === "checking" ? "animate-spin" : undefined} />
          {appStatus.checkState === "checking" ? "Checking…" : "Check again"}
        </Button>
      </div>

      <div className="grid gap-2 lg:grid-cols-2">
        {CLASSIC_APPS.map((app) => {
          const installed = appStatus.status?.classicApps[app.statusKey] ?? false;
          return (
            <div key={app.operation} className="flex items-start justify-between gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3">
              <div className="flex min-w-0 gap-3">
                <span className="flex size-9 shrink-0 items-center justify-center rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-3)] text-[var(--accent)]">
                  <Icon icon={app.icon} />
                </span>
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="text-sm font-medium text-[var(--text)]">{app.name}</p>
                    {statusBadge(appStatus.checkState, installed)}
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-[var(--text-dim)]">{app.description}</p>
                </div>
              </div>
              <Button
                size="sm"
                variant="primary"
                disabled={appStatus.checkState !== "ready" || Boolean(tools.busyOperation)}
                onClick={() => setPending(app)}
              >
                <Icon icon="download" />{tools.busyOperation === app.operation ? "Installing…" : installed ? "Repair" : "Install"}
              </Button>
            </div>
          );
        })}
      </div>

      {appStatus.checkError && <p className="text-xs text-[var(--danger)]">{appStatus.checkError} Use Check again to retry.</p>}
      {tools.error && <p className="text-xs text-[var(--danger)]">{tools.error}</p>}

      <AlertDialog open={pending !== undefined} onOpenChange={(open) => { if (!open) setPending(undefined); }}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>Install {pending?.name}?</AlertDialogTitle><AlertDialogDescription>{pending?.description}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => void install()}>Install</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
