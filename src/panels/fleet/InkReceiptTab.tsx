import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { showError, showSuccess } from "@/utils/toast";
import { getInkPolicy, getOrgSettings, saveInkPolicy, updateOrgSettings } from "./fleetAdminApi";
import { useFleetSession } from "./FleetAdminSession";
import type { InkDestination, InkReceiptPolicy } from "./fleetAdminTypes";
import SecurityField from "./SecurityField";

const DEFAULT_POLICY: InkReceiptPolicy = {
  managedDestinations: [{ name: "Controlled PDF", printerClass: "pdf" }],
  ticketRequired: true,
  offlineBehavior: "ex_post_duplicate_detection",
  watermarkTemplate: "CONFIDENTIAL MOVEMENT RECEIPT\n{device} | {timestamp} | {ticket}",
  failureStance: { pdf: "fail_closed", securePhysical: "fail_soft" },
};

export default function InkReceiptTab() {
  const { session } = useFleetSession();
  const [enabled, setEnabled] = useState(false);
  const [ticketTtl, setTicketTtl] = useState(900);
  const [offlineMax, setOfflineMax] = useState(25);
  const [policy, setPolicy] = useState<InkReceiptPolicy>(DEFAULT_POLICY);
  const [version, setVersion] = useState<number | null>(null);
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    if (!session) return;
    Promise.all([getOrgSettings(session), getInkPolicy(session)])
      .then(([settings, current]) => {
        setEnabled(settings.ink_receipt_enabled);
        setTicketTtl(settings.ink_receipt_ticket_ttl_secs);
        setOfflineMax(settings.ink_receipt_offline_max);
        if (current) {
          const { policy_version, ...saved } = current;
          setPolicy(saved); setVersion(policy_version);
        }
      })
      .catch(cause => void showError(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setBusy(false));
  }, [session]);

  if (!session) return null;
  const mutate = session.role === "admin" || session.role === "super_admin";
  const patchDestination = (index: number, patch: Partial<InkDestination>) => setPolicy(current => ({
    ...current,
    managedDestinations: current.managedDestinations.map((destination, i) => i === index ? { ...destination, ...patch } : destination),
  }));
  const save = async () => {
    setBusy(true);
    try {
      await updateOrgSettings(session, {
        ink_receipt_enabled: enabled,
        ink_receipt_ticket_ttl_secs: ticketTtl,
        ink_receipt_offline_max: offlineMax,
      });
      const stored = await saveInkPolicy(session, policy);
      setVersion(stored.policy_version);
      void showSuccess(`Ink Receipt policy published as epoch ${stored.policy_version}.`);
    } catch (cause) { void showError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };

  return <div className="fleet-admin-stack">
    <Card><CardHeader><CardTitle>Ink Receipt service</CardTitle><CardDescription>Configure signed print tickets, offline limits, managed destination classes, watermarking, and failure stance.</CardDescription></CardHeader>
      <CardContent>
        <label className="fleet-switch-field"><Switch checked={enabled} onCheckedChange={setEnabled} disabled={!mutate} /><span><strong>Enable Ink Receipt</strong><small>Receipts carry no document, queue, printer, path, or username content.</small></span></label>
        <div className="fleet-form-grid">
          <SecurityField label="Ticket lifetime (seconds)"><Input type="number" min={60} value={ticketTtl} onChange={event => setTicketTtl(Number(event.target.value))} /></SecurityField>
          <SecurityField label="Offline outstanding-ticket cap"><Input type="number" min={0} value={offlineMax} onChange={event => setOfflineMax(Number(event.target.value))} /></SecurityField>
          <label className="fleet-switch-field"><Switch checked={policy.ticketRequired} onCheckedChange={ticketRequired => setPolicy(current => ({ ...current, ticketRequired }))} /><span><strong>Require a signed ticket</strong><small>Disable only for watermark-and-receipt audit mode.</small></span></label>
        </div>
      </CardContent>
    </Card>
    <Card><CardHeader><CardTitle>Managed destinations</CardTitle><CardDescription>Product labels only—never raw Windows printer or queue names.</CardDescription></CardHeader>
      <CardContent><div className="fleet-volume-stack">{policy.managedDestinations.map((destination, index) => <section className="fleet-subcard" key={`${index}-${destination.name}`}>
        <div className="fleet-form-grid">
          <SecurityField label="Display label"><Input value={destination.name} onChange={event => patchDestination(index, { name: event.target.value })} /></SecurityField>
          <SecurityField label="Printer class"><Select value={destination.printerClass} onValueChange={printerClass => patchDestination(index, { printerClass: printerClass as InkDestination["printerClass"] })}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="pdf">Controlled PDF</SelectItem><SelectItem value="secure_physical">Secure physical</SelectItem></SelectContent></Select></SecurityField>
        </div><Button size="sm" variant="danger" onClick={() => setPolicy(current => ({ ...current, managedDestinations: current.managedDestinations.filter((_, i) => i !== index) }))}>Remove</Button>
      </section>)}</div>
      <Button onClick={() => setPolicy(current => ({ ...current, managedDestinations: [...current.managedDestinations, { name: "New destination", printerClass: "pdf" }] }))}>Add destination</Button></CardContent>
    </Card>
    <Card><CardHeader><CardTitle>Watermark and failure behavior</CardTitle><CardDescription>Controlled PDF can fail closed; physical printing is observational and defaults to fail soft.</CardDescription></CardHeader>
      <CardContent className="fleet-form-grid">
        <SecurityField label="Watermark template"><textarea className="fleet-textarea" value={policy.watermarkTemplate} onChange={event => setPolicy(current => ({ ...current, watermarkTemplate: event.target.value }))} /></SecurityField>
        <SecurityField label="PDF failure stance"><Select value={policy.failureStance.pdf} onValueChange={pdf => setPolicy(current => ({ ...current, failureStance: { ...current.failureStance, pdf: pdf as "fail_closed" | "fail_soft" } }))}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="fail_closed">Fail closed</SelectItem><SelectItem value="fail_soft">Fail soft</SelectItem></SelectContent></Select></SecurityField>
        <SecurityField label="Physical failure stance"><Select value={policy.failureStance.securePhysical} onValueChange={securePhysical => setPolicy(current => ({ ...current, failureStance: { ...current.failureStance, securePhysical: securePhysical as "fail_closed" | "fail_soft" } }))}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="fail_soft">Fail soft</SelectItem><SelectItem value="fail_closed">Fail closed policy (not a physical guarantee)</SelectItem></SelectContent></Select></SecurityField>
        <div className="fleet-action-row"><span>{version == null ? "No policy published" : `Current epoch ${version}`}</span><Button variant="primary" onClick={save} disabled={busy || !mutate}>Save & publish</Button></div>
      </CardContent>
    </Card>
  </div>;
}
