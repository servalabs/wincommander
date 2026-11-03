import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/bp";
import SectionCard from "../../components/shared/SectionCard";
import { executeBackendCommand } from "../../hooks/useBackend";
import useEntitlements from "../../hooks/useEntitlements";

const permissions = [
  ["humanPresence", "Presence sensing"], ["passkeys", "Passkeys"],
  ["bluetooth", "Bluetooth devices"], ["humanInterfaceDevice", "Human-interface devices"],
  ["passkeysEnumeration", "List of passkeys"], ["sensors.custom", "Custom sensors"],
  ["serialCommunication", "Serial ports"], ["usb", "USB devices"],
  ["wifiData", "Wi-Fi information"], ["wiFiDirect", "Wi-Fi Direct"],
] as const;

export function additionalPermissionMatches(query: string): boolean {
  const q = query.trim().toLowerCase();
  return !q || ["additional app permission defaults", ...permissions.map(([, label]) => label.toLowerCase())].some(term => term.includes(q));
}

interface PermissionState {
  capability: string; scope: "User" | "Machine"; available: boolean;
  value: "Allow" | "Deny" | "Prompt" | "Unset" | "Unknown";
  owned: boolean; pending: boolean; conflict: boolean; managed: boolean;
}
interface PermissionResult { isAdmin: boolean; permissions: PermissionState[] }
interface ChangeResult { verified: boolean }

export default function AdditionalAppPermissions({ searchQuery }: { searchQuery: string }) {
  const { isInvestigator } = useEntitlements();
  const [data, setData] = useState<PermissionResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  const refresh = useCallback(async () => {
    const result = await executeBackendCommand<PermissionResult>("Get-AdditionalAppPermissions");
    if (!result.success || !result.data || !Array.isArray(result.data.permissions)) {
      setData(null); throw new Error(result.error || "Windows could not read these permissions.");
    }
    setData(result.data);
  }, []);
  useEffect(() => { void refresh().catch((e: unknown) => setError(String(e))); }, [refresh]);

  const change = async (row: PermissionState, mode: "deny" | "restore") => {
    if (running.current) return;
    running.current = true; setBusy(true); setError(null);
    let failure: string | null = null;
    try {
      const result = await executeBackendCommand<ChangeResult>("Set-AdditionalAppPermission", {
        Capability: row.capability, Scope: row.scope, Mode: mode,
      });
      if (!result.success || result.data?.verified !== true) throw new Error(result.error || "Windows could not verify the change.");
    } catch (e) { failure = String(e); }
    try { await refresh(); } catch (e) { failure = failure ? `${failure} Refresh also failed: ${String(e)}` : String(e); }
    finally { setError(failure); setBusy(false); running.current = false; }
  };

  const q = searchQuery.trim().toLowerCase();
  const visible = permissions.filter(([, label]) => !q || "additional app permission defaults".includes(q) || label.toLowerCase().includes(q));
  const describe = (row: PermissionState) => {
    if (row.conflict) return "Changed elsewhere — review needed";
    if (row.managed) return "Controlled by policy";
    if (!row.available) return "Not detected on this PC";
    if (row.pending) return "Previous change interrupted";
    if (row.owned) return "Default set to block";
    return row.value === "Unset" ? "Not set" : row.value === "Deny" ? "Default already blocks" : row.value === "Allow" ? "Default allows" : row.value === "Prompt" ? "Ask first" : "Unknown";
  };
  const cell = (key: string, scope: PermissionState["scope"]) => {
    const row = data?.permissions.find(p => p.capability === key && p.scope === scope);
    if (!row) return <span>Unavailable</span>;
    const disabled = busy || isInvestigator || !row.available || row.conflict || row.managed || row.value === "Unknown" || (scope === "Machine" && !data?.isAdmin) || (!row.owned && !row.pending && row.value === "Deny");
    const cancelDisabled = busy || isInvestigator || row.conflict || (scope === "Machine" && !data?.isAdmin);
    return <div className="flex flex-wrap items-center gap-2"><span>{describe(row)}</span>
      {!row.owned && <Button small disabled={disabled} onClick={() => void change(row, "deny")}>{row.pending ? "Retry block" : "Block default"}</Button>}
      {(row.owned || row.pending) && <Button small disabled={row.pending ? cancelDisabled : disabled} onClick={() => void change(row, "restore")}>{row.pending ? "Cancel change" : "Restore"}</Button>}
    </div>;
  };
  return <SectionCard title="More app-permission defaults" icon="applications">
    <div className="flex flex-col gap-3 text-sm">
      <p>Set a default for your account or for this PC. Restore returns the previous default.</p>
      <p className="text-xs text-[var(--text-mute)]">These settings do not change individual app exceptions or stop every desktop app. Windows or an administrator can apply other rules. Changes may need an app restart.</p>
      {data && <div className="overflow-x-auto"><table className="bp5-html-table bp5-html-table-striped w-full" aria-label="Additional app permission defaults">
        <thead><tr><th>Permission</th><th>This account</th><th>This PC</th></tr></thead>
        <tbody>{visible.map(([key, label]) => <tr key={key}><td>{label}</td><td>{cell(key, "User")}</td><td>{cell(key, "Machine")}</td></tr>)}</tbody>
      </table></div>}
      {data && !data.isAdmin && <p className="text-xs text-[var(--color-warning)]">Open WinCommander as administrator to change settings for this PC.</p>}
      {isInvestigator && <p>Changes are unavailable while Investigator mode is active.</p>}
      {error && <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p>}
      <div><Button small minimal icon="refresh" disabled={busy} onClick={() => { setError(null); void refresh().catch((e: unknown) => setError(String(e))); }}>Refresh</Button></div>
    </div>
  </SectionCard>;
}
