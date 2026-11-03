import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/bp";
import SectionCard from "../shared/SectionCard";
import { executeBackendCommand } from "../../hooks/useBackend";
import useEntitlements from "../../hooks/useEntitlements";

const items = [["IncludeLibrary", "Include in library"], ["GiveAccess", "Give access to"], ["Share", "Share"]] as const;
export function contextItemMatches(query: string): boolean {
  const q = query.trim().toLowerCase();
  return !q || ["hide right click context menu items", ...items.map(([, label]) => label.toLowerCase())].some(term => term.includes(q));
}
interface ItemState { item: string; detected: number; owned: boolean; pending: boolean; conflict: boolean; otherAccount: boolean }
interface Status { isAdmin: boolean; items: ItemState[] }
interface Receipt { verified: boolean }

export default function ContextItemVisibilityCard({ searchQuery }: { searchQuery: string }) {
  const { isInvestigator } = useEntitlements();
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  const refresh = useCallback(async () => {
    const result = await executeBackendCommand<Status>("Get-ContextItemVisibility");
    if (!result.success || !result.data || !Array.isArray(result.data.items)) { setStatus(null); throw new Error(result.error || "Windows could not read the right-click entries."); }
    setStatus(result.data);
  }, []);
  useEffect(() => { void refresh().catch((e: unknown) => setError(String(e))); }, [refresh]);
  const change = async (item: string, mode: "hide" | "restore") => {
    if (running.current) return;
    running.current = true; setBusy(true); setError(null);
    let failure: string | null = null;
    try {
      const result = await executeBackendCommand<Receipt>("Set-ContextItemVisibility", { Item: item, Mode: mode });
      if (!result.success || result.data?.verified !== true) throw new Error(result.error || "Windows could not verify the change.");
    } catch (e) { failure = String(e); }
    try { await refresh(); } catch (e) { failure = failure ? `${failure} Refresh also failed: ${String(e)}` : String(e); }
    finally { setError(failure); setBusy(false); running.current = false; }
  };
  const q = searchQuery.trim().toLowerCase();
  const visible = items.filter(([, label]) => !q || "hide right click context menu items".includes(q) || label.toLowerCase().includes(q));
  return <SectionCard title="Right-click menu items" icon="applications">
    <div className="flex flex-col gap-3 text-sm">
      <p>Hide individual entries and restore their previous Windows registrations later.</p>
      <p className="text-xs text-[var(--text-mute)]">This changes the PC defaults and the current account. Other accounts and newer Windows menus may differ. Sign out or restart Explorer if a change is not visible.</p>
      {status && visible.map(([key, label]) => {
        const item = status.items.find(row => row.item === key);
        if (!item) return null;
        const reading = item.otherAccount ? "Restore from the account that hid it" : item.conflict ? "Changed elsewhere — review needed" : item.owned ? "Registration hidden by WinCommander" : item.pending ? "Previous change interrupted" : item.detected ? "Registration detected" : "Registration not detected";
        const disabled = busy || isInvestigator || !status.isAdmin || item.conflict;
        return <div key={key} className="flex flex-wrap items-center justify-between gap-2"><div><div className="font-medium">{label}</div><div className="text-xs text-[var(--text-mute)]">{reading}</div></div><div className="flex gap-2">
          {(!item.owned || item.pending) && <Button small disabled={disabled || (!item.pending && item.detected === 0)} onClick={() => void change(key, "hide")}>{item.pending ? "Finish hiding" : "Hide"}</Button>}
          {(item.owned || item.pending) && <Button small disabled={disabled} onClick={() => void change(key, "restore")}>Restore</Button>}
        </div></div>;
      })}
      {status && !status.isAdmin && <p className="text-xs text-[var(--color-warning)]">Open WinCommander as administrator to change these entries.</p>}
      {isInvestigator && <p>Changes are unavailable while Investigator mode is active.</p>}
      {error && <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p>}
      <div><Button small minimal icon="refresh" disabled={busy} onClick={() => { setError(null); void refresh().catch((e: unknown) => setError(String(e))); }}>Refresh</Button></div>
    </div>
  </SectionCard>;
}
