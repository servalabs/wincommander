import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Spinner } from "@/components/ui/bp";
import { executeBackendCommand } from "../../hooks/useBackend";
import SectionCard from "../shared/SectionCard";
import { useAppConfirm } from "../shared/AppConfirmDialog";

interface RestartPolicyStatus {
  isAdmin: boolean;
  configured: boolean;
  policyExists: boolean;
  owned: boolean;
  reserved: boolean;
  conflict: boolean;
  scheduledMode: boolean;
  canPrepareSchedule: boolean;
  scheduleOwned: boolean;
  scheduleReserved: boolean;
}

export default function UpdateRestartCard() {
  const confirm = useAppConfirm();
  const [status, setStatus] = useState<RestartPolicyStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);

  const refresh = useCallback(async () => {
    const result = await executeBackendCommand<RestartPolicyStatus>("Get-UpdateRestartPolicyStatus");
    if (result.success && result.data) setStatus(result.data);
    else { setStatus(null); setError(result.error || "Windows could not read this setting."); }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const change = async (mode: "apply" | "applyWithSchedule" | "revert") => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const result = await executeBackendCommand<RestartPolicyStatus>("Set-UpdateRestartPolicy", { Mode: mode });
      if (result.success && result.data) setStatus(result.data);
      else { setError(result.error || "Windows could not change this setting."); await refresh(); }
    } finally { pending.current = false; setBusy(false); }
  };

  const prepareSchedule = async () => {
    const accepted = await confirm({
      title: "Schedule Windows updates?",
      description: "Windows requires scheduled update installation for this restart setting. This will allow Windows to download and install updates on its configured schedule; if no time is set, Windows normally uses 3 AM. Restoring this option removes the schedule setting WinCommander adds.",
      confirmLabel: "Set schedule and prevent restart",
    });
    if (accepted) await change("applyWithSchedule");
  };

  return (
    <SectionCard title="Windows Update restarts" icon="refresh">
      <div className="flex flex-col gap-3 text-sm">
        <p>Ask Windows not to restart for scheduled updates while someone is signed in.</p>
        {!status && !error && <div className="flex items-center gap-2"><Spinner size={16} /> Reading Windows settings…</div>}
        {status && <>
          <p className="text-xs text-[var(--text-mute)]">
            {status.conflict ? "The setting changed outside WinCommander or a previous change was interrupted. Review it in Windows before continuing."
              : status.owned && status.configured ? "The policy is set by WinCommander."
              : status.policyExists ? "This setting is already controlled outside WinCommander."
              : status.reserved ? "A previous change was interrupted. You can retry or clear the unfinished change."
              : "This policy is not set."}
          </p>
          {!status.scheduledMode && <p className="text-xs text-[var(--color-warning)]">This policy requires scheduled installation. You can choose to configure it below when no other update policy is already set.</p>}
          {status.scheduleOwned && <p className="text-xs text-[var(--text-mute)]">WinCommander set scheduled installation for this control. Restore removes that added schedule setting too.</p>}
          {!status.isAdmin && <p className="text-xs text-[var(--color-warning)]">Open WinCommander as administrator to change this setting.</p>}
          <div className="flex flex-wrap gap-2">
            <Button small text="Prevent scheduled restart" disabled={busy || !status.isAdmin || !status.scheduledMode || status.conflict || status.policyExists} onClick={() => void change("apply")} />
            {!status.scheduledMode && <Button small text={status.scheduleReserved ? "Retry schedule and prevent restart" : "Set schedule and prevent restart"} disabled={busy || !status.isAdmin || !status.canPrepareSchedule || status.conflict || status.policyExists} onClick={() => void prepareSchedule()} />}
            <Button small text="Restore previous setting" disabled={busy || !status.isAdmin || status.conflict || !(status.owned || status.reserved || status.scheduleOwned || status.scheduleReserved)} onClick={() => void change("revert")} />
            <Button small minimal icon="refresh" text="Refresh" disabled={busy} onClick={() => { setError(null); void refresh(); }} />
          </div>
        </>}
        {error && <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p>}
        {!status && error && <div><Button small minimal icon="refresh" text="Try again" disabled={busy} onClick={() => { setError(null); void refresh(); }} /></div>}
        <p className="text-xs text-[var(--text-mute)]">Other update policies can affect the result. This does not stop manual restarts or every kind of automatic restart. Updates may need a restart to finish.</p>
      </div>
    </SectionCard>
  );
}
