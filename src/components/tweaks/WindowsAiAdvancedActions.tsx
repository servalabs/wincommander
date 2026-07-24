import { useEffect, useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../ui/alert-dialog";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Icon } from "../ui/icon";
import { type AIControlMode, useAIControlOperations } from "../../hooks/useAIControlOperations";
import type { AIControlOperationId } from "../../hooks/useBackend";

interface AdvancedAction {
  operation: AIControlOperationId;
  label: string;
  description: string;
  danger?: boolean;
}

const ADVANCED_ACTIONS: AdvancedAction[] = [
  { operation: "package-guard", label: "Reinstall guard", description: "Keep removed AI packages deprovisioned after servicing." },
  { operation: "appx-packages", label: "AI app packages", description: "Remove provisioned and installed Copilot, Recall, AIX, and Core AI workloads.", danger: true },
  { operation: "recall-feature", label: "Recall feature payload", description: "Remove the optional Recall feature payload with DISM.", danger: true },
  { operation: "cbs-packages", label: "AI system packages", description: "Remove matching component-store packages; repair uses DISM.", danger: true },
  { operation: "ai-files", label: "Remaining AI files", description: "Back up and remove residual AI binaries, folders, handlers, and Office AI files.", danger: true },
  { operation: "scheduled-tasks", label: "AI scheduled tasks", description: "Export and remove Windows AI and Office Actions tasks." },
  { operation: "update-cleanup", label: "Post-update cleanup", description: "Run a bounded cleanup after Windows servicing changes the build." },
];

interface PendingAction extends AdvancedAction {
  mode: AIControlMode;
}

export default function WindowsAiAdvancedActions() {
  const tools = useAIControlOperations();
  const { refresh } = tools;
  const [pending, setPending] = useState<PendingAction>();

  useEffect(() => { void refresh(); }, [refresh]);

  const confirm = () => {
    const action = pending;
    setPending(undefined);
    if (action) void tools.run(action.operation, `${action.mode === "revert" ? "Restore" : "Apply"} ${action.label}`, action.mode);
  };

  return (
    <div className="mt-4 flex flex-col gap-3 border-t border-[var(--border)] pt-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-sm font-semibold text-[var(--text)]">Advanced AI component cleanup</p>
          <p className="text-xs text-[var(--text-dim)]">The main toggle above handles policies and app-level AI features. These deeper removals stay explicit.</p>
        </div>
        <div className="flex items-center gap-2">
          {tools.status && <Badge tone={tools.status.isAdmin ? "success" : "warning"}>{tools.status.isAdmin ? "administrator" : "admin required"}</Badge>}
          <Button size="sm" variant="outline" disabled={Boolean(tools.busyOperation)} onClick={() => void tools.run("restore-point", "Create restore point")}>
            <Icon icon="history" /> Restore point
          </Button>
        </div>
      </div>

      <div className="grid gap-2 xl:grid-cols-2">
        {ADVANCED_ACTIONS.map((action) => (
          <div key={action.operation} className="flex items-start justify-between gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3">
            <div className="min-w-0">
              <p className="text-sm font-medium text-[var(--text)]">{action.label}</p>
              <p className="mt-1 text-xs leading-relaxed text-[var(--text-dim)]">{action.description}</p>
            </div>
            <div className="flex shrink-0 flex-col gap-1">
              <Button
                size="sm"
                variant={action.danger ? "danger" : "default"}
                disabled={Boolean(tools.busyOperation)}
                onClick={() => setPending({ ...action, mode: "apply" })}
              >
                {tools.busyOperation === action.operation ? "Working…" : "Apply"}
              </Button>
              <Button size="sm" variant="ghost" disabled={Boolean(tools.busyOperation)} onClick={() => setPending({ ...action, mode: "revert" })}>
                Restore
              </Button>
            </div>
          </div>
        ))}
      </div>

      <details className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2 text-xs text-[var(--text-dim)]">
        <summary className="cursor-pointer font-medium text-[var(--text)]">Manual cleanup checks</summary>
        <p className="mt-2 leading-relaxed">Review OneDrive People, Windows Studio Effects, Teams, Outlook, and any Copilot app-bar entries after reboot. Windows exposes these inconsistently, so WinCommander does not delete them without detection.</p>
      </details>

      {tools.error && <p className="text-xs text-[var(--danger)]">{tools.error}</p>}

      <AlertDialog open={pending !== undefined} onOpenChange={(open) => { if (!open) setPending(undefined); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{pending?.mode === "revert" ? "Restore" : "Apply"} {pending?.label}?</AlertDialogTitle>
            <AlertDialogDescription>
              {pending?.mode === "revert"
                ? "WinCommander will use its retained backup or the Windows component repair path. Restores can be best effort after system updates."
                : `${pending?.description} A reboot may be required. Create a restore point first for deep component removals.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={confirm}>Continue</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
