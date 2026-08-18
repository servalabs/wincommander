import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import useBackend, { type EncryptionPartition } from "@/hooks/useBackend";
import { showError, showSuccess } from "@/utils/toast";
import FleetField from "./FleetField";
import VaultGroupsEditor from "./VaultGroupsEditor";
import VaultMatrixPreview from "./VaultMatrixPreview";
import VaultVolumesEditor from "./VaultVolumesEditor";
import type { VaultFleetPolicy } from "./vaultFleetTypes";
import {
  buildVaultMatrix, loadVaultPolicy, saveVaultPolicy, validateVaultPolicy,
} from "./vaultFleetPolicy";

function partitionLabel(partition: EncryptionPartition) {
  return `${partition.model || "Disk"} · Disk ${partition.diskNumber}, partition ${partition.partitionNumber} · ${partition.size}`;
}

export default function VaultAccessTab() {
  const [policy, setPolicy] = useState<VaultFleetPolicy>(loadVaultPolicy);
  const [partitions, setPartitions] = useState<EncryptionPartition[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [showMatrix, setShowMatrix] = useState(false);
  const { getEncryptionPartitions, getUserProfiles } = useBackend();
  const errors = useMemo(() => validateVaultPolicy(policy), [policy]);

  useEffect(() => {
    if (policy.ownerPrincipal) return;
    void getUserProfiles().then(result => {
      if (result.success && result.data?.currentUser) {
        setPolicy(current => current.ownerPrincipal ? current : { ...current, ownerPrincipal: result.data!.currentUser });
      }
    });
  }, [getUserProfiles, policy.ownerPrincipal]);

  const update = (patch: Partial<VaultFleetPolicy>) => setPolicy(current => ({ ...current, ...patch }));
  const discover = async () => {
    setDiscovering(true);
    const result = await getEncryptionPartitions();
    setDiscovering(false);
    if (!result.success) return void showError(result.error || "Disk discovery failed.");
    setPartitions(result.data?.partitions ?? []);
    void showSuccess(`Found ${result.data?.partitions.length ?? 0} eligible partitions.`);
  };

  const selectPartition = (partition: EncryptionPartition) => update({
    diskNumber: partition.diskNumber,
    diskUniqueId: partition.diskUniqueId,
    confirmationText: partition.confirmationToken,
  });

  const save = () => {
    if (errors.length) return void showError(errors[0]);
    saveVaultPolicy(policy);
    void showSuccess("Vault deployment policy saved on this administrator workstation.");
  };

  const copyManifest = async () => {
    if (errors.length) return void showError(errors[0]);
    await navigator.clipboard.writeText(JSON.stringify(policy, null, 2));
    void showSuccess("Deployment manifest copied. It contains no plaintext passwords.");
  };

  return (
    <div className="fleet-admin-stack">
      <div className="fleet-callout fleet-callout-warning">
        <strong>Destructive boundary:</strong> only the selected test disk/partitions are eligible. C: and D: are never inferred from a drive letter and must remain outside the pinned disk identity.
      </div>

      <Card>
        <CardHeader><CardTitle>Owner and disk boundary</CardTitle><CardDescription>Pin the existing administrator and the exact test disk before generating a deployment manifest.</CardDescription></CardHeader>
        <CardContent className="fleet-form-grid">
          <FleetField label="Owner principal" hint="This account remains an administrator and receives every volume credential.">
            <Input value={policy.ownerPrincipal} onChange={event => update({ ownerPrincipal: event.target.value })} />
          </FleetField>
          <FleetField label="Mount scope"><Input value="Per-user session isolation" disabled /></FleetField>
          <FleetField label="Protected engine directory">
            <Input value={policy.installDirectory} onChange={event => update({ installDirectory: event.target.value })} />
          </FleetField>
          <label className="fleet-switch-field"><Switch checked={policy.preloadDriverAtStartup} onCheckedChange={checked => update({ preloadDriverAtStartup: checked })} /> <span><strong>Protected SYSTEM driver preload</strong><small>Standard users never receive service-control or driver-load privilege.</small></span></label>
          <label className="fleet-switch-field"><Switch checked={policy.allowUnallocatedSpace} onCheckedChange={checked => update({ allowUnallocatedSpace: checked })} /> <span><strong>Allow selected-disk unallocated space</strong><small>Raw-volume entries may allocate only on the pinned disk, never C: or D:.</small></span></label>
          <FleetField label="Unallocated reserve (MiB)" hint="Leave this amount unused after planned raw-volume allocations.">
            <Input type="number" min={0} value={policy.unallocatedReserveMb} onChange={event => update({ unallocatedReserveMb: Number(event.target.value) })} disabled={!policy.allowUnallocatedSpace} />
          </FleetField>
          <div className="fleet-discovery-row">
            <Button onClick={discover} disabled={discovering}>{discovering ? "Discovering…" : "Discover test partitions"}</Button>
            {policy.diskNumber != null && <span>Selected Disk {policy.diskNumber} · <code>{policy.diskUniqueId}</code></span>}
          </div>
          {partitions.length > 0 && <div className="fleet-partition-list">
            {partitions.map(partition => <button key={partition.devicePath} disabled={!partition.safeForCreation} onClick={() => selectPartition(partition)} className={policy.diskUniqueId === partition.diskUniqueId ? "is-selected" : ""}>
              <span>{partitionLabel(partition)}</span><small>{partition.devicePath} · {partition.safeForCreation ? "eligible" : "mount only"}</small>
            </button>)}
          </div>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader><CardTitle>Groups and standard users</CardTitle><CardDescription>Accounting, Sales, and Partner membership is explicit and mutually exclusive.</CardDescription></CardHeader>
        <CardContent><VaultGroupsEditor groups={policy.groups} onChange={groups => update({ groups })} /></CardContent>
      </Card>

      <Card>
        <CardHeader><CardTitle>Volumes and access policy</CardTitle><CardDescription>Configure file containers, raw partitions, dual volumes, credentials, drive letters, and group visibility.</CardDescription></CardHeader>
        <CardContent><VaultVolumesEditor volumes={policy.volumes} onChange={volumes => update({ volumes })} /></CardContent>
      </Card>

      <Card>
        <CardHeader><CardTitle>Validation matrix</CardTitle><CardDescription>Preview the expected distinction between backing visibility, mount/decrypt authorization, content ACLs, and another session’s mounted drive.</CardDescription></CardHeader>
        <CardContent>
          {errors.length > 0 && <ul className="fleet-validation-errors">{errors.map(error => <li key={error}>{error}</li>)}</ul>}
          <div className="fleet-action-row">
            <Button onClick={() => setShowMatrix(value => !value)}>{showMatrix ? "Hide matrix" : "Preview matrix"}</Button>
            <Button onClick={copyManifest}>Copy deployment manifest</Button>
            <Button variant="primary" onClick={save}>Save policy</Button>
          </div>
          {showMatrix && <VaultMatrixPreview rows={buildVaultMatrix(policy)} />}
          <p className="fleet-field-hint">This preview validates policy intent. Live per-user probes and reboot validation remain a separate deployment step and must not be inferred from this table.</p>
        </CardContent>
      </Card>
    </div>
  );
}
