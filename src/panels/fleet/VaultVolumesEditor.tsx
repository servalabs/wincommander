import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { FleetAccessGroup } from "./accessControlTypes";
import FleetField from "./FleetField";
import FleetInfoPopover from "./FleetInfoPopover";
import { updateVolume } from "./vaultFleetPolicy";
import type { VaultKind, VaultPermission, VaultVolumePolicy } from "./vaultFleetTypes";

function nextDriveLetter(volumes: VaultVolumePolicy[]) {
  const used = new Set(volumes.map(volume => volume.driveLetter));
  return [..."VUWXYZTSRQPNMLKJIHGFEBA"].find(letter => !used.has(letter)) ?? "V";
}

export default function VaultVolumesEditor({ volumes, groups, onChange }: {
  volumes: VaultVolumePolicy[];
  groups: FleetAccessGroup[];
  onChange: (volumes: VaultVolumePolicy[]) => void;
}) {
  const [selectedId, setSelectedId] = useState(volumes[0]?.id ?? "");
  const selected = volumes.find(volume => volume.id === selectedId);

  useEffect(() => {
    if (selected || volumes.length === 0) return;
    setSelectedId(volumes[0].id);
  }, [selected, volumes]);

  const patch = (value: Partial<VaultVolumePolicy>) => {
    if (!selected) return;
    onChange(updateVolume(volumes, selected.id, value));
  };

  const addVolume = () => {
    const suffix = typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : Date.now().toString(36);
    const volume: VaultVolumePolicy = {
      id: `volume-${suffix}`,
      label: "New volume",
      kind: "container",
      backing: `volume-${suffix}.hc`,
      sizeMb: 1024,
      driveLetter: nextDriveLetter(volumes),
      credentialRef: `Credential-${suffix}`,
      groupPermissions: {},
      ownerOnly: true,
    };
    onChange([...volumes, volume]);
    setSelectedId(volume.id);
  };

  const removeVolume = () => {
    if (!selected) return;
    const remaining = volumes.filter(volume => volume.id !== selected.id);
    onChange(remaining);
    setSelectedId(remaining[0]?.id ?? "");
  };

  const setGroupPermission = (volume: VaultVolumePolicy, groupId: string, permission: "none" | VaultPermission) => {
    const groupPermissions = { ...volume.groupPermissions };
    if (permission === "none") delete groupPermissions[groupId];
    else groupPermissions[groupId] = permission;
    onChange(updateVolume(volumes, volume.id, { groupPermissions, ownerOnly: false }));
  };

  return (
    <Card className="fleet-vault-workspace">
      <CardHeader className="fleet-vault-workspace-header">
        <div>
          <CardTitle>Vault permissions</CardTitle>
          <CardDescription>{volumes.length} configured · {groups.length} access group{groups.length === 1 ? "" : "s"}</CardDescription>
        </div>
        <Button size="sm" onClick={addVolume}><Icon icon="plus" />Add volume</Button>
      </CardHeader>

      <CardContent className="fleet-vault-workspace-content">
        {volumes.length === 0 ? <div className="fleet-vault-empty-inline">
          <Icon icon="database" size={20} />
          <div><strong>No Vault volumes yet</strong><small>Add a volume to set its group permissions.</small></div>
        </div> : <>
          <div className="fleet-vault-matrix-wrap">
            <table className="fleet-vault-matrix">
              <thead>
                <tr><th scope="col">Vault / drive</th><th scope="col">Owner / administrator</th>{groups.map(group => <th scope="col" key={group.id}>{group.name}</th>)}</tr>
              </thead>
              <tbody>
                {volumes.map(volume => <tr className={volume.id === selectedId ? "is-selected" : ""} key={volume.id}>
                  <td>
                    <button type="button" className="fleet-vault-matrix-volume" onClick={() => setSelectedId(volume.id)}>
                      <strong>{volume.label} · {volume.driveLetter}:</strong><small>{volume.backing}</small>
                    </button>
                  </td>
                  <td><span className="fleet-vault-owner-lock">Full · locked</span></td>
                  {groups.map(group => <td key={group.id}>
                    <Select
                      disabled={volume.ownerOnly}
                      value={volume.groupPermissions[group.id] ?? "none"}
                      onValueChange={permission => setGroupPermission(volume, group.id, permission as "none" | VaultPermission)}
                    >
                      <SelectTrigger aria-label={`${group.name} access to ${volume.label}`}><SelectValue /></SelectTrigger>
                      <SelectContent><SelectItem value="none">No access</SelectItem><SelectItem value="read">Read</SelectItem><SelectItem value="write">Read & write</SelectItem></SelectContent>
                    </Select>
                  </td>)}
                </tr>)}
              </tbody>
            </table>
          </div>
          <p className="fleet-vault-matrix-hint">New groups from Access control become columns here. Wide group sets scroll horizontally only inside this table.</p>

          {selected && <div className="fleet-vault-editor">
            <div className="fleet-vault-editor-header">
              <div><span>Selected Vault</span><strong>{selected.label} · {selected.driveLetter}:</strong></div>
              <Button size="sm" variant="danger" onClick={removeVolume}><Icon icon="trash" />Remove</Button>
            </div>
            <div className={`fleet-vault-fields${selected.kind === "dual" ? " is-dual" : ""}`}>
              <FleetField compact label="Volume name"><Input value={selected.label} onChange={event => patch({ label: event.target.value })} /></FleetField>
              <FleetField compact label="Type">
                <Select value={selected.kind} onValueChange={kind => patch({ kind: kind as VaultKind })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent><SelectItem value="container">Container</SelectItem><SelectItem value="partition">Raw partition</SelectItem><SelectItem value="dual">Decoy + hidden</SelectItem></SelectContent>
                </Select>
              </FleetField>
              <FleetField compact label="Backing path / device"><Input value={selected.backing} onChange={event => patch({ backing: event.target.value })} /></FleetField>
              <FleetField compact label="Size (MiB)"><Input type="number" min={64} value={selected.sizeMb} onChange={event => patch({ sizeMb: Number(event.target.value) })} /></FleetField>
              <FleetField compact label="Drive letter"><Input maxLength={1} value={selected.driveLetter} onChange={event => patch({ driveLetter: event.target.value.toUpperCase() })} /></FleetField>
              <FleetField compact label="Credential reference"><Input value={selected.credentialRef} onChange={event => patch({ credentialRef: event.target.value })} /></FleetField>
              {selected.kind === "dual" && <>
                <FleetField compact label="Hidden size (MiB)"><Input type="number" min={32} value={selected.hiddenSizeMb ?? 128} onChange={event => patch({ hiddenSizeMb: Number(event.target.value) })} /></FleetField>
                <FleetField compact label="Hidden credential"><Input value={selected.hiddenCredentialRef ?? ""} onChange={event => patch({ hiddenCredentialRef: event.target.value })} /></FleetField>
              </>}
            </div>
            <div className="fleet-vault-permission-head">
              <div className="fleet-vault-permission-title">
                <div><strong>Group permissions</strong><small>{groups.length ? "Set each group’s access in the table above." : "Create groups before sharing this volume."}</small></div>
                <FleetInfoPopover label="About Vault group permissions" title="How Vault access is decided" description="These permissions apply only to the selected Vault volume.">
                  <ul><li>Read allows viewing without changing files.</li><li>Read &amp; write allows viewing and changing files.</li><li>For multiple groups, the highest Vault permission wins.</li></ul>
                </FleetInfoPopover>
              </div>
              <label className="fleet-vault-owner-toggle"><Switch checked={selected.ownerOnly} onCheckedChange={checked => patch({ ownerOnly: checked, groupPermissions: checked ? {} : selected.groupPermissions })} />Owner only</label>
            </div>
            {groups.length === 0 && <div className="fleet-vault-empty-inline fleet-vault-empty-groups"><Icon icon="people" size={18} /><div><strong>No access groups</strong><small>Create groups in Access control before sharing this volume.</small></div></div>}
          </div>}
        </>}
      </CardContent>
    </Card>
  );
}
