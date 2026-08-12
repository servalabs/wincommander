import { useEffect, useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../ui/alert-dialog";
import { Badge } from "../ui/badge";
import { Icon, type IconName } from "../ui/icon";
import { Switch } from "../ui/switch";
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

      <div className="grid gap-2 xl:grid-cols-2">
        {ADVANCED_ACTIONS.map((action) => {
          const enabled = desiredState[action.operation] === true;
          const busy = tools.busyOperation === action.operation;
          return (
            <div key={action.operation} className="flex items-start justify-between gap-3 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <Icon icon={action.icon} size={16} className="shrink-0 text-[var(--color-danger)]" aria-hidden="true" />
                  <p className="text-[13px] font-bold text-[var(--text)]">{action.label}</p>
                  <span className={`text-[11px] font-medium ${busy ? "text-[var(--text-dim)]" : enabled ? "text-[var(--success)]" : "text-[var(--text-dim)]"}`}>
                    {busy ? "Applying…" : enabled ? "Enabled" : "Off"}
                  </span>
                </div>
                <p className="mt-1 text-[12.5px] leading-relaxed text-[var(--text-dim)]">{action.description}</p>
              </div>
              <Switch
                checked={enabled}
                disabled={Boolean(tools.busyOperation)}
                aria-label={`${enabled ? "Disable" : "Enable"} ${action.label}`}
                onCheckedChange={(next) => requestChange(action, next)}
              />
            </div>
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
