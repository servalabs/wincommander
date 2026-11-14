import { useCallback, useEffect, useRef, useState } from "react";
import { Button, HTMLSelect, InputGroup } from "@/components/ui/bp";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import { executeBackendCommand } from "../../hooks/useBackend";
import useEntitlements from "../../hooks/useEntitlements";

interface IpRule {
  Id: string;
  RemoteAddress: string;
  Direction: string;
  Action: string;
  Enabled: boolean;
  Active: boolean;
  ScopeMatches: boolean;
}
interface IpRulesResult { rules: IpRule[]; readbackComplete: boolean }
interface MutationResult { verified: boolean }

export default function IpFirewallCard() {
  const { hasPaid, isLoading, isInvestigator } = useEntitlements();
  const confirm = useAppConfirm();
  const [rules, setRules] = useState<IpRule[] | null>(null);
  const [address, setAddress] = useState("");
  const [direction, setDirection] = useState("Outbound");
  const [action, setAction] = useState("Block");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const running = useRef(false);
  const refresh = useCallback(async () => {
    const result = await executeBackendCommand<IpRulesResult>("Get-IPFirewallRules");
    if (!result.success || !result.data?.readbackComplete || !Array.isArray(result.data.rules)) {
      setRules(null);
      throw new Error(result.error?.includes("not yet implemented") ? "IP firewall controls require the current Pro backend. Restart WinCommander after updating it." : result.error || "Windows could not read IP firewall rules.");
    }
    setRules(result.data.rules);
  }, []);
  useEffect(() => {
    if (!hasPaid) { setRules(null); return; }
    void refresh().catch((e: unknown) => setError(String(e)));
  }, [hasPaid, refresh]);

  const mutate = async (command: string, params: Record<string, string>) => {
    if (running.current) return;
    running.current = true; setBusy(true); setError(null);
    let failure: string | null = null;
    try {
      const result = await executeBackendCommand<MutationResult>(command, params);
      if (!result.success || result.data?.verified !== true) throw new Error(result.error || "Windows did not verify the rule change.");
      if (command === "Add-IPFirewallRule") setAddress("");
    } catch (e) { failure = String(e); }
    try { await refresh(); } catch (e) { failure = failure ? `${failure} Refresh also failed: ${String(e)}` : String(e); }
    finally { setError(failure); running.current = false; setBusy(false); }
  };

  const add = async () => {
    const value = address.trim();
    if (!value) { setError("Enter one IP address or subnet."); return; }
    if (!await confirm({
      title: `${action} connections for this IP?`,
      description: `${action} ${direction.toLowerCase()} connections with ${value}. This applies to all apps and can interrupt remote access. Other Windows firewall rules may still take precedence.`,
      confirmLabel: `${action} IP`,
    })) return;
    await mutate("Add-IPFirewallRule", { Address: value, Direction: direction, Action: action });
  };

  return <SectionCard title="IP firewall rules" icon="shield" className="port-guard-card ip-firewall-card">
    <div className="flex flex-col gap-3 text-sm">
      <p>Allow or block connections with a specific IP address or subnet.</p>
      {!hasPaid ? <p>{isLoading ? "Checking access…" : "Available with WinCommander Pro."}</p> : <>
        <p className="text-xs text-[var(--text-mute)]">Rules apply to all apps and all network profiles. A subnet covers a range of addresses. An allow rule does not override a Windows block rule. Firewall settings and other policies can affect the result.</p>
        <div className="port-guard-form">
          <InputGroup aria-label="IP address or subnet" placeholder="192.0.2.10 or 192.0.2.0/24" value={address} onChange={e => setAddress(e.target.value)} maxLength={64} />
          <HTMLSelect aria-label="Connection direction" value={direction} onChange={e => setDirection(e.target.value)}><option value="Outbound">Outbound</option><option value="Inbound">Inbound</option></HTMLSelect>
          <HTMLSelect aria-label="Firewall action" value={action} onChange={e => setAction(e.target.value)}><option value="Block">Block</option><option value="Allow">Allow</option></HTMLSelect>
          <Button intent="primary" disabled={busy || isInvestigator} onClick={() => void add()}>Add rule</Button>
          <Button minimal icon="refresh" disabled={busy} onClick={() => { setError(null); void refresh().catch((e: unknown) => setError(String(e))); }}>Refresh</Button>
        </div>
        {isInvestigator && <p>Rules cannot be changed while Investigator mode is active.</p>}
        <div className="port-guard-table-scroll"><table className="bp5-html-table bp5-html-table-striped port-guard-table" aria-label="WinCommander IP firewall rules">
          <thead><tr><th>Remote IP / subnet</th><th>Direction</th><th>Action</th><th>Windows rule</th><th /></tr></thead>
          <tbody>{rules?.map(rule => <tr key={rule.Id}>
            <td>{rule.RemoteAddress}</td><td>{rule.Direction}</td><td>{rule.Action}</td>
            <td>{!rule.ScopeMatches ? "Changed — needs review" : rule.Enabled && rule.Active ? "Enabled in policy" : "Inactive or not applied"}</td>
            <td><Button small disabled={busy || isInvestigator} aria-label={`Remove ${rule.Action} ${rule.RemoteAddress} ${rule.Direction} rule`} onClick={() => void mutate("Remove-IPFirewallRule", { Id: rule.Id })}>Remove</Button></td>
          </tr>)}</tbody>
        </table></div>
        {rules?.length === 0 && <p>No WinCommander IP rules.</p>}
      </>}
      {error && <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p>}
    </div>
  </SectionCard>;
}
