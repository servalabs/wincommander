import { useEffect, useMemo, useState } from "react";
import ToggleTile from "@/components/shared/ToggleTile";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import useBackend, { type EncryptionPartition } from "@/hooks/useBackend";
import { showError, showSuccess } from "@/utils/toast";
import FleetField from "./FleetField";
import VaultMatrixPreview from "./VaultMatrixPreview";
import VaultVolumesEditor from "./VaultVolumesEditor";
import type { FleetAccessDirectory } from "./accessControlTypes";
import type { VaultFleetPolicy } from "./vaultFleetTypes";
import {
  buildVaultMatrix, validateVaultPolicy,
} from "./vaultFleetPolicy";

function partitionLabel(partition: EncryptionPartition) {
  return `${partition.model || "Disk"} · Disk ${partition.diskNumber}, partition ${partition.partitionNumber} · ${partition.size}`;
}

interface VaultAccessTabProps {
  directory: FleetAccessDirectory;
  policy: VaultFleetPolicy;
  onChange: (policy: VaultFleetPolicy) => void;
  onSave: () => void;
}

export default function VaultAccessTab({ directory, policy, onChange, onSave }: VaultAccessTabProps) {
  const [partitions, setPartitions] = useState<EncryptionPartition[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [showMatrix, setShowMatrix] = useState(false);
  const { getEncryptionPartitions, getUserProfiles } = useBackend();
  const errors = useMemo(() => validateVaultPolicy(policy, directory.groups), [directory.groups, policy]);

  useEffect(() => {
    if (policy.ownerPrincipal) return;
    void getUserProfiles().then(result => {
      if (result.success && result.data?.currentUser) {
        if (!policy.ownerPrincipal) onChange({ ...policy, ownerPrincipal: result.data!.currentUser });
      }
    });
  }, [getUserProfiles, onChange, policy]);

  const update = (patch: Partial<VaultFleetPolicy>) => onChange({ ...policy, ...patch });
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
    onSave();
    void showSuccess("Vault deployment policy saved on this administrator workstation.");
  };

  const copyManifest = async () => {
    if (errors.length) return void showError(errors[0]);
    await navigator.clipboard.writeText(JSON.stringify(policy, null, 2));
    void showSuccess("Deployment manifest copied. It contains no plaintext passwords.");
  };

  return (
    <div className="fleet-admin-stack">
      <Card>
        <CardHeader><CardTitle>Owner and disk boundary</CardTitle><CardDescription>Pin the existing administrator and the exact test disk before generating a deployment manifest.</CardDescription></CardHeader>
        <CardContent className="fleet-owner-stack">
          <div className="fleet-owner-inputs">
            <FleetField compact label="Owner principal" hint="This account remains an administrator and receives every volume credential.">
              <Input value={policy.ownerPrincipal} onChange={event => update({ ownerPrincipal: event.target.value })} />
            </FleetField>
            <FleetField compact label="Mount scope"><Input value="Per-user session isolation" disabled /></FleetField>
            <FleetField compact label="Protected engine directory">
              <Input value={policy.installDirectory} onChange={event => update({ installDirectory: event.target.value })} />
            </FleetField>
            <FleetField compact label="Unallocated reserve (MiB)" hint="Leave this amount unused after planned raw-volume allocations.">
              <Input type="number" min={0} value={policy.unallocatedReserveMb} onChange={event => update({ unallocatedReserveMb: Number(event.target.value) })} disabled={!policy.allowUnallocatedSpace} />
            </FleetField>
          </div>
          <div className="fleet-owner-tools">
            <div className="fleet-policy-toggle-grid">
              <ToggleTile
                label="Protected SYSTEM driver preload"
                description="Standard users never receive service-control or driver-load privilege."
                checked={policy.preloadDriverAtStartup}
                onChange={checked => update({ preloadDriverAtStartup: checked })}
                domain="security"
                icon="shield"
              />
              <ToggleTile
                label="Allow selected-disk unallocated space"
                description="Raw-volume entries may allocate only on the pinned disk, never C: or D:."
                checked={policy.allowUnallocatedSpace}
                onChange={checked => update({ allowUnallocatedSpace: checked })}
                domain="tweaks"
                icon="hard-drive"
              />
            </div>
            <div className="fleet-discovery-row">
              <Button onClick={discover} disabled={discovering}>{discovering ? "Discovering…" : "Discover test partitions"}</Button>
              {policy.diskNumber != null && <span>Selected Disk {policy.diskNumber} · <code>{policy.diskUniqueId}</code></span>}
            </div>
          </div>
          {partitions.length > 0 && <div className="fleet-partition-list">
            {partitions.map(partition => <button key={partition.devicePath} disabled={!partition.safeForCreation} onClick={() => selectPartition(partition)} className={policy.diskUniqueId === partition.diskUniqueId ? "is-selected" : ""}>
              <span>{partitionLabel(partition)}</span><small>{partition.devicePath} · {partition.safeForCreation ? "eligible" : "mount only"}</small>
            </button>)}
          </div>}
        </CardContent>
      </Card>

      <VaultVolumesEditor volumes={policy.volumes} groups={directory.groups} onChange={volumes => update({ volumes })} />

      <Card>
        <CardHeader><CardTitle>Validation matrix</CardTitle><CardDescription>Preview the expected distinction between backing visibility, mount/decrypt authorization, content ACLs, and another session’s mounted drive.</CardDescription></CardHeader>
        <CardContent>
          {errors.length > 0 && <ul className="fleet-validation-errors">{errors.map(error => <li key={error}>{error}</li>)}</ul>}
          <div className="fleet-action-row">
            <Button onClick={() => setShowMatrix(value => !value)}>{showMatrix ? "Hide matrix" : "Preview matrix"}</Button>
            <Button onClick={copyManifest}>Copy deployment manifest</Button>
            <Button variant="primary" onClick={save}>Save policy</Button>
          </div>
          {showMatrix && <VaultMatrixPreview rows={buildVaultMatrix(policy, directory)} />}
          <p className="fleet-field-hint">This preview validates policy intent. Live per-user probes and reboot validation remain a separate deployment step and must not be inferred from this table.</p>
        </CardContent>
      </Card>
    </div>
  );
}
