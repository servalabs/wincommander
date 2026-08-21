import { useEffect, useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../ui/alert-dialog";
import { Badge } from "../ui/badge";
import { Icon, type IconName } from "../ui/icon";
import UniversalToggle from "../shared/UniversalToggle";
import { useAppState } from "../../context/AppContext";
import { type AiComponentCleanupOperation } from "../../types/settings";
import { useAIControlOperations } from "../../hooks/useAIControlOperations";

interface AdvancedAction {
  operation: AiComponentCleanupOperation;
  label: string;
  description: string;
  icon: IconName;
  needsConfirmation?: boolean;
}

const ADVANCED_ACTIONS: AdvancedAction[] = [
  { operation: "package-guard", label: "Reinstall guard", description: "Keeps removed AI packages deprovisioned after Windows servicing.", icon: "shield-check" },
  { operation: "appx-packages", label: "AI app packages", description: "Removes Copilot, Recall, AIX, and Core AI app packages.", icon: "applications", needsConfirmation: true },
  { operation: "recall-feature", label: "Recall feature payload", description: "Removes the optional Windows Recall feature payload.", icon: "eye-open", needsConfirmation: true },
  { operation: "cbs-packages", label: "AI system packages", description: "Removes matching Windows component-store packages.", icon: "cube", needsConfirmation: true },
  { operation: "ai-files", label: "Remaining AI files", description: "Backs up and removes residual AI files and handlers.", icon: "document", needsConfirmation: true },
  { operation: "scheduled-tasks", label: "AI scheduled tasks", description: "Disables Windows AI and Office Actions scheduled tasks.", icon: "properties" },
  { operation: "update-cleanup", label: "Post-update cleanup", description: "Runs a bounded cleanup after Windows servicing changes the build.", icon: "refresh" },
];

interface PendingChange {
  action: AdvancedAction;
  enabled: boolean;
}

export default function WindowsAiAdvancedActions() {
  const { appSettings, patchAppSettings } = useAppState();
  const tools = useAIControlOperations();
  const { refresh } = tools;
  const [pendingChange, setPendingChange] = useState<PendingChange>();

  useEffect(() => { void refresh(); }, [refresh]);

  const desiredState = appSettings?.ideal?.tweaks?.aiComponentCleanup ?? {};

  const applyChange = async (change: PendingChange) => {
    setPendingChange(undefined);
    const { action, enabled } = change;
    const label = `${enabled ? "Enable" : "Restore"} ${action.label}`;
    const completed = await tools.run(action.operation, label, enabled ? "apply" : "revert");
    if (!completed) return;
    await patchAppSettings({
      ideal: {
        tweaks: {
          aiComponentCleanup: {
            ...desiredState,
            [action.operation]: enabled,
          },
        },
      },
    });
  };

  const requestChange = (action: AdvancedAction, enabled: boolean) => {
    const change = { action, enabled };
    if (enabled && action.needsConfirmation) {
      setPendingChange(change);
      return;
    }
    void applyChange(change);
  };

  return (
    <div className="mt-4 flex flex-col gap-3 border-t border-[var(--border)] pt-4">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <p className="text-sm font-semibold text-[var(--text)]">Advanced AI component cleanup</p>
          <p className="mt-1 text-xs text-[var(--text-dim)]">Turn on only the parts you want removed. Each control can be restored on its own.</p>
        </div>
        {tools.status && <Badge tone={tools.status.isAdmin ? "success" : "warning"}>{tools.status.isAdmin ? "administrator" : "admin required"}</Badge>}
      </div>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 2xl:grid-cols-3">
        {ADVANCED_ACTIONS.map((action) => {
          const enabled = desiredState[action.operation] === true;
          const busy = tools.busyOperation === action.operation;
          return (
            <UniversalToggle
              key={action.operation}
              label={action.label}
              description={action.description}
              icon={action.icon}
              domain="tweaks"
              size="compact"
              checked={enabled}
              loading={busy}
              disabled={Boolean(tools.busyOperation)}
              onChange={(next) => requestChange(action, next)}
            />
          );
        })}
      </div>

      {tools.error && (
        <p className="flex items-center gap-1.5 text-xs text-[var(--danger)]"><Icon icon="warning-sign" size={14} />{tools.error}</p>
      )}

      <AlertDialog open={pendingChange !== undefined} onOpenChange={(open) => { if (!open) setPendingChange(undefined); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Enable {pendingChange?.action.label}?</AlertDialogTitle>
            <AlertDialogDescription>
              {pendingChange?.action.description} WinCommander saves a recovery copy where Windows supports it. A restart may be required.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Back</AlertDialogCancel>
            <AlertDialogAction onClick={() => { if (pendingChange) void applyChange(pendingChange); }}>Enable</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
