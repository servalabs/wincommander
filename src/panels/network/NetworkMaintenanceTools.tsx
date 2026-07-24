import { useRef, useState } from "react";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import type { FirewallAudit } from "../../hooks/useBackend";
import { useBackend } from "../../hooks/useBackend";
import { useArpMaintenance } from "./useArpMaintenance";

type FirewallAction = "enable" | "disable" | "remove";

export function NetworkMaintenanceTools() {
  const arp = useArpMaintenance();
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [audit, setAudit] = useState<FirewallAudit>();
  const [ruleIds, setRuleIds] = useState<Set<string>>(new Set());
  const [firewallBusy, setFirewallBusy] = useState(false);
  const [firewallMessage, setFirewallMessage] = useState<string>();
  const [confirmArpClear, setConfirmArpClear] = useState(false);
  const inspectFirewall = async () => {
    setFirewallBusy(true); setFirewallMessage(undefined);
    try { setAudit(await backendRef.current.firewallAuditPreview()); setRuleIds(new Set()); } catch (cause) { setFirewallMessage(String(cause)); } finally { setFirewallBusy(false); }
  };
  const remediateFirewall = async (action: FirewallAction) => {
    if (!ruleIds.size) return;
    setFirewallBusy(true); setFirewallMessage(undefined);
    try {
      const result = await backendRef.current.firewallAuditRemediate([...ruleIds], action);
      setFirewallMessage(`${result.changed} rule(s) ${action === "remove" ? "removed" : `${action}d`}${result.errors.length ? `; ${result.errors.length} refused or failed.` : "."}`);
      setAudit(await backendRef.current.firewallAuditPreview()); setRuleIds(new Set());
    } catch (cause) { setFirewallMessage(String(cause)); } finally { setFirewallBusy(false); }
  };
  return <div className="flex flex-col gap-4">
    <Card><CardHeader><CardTitle>Adapter diagnostics</CardTitle><CardDescription>Inspect Windows neighbor mappings before clearing dynamic entries. Static entries are preserved.</CardDescription></CardHeader><CardContent className="flex flex-wrap items-center gap-2"><Button variant="primary" disabled={arp.busy} onClick={() => void arp.inspect()}><Icon icon="search" />{arp.busy ? "Working…" : "Inspect ARP cache"}</Button>{arp.scan && <><Badge tone="accent">{arp.scan.entries.length} entries</Badge><Badge tone="warning">{arp.scan.dynamicEntries} dynamic</Badge></>}</CardContent></Card>
    {arp.error && <Notice tone="danger" text={arp.error} />}
    {arp.scan && <Card><CardHeader><CardTitle>Neighbor mappings</CardTitle><CardDescription>Adapter, IP address, physical address, and cache type from Windows.</CardDescription></CardHeader><CardContent className="flex flex-col gap-2">{arp.scan.entries.map((entry, index) => <div key={`${entry.interface}-${entry.address}-${index}`} className="grid gap-1 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 md:grid-cols-4"><span className="font-mono text-xs text-[var(--text)]">{entry.address}</span><span className="font-mono text-xs text-[var(--text-dim)]">{entry.physicalAddress}</span><span className="font-mono text-xs text-[var(--text-mute)]">{entry.interface}</span><Badge tone={entry.entryType === "dynamic" ? "warning" : "success"}>{entry.entryType}</Badge></div>)}{!arp.scan.entries.length && <Empty text="ARP cache is empty." />}</CardContent></Card>}
    {!!arp.scan?.dynamicEntries && <div className="flex justify-end"><Button variant="danger" disabled={arp.busy} onClick={() => setConfirmArpClear(true)}>Clear dynamic entries</Button></div>}
    {arp.result && <Notice tone="success" text={`Cleared ${arp.result.cleared} dynamic entries; ${arp.result.remaining} remain.`} />}
    <Card><CardHeader><CardTitle>Firewall audit</CardTitle><CardDescription>Shows only third-party disabled or allow rules. Windows and WinCommander rules, duplicate names, and changed preview entries are excluded from remediation.</CardDescription></CardHeader><CardContent className="flex flex-wrap items-center gap-2"><Button variant="primary" disabled={firewallBusy} onClick={() => void inspectFirewall()}><Icon icon="shield" />{firewallBusy ? "Auditing…" : "Audit rules"}</Button>{firewallBusy && <Button variant="outline" onClick={() => void backendRef.current.firewallAuditCancel()}><Icon icon="stop" /> Cancel</Button>}{audit && <Badge tone="accent">{audit.rules.length} candidates</Badge>}</CardContent></Card>
    {audit?.error && <Notice tone="danger" text={audit.error} />}
    {audit && !audit.error && <Card><CardContent className="flex flex-col gap-2 py-4">{audit.rules.map((rule) => <SelectableRow key={rule.id} checked={ruleIds.has(rule.id)} onClick={() => setRuleIds(toggle(ruleIds, rule.id))} title={rule.name} detail={`${rule.enabled ? "enabled" : "disabled"} · ${rule.action} · ${rule.program || "all programs"}`} />)}{!audit.rules.length && <Empty text="No safe third-party candidates found." />}</CardContent></Card>}
    {!!ruleIds.size && <div className="flex flex-wrap justify-end gap-2"><Button variant="outline" disabled={firewallBusy} onClick={() => void remediateFirewall("enable")}>Enable selected</Button><Button variant="outline" disabled={firewallBusy} onClick={() => void remediateFirewall("disable")}>Disable selected</Button><Button variant="danger" disabled={firewallBusy} onClick={() => void remediateFirewall("remove")}>Remove selected</Button></div>}
    {firewallMessage && <Notice tone={audit?.error ? "danger" : "success"} text={firewallMessage} />}
    <AlertDialog open={confirmArpClear} onOpenChange={setConfirmArpClear}><AlertDialogContent><AlertDialogHeader><AlertDialogTitle>Clear dynamic ARP entries?</AlertDialogTitle><AlertDialogDescription>Windows will relearn active neighbor mappings. Existing network connections can pause briefly.</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => { setConfirmArpClear(false); void arp.clear(); }}>Clear cache</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>
  </div>;
}
function SelectableRow({ checked, onClick, title, detail }: { checked: boolean; onClick: () => void; title: string; detail: string }) { return <button type="button" onClick={onClick} className="flex w-full items-start gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]"><span className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>{checked && <Icon icon="check" />}</span><span className="min-w-0"><span className="block text-sm text-[var(--text)]">{title}</span><span className="block break-all font-mono text-[11px] text-[var(--text-mute)]">{detail}</span></span></button>; }
function Notice({ tone, text }: { tone: "success" | "danger"; text: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{text}</p></CardContent></Card>; }
function Empty({ text }: { text: string }) { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">{text}</p>; }
function toggle(current: Set<string>, id: string) { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; }
