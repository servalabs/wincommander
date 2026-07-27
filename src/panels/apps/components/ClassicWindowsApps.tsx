import { useEffect, useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../../../components/ui/alert-dialog";
import { Badge } from "../../../components/ui/badge";
import { Button } from "../../../components/ui/button";
import { Icon } from "../../../components/ui/icon";
import { useAIControlOperations } from "../../../hooks/useAIControlOperations";
import type { AIControlOperationId, AIControlStatus } from "../../../hooks/useBackend";

interface ClassicApp {
  operation: AIControlOperationId;
  statusKey: keyof AIControlStatus["classicApps"];
  name: string;
  description: string;
  note?: string;
}

// Find-AIControlLegacyBinary (ai-control-maintenance.ps1) throws this when no
// Microsoft-signed Paint/Snipping Tool source exists to restore from — the
// normal case on Windows 11, since Microsoft no longer ships these by default
// and WinCommander does not bundle them. Surface it as an explanation, not a
// raw PowerShell error dump.
function isLegacySourceMissingError(error: string): boolean {
  return error.includes("Microsoft-signed") && error.includes("source wasn't found");
}

const LEGACY_SOURCE_MISSING_MESSAGE =
  "Windows 11 no longer includes this app by default, and WinCommander could not find a Microsoft-signed copy on this device or an inserted Windows installation medium to restore it from.";

const CLASSIC_APPS: ClassicApp[] = [
  { operation: "classic-photo-viewer", statusKey: "photoViewer", name: "Windows Photo Viewer", description: "Register the inbox viewer for supported image formats." },
  { operation: "classic-paint", statusKey: "paint", name: "Classic Paint", description: "Restore a Microsoft-signed local Paint binary and Start shortcut.", note: "Requires a compatible signed source if Windows no longer contains it." },
  { operation: "classic-snipping", statusKey: "snipping", name: "Classic Snipping Tool", description: "Restore a Microsoft-signed local Snipping Tool and shortcut.", note: "Requires a compatible signed source if Windows no longer contains it." },
  { operation: "classic-notepad", statusKey: "notepad", name: "Classic Notepad", description: "Replace the Store package with the Windows capability and classic associations." },
  { operation: "photos-legacy", statusKey: "photosLegacy", name: "Photos Legacy", description: "Install Microsoft Photos Legacy from the Store source." },
];

export default function ClassicWindowsApps() {
  const tools = useAIControlOperations();
  const { refresh } = tools;
  const [pending, setPending] = useState<ClassicApp>();

  useEffect(() => { void refresh(); }, [refresh]);

  const install = () => {
    const app = pending;
    setPending(undefined);
    if (app) void tools.run(app.operation, `Install ${app.name}`);
  };

  return (
    <div className="flex flex-col gap-3 px-4 py-4">
      <div>
        <p className="text-sm font-semibold text-[var(--text)]">Classic Windows apps</p>
        <p className="mt-1 text-xs text-[var(--text-dim)]">Restore familiar inbox tools without bundling Windows binaries inside WinCommander.</p>
      </div>
      <div className="grid gap-2 lg:grid-cols-2">
        {CLASSIC_APPS.map((app) => {
          const installed = tools.status?.classicApps[app.statusKey] ?? false;
          return (
            <div key={app.operation} className="flex items-start justify-between gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <p className="text-sm font-medium text-[var(--text)]">{app.name}</p>
                  <Badge tone={installed ? "success" : "neutral"}>{installed ? "available" : "not detected"}</Badge>
                </div>
                <p className="mt-1 text-xs leading-relaxed text-[var(--text-dim)]">{app.description}</p>
                {app.note && <p className="mt-1 text-[11px] text-[var(--text-mute)]">{app.note}</p>}
              </div>
              <Button size="sm" variant="primary" disabled={Boolean(tools.busyOperation)} onClick={() => setPending(app)}>
                <Icon icon="download" />{tools.busyOperation === app.operation ? "Installing…" : installed ? "Repair" : "Install"}
              </Button>
            </div>
          );
        })}
      </div>
      {tools.error && (
        <p className="text-xs text-[var(--danger)]">
          {isLegacySourceMissingError(tools.error) ? LEGACY_SOURCE_MISSING_MESSAGE : tools.error}
        </p>
      )}
      <AlertDialog open={pending !== undefined} onOpenChange={(open) => { if (!open) setPending(undefined); }}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>Install {pending?.name}?</AlertDialogTitle><AlertDialogDescription>{pending?.description} {pending?.note}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={install}>Install</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
