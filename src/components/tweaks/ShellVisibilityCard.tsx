import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/bp";
import SectionCard from "../shared/SectionCard";
import { executeBackendCommand } from "../../hooks/useBackend";
import useEntitlements from "../../hooks/useEntitlements";

const items = [["StartRecommended", "Start recommended section"], ["SettingsHome", "Settings Home page"]] as const;
export function shellVisibilityMatches(query: string): boolean {
  const q = query.trim().toLowerCase();
  return !q || ["hide start settings home recommendations", ...items.map(([, label]) => label.toLowerCase())].some(term => term.includes(q));
}
interface VisibilityItem {
  item: string; eligible: boolean; owned: boolean; pending: boolean; conflict: boolean;
  userPolicy: boolean; existing: boolean; externalHidden: boolean; canApply: boolean;
}
interface VisibilityStatus { isAdmin: boolean; items: VisibilityItem[] }
interface VisibilityReceipt { verified: boolean }

export default function ShellVisibilityCard({ searchQuery }: { searchQuery: string }) {
  const { isInvestigator } = useEntitlements();
  const [status, setStatus] = useState<VisibilityStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  const refresh = useCallback(async () => {
    const result = await executeBackendCommand<VisibilityStatus>("Get-ShellVisibilityStatus");
    if (!result.success || !result.data || !Array.isArray(result.data.items)) { setStatus(null); throw new Error(result.error || "Windows could not read these settings."); }
    setStatus(result.data);
  }, []);
  useEffect(() => { void refresh().catch((e: unknown) => setError(String(e))); }, [refresh]);
  const change = async (item: VisibilityItem, mode: "hide" | "restore") => {
    if (running.current) return;
    running.current = true; setBusy(true); setError(null);
    let failure: string | null = null;
    try {
      const result = await executeBackendCommand<VisibilityReceipt>("Set-ShellVisibility", { Item: item.item, Mode: mode });
      if (!result.success || result.data?.verified !== true) throw new Error(result.error || "Windows could not verify the change.");
    } catch (e) { failure = String(e); }
    try { await refresh(); } catch (e) { failure = failure ? `${failure} Refresh also failed: ${String(e)}` : String(e); }
    finally { setError(failure); setBusy(false); running.current = false; }
  };
  const visible = items.filter(([, label]) => !searchQuery.trim() || "hide start settings home recommendations".includes(searchQuery.trim().toLowerCase()) || label.toLowerCase().includes(searchQuery.trim().toLowerCase()));
  return <SectionCard title="Start and Settings visibility" icon="applications">
    <div className="flex flex-col gap-3 text-sm">
      <p>Choose which Windows sections to hide. Restore returns the previous policy.</p>
      <p className="text-xs text-[var(--text-mute)]">These options require a supported Windows 11 edition. Windows may need a sign-out or restart, and other policies can affect the result.</p>
      {status && visible.map(([key, label]) => {
        const item = status.items.find(row => row.item === key);
        if (!item) return null;
        const reading = item.conflict ? "Changed elsewhere — review needed" : item.userPolicy ? "Controlled by an account policy" : !item.eligible ? "Not supported on this edition or version" : item.owned ? "Hide policy set by WinCommander" : item.pending ? "Previous change interrupted" : item.externalHidden ? "Already hidden by another policy" : item.existing && !item.canApply ? "Controlled by another policy" : "Not hidden by WinCommander";
        const disabled = busy || isInvestigator || !status.isAdmin || item.conflict || item.userPolicy || (!item.owned && (!item.eligible || !item.canApply));
        const cancelDisabled = busy || isInvestigator || !status.isAdmin || item.conflict;
        return <div key={key} className="flex flex-wrap items-center justify-between gap-2"><div><div className="font-medium">{label}</div><div className="text-xs text-[var(--text-mute)]">{reading}</div></div><div className="flex gap-2">
          {!item.owned && <Button small disabled={disabled} onClick={() => void change(item, "hide")}>{item.pending ? "Retry hide" : "Hide"}</Button>}
          {(item.owned || item.pending) && <Button small disabled={item.pending ? cancelDisabled : disabled} onClick={() => void change(item, "restore")}>{item.pending ? "Cancel change" : "Restore"}</Button>}
        </div></div>;
      })}
      {status && !status.isAdmin && <p className="text-xs text-[var(--color-warning)]">Open WinCommander as administrator to change these settings for the PC.</p>}
      {isInvestigator && <p>Changes are unavailable while Investigator mode is active.</p>}
      {error && <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p>}
      <div><Button small minimal icon="refresh" disabled={busy} onClick={() => { setError(null); void refresh().catch((e: unknown) => setError(String(e))); }}>Refresh</Button></div>
    </div>
  </SectionCard>;
}
