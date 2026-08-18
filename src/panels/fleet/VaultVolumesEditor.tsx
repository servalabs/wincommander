import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import type { VaultGroup, VaultKind, VaultVolumePolicy } from "./vaultFleetTypes";
import FleetField from "./FleetField";
import { updateVolume } from "./vaultFleetPolicy";

const GROUPS: VaultGroup["id"][] = ["accounting", "sales", "partner"];

export default function VaultVolumesEditor({ volumes, onChange }: {
  volumes: VaultVolumePolicy[];
  onChange: (volumes: VaultVolumePolicy[]) => void;
}) {
  const patch = (id: string, value: Partial<VaultVolumePolicy>) =>
    onChange(updateVolume(volumes, id, value));

  const addVolume = () => {
    const suffix = Date.now().toString(36);
    onChange([...volumes, {
      id: `volume-${suffix}`,
      label: "New volume",
      kind: "container",
      backing: `volume-${suffix}.hc`,
      sizeMb: 1024,
      driveLetter: "V",
      credentialRef: `Credential-${suffix}`,
      allowedGroups: [],
      ownerOnly: true,
    }]);
  };

  return (
    <div className="fleet-volume-stack">
      {volumes.map(volume => (
        <section className="fleet-subcard" key={volume.id}>
          <div className="fleet-subcard-heading">
            <Input aria-label="Volume label" value={volume.label} onChange={event => patch(volume.id, { label: event.target.value })} />
            <Button size="sm" variant="danger" onClick={() => onChange(volumes.filter(item => item.id !== volume.id))}>Remove</Button>
          </div>
          <div className="fleet-form-grid fleet-form-grid-4">
            <FleetField label="Type">
              <Select value={volume.kind} onValueChange={kind => patch(volume.id, { kind: kind as VaultKind })}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="container">Container</SelectItem>
                  <SelectItem value="partition">Raw partition</SelectItem>
                  <SelectItem value="dual">Decoy + hidden</SelectItem>
                </SelectContent>
              </Select>
            </FleetField>
            <FleetField label="Backing path / device">
              <Input value={volume.backing} onChange={event => patch(volume.id, { backing: event.target.value })} />
            </FleetField>
            <FleetField label="Size (MiB)">
              <Input type="number" min={64} value={volume.sizeMb} onChange={event => patch(volume.id, { sizeMb: Number(event.target.value) })} />
            </FleetField>
            <FleetField label="Drive letter">
              <Input maxLength={1} value={volume.driveLetter} onChange={event => patch(volume.id, { driveLetter: event.target.value.toUpperCase() })} />
            </FleetField>
            <FleetField label="Credential reference" hint="Passwords are generated/stored by the protected deployment workflow.">
              <Input value={volume.credentialRef} onChange={event => patch(volume.id, { credentialRef: event.target.value })} />
            </FleetField>
            {volume.kind === "dual" && <>
              <FleetField label="Hidden size (MiB)">
                <Input type="number" min={32} value={volume.hiddenSizeMb ?? 128} onChange={event => patch(volume.id, { hiddenSizeMb: Number(event.target.value) })} />
              </FleetField>
              <FleetField label="Hidden credential">
                <Input value={volume.hiddenCredentialRef ?? ""} onChange={event => patch(volume.id, { hiddenCredentialRef: event.target.value })} />
              </FleetField>
            </>}
          </div>
          <div className="fleet-policy-row">
            <label><Switch checked={volume.ownerOnly} onCheckedChange={checked => patch(volume.id, { ownerOnly: checked, allowedGroups: checked ? [] : volume.allowedGroups })} /> Owner only</label>
            {GROUPS.map(group => (
              <label key={group} className={volume.ownerOnly ? "is-disabled" : ""}>
                <input
                  type="checkbox"
                  disabled={volume.ownerOnly}
                  checked={volume.allowedGroups.includes(group)}
                  onChange={event => patch(volume.id, {
                    allowedGroups: event.target.checked
                      ? [...volume.allowedGroups, group]
                      : volume.allowedGroups.filter(item => item !== group),
                  })}
                /> {group}
              </label>
            ))}
            {volume.kind === "dual" && !volume.ownerOnly && (
              <label><Switch checked={volume.groupReadOnly ?? false} onCheckedChange={checked => patch(volume.id, { groupReadOnly: checked })} /> Group outer read-only</label>
            )}
          </div>
        </section>
      ))}
      <Button onClick={addVolume}>Add volume</Button>
    </div>
  );
}
