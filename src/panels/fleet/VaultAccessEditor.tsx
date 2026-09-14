import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { FleetAccessDirectory } from "./accessControlTypes";
import type { VaultAccessEntry, VaultContainerKind } from "./vaultAccessTypes";
import { vaultAccessPreset, type VaultAccessPreset } from "./vaultAccessPresets";
import VaultAccessInfo from "./VaultAccessInfo";
import VaultAccessPatternPicker from "./VaultAccessPatternPicker";
import VaultPrincipalPicker from "./VaultPrincipalPicker";
import "./VaultAccessEditor.css";

interface VaultAccessEditorProps {
  entry: VaultAccessEntry;
  entryIndex: number;
  directory: FleetAccessDirectory;
  onEntryChange: (patch: Partial<VaultAccessEntry>) => void;
  onOwnerChange: (owner: string) => void;
  onPresetChange: (preset: Exclude<VaultAccessPreset, "custom">) => void;
}

function Field({ label, help, children }: { label: string; help: string; children: ReactNode }) {
  return <div className="fleet-field">
    <div className="vault-access-field-heading"><span>{label}</span><VaultAccessInfo label={`About ${label.toLowerCase()}`}>{help}</VaultAccessInfo></div>
    {children}
  </div>;
}

export default function VaultAccessEditor({ entry, entryIndex, directory, onEntryChange, onOwnerChange, onPresetChange }: VaultAccessEditorProps) {
  const accessPreset = vaultAccessPreset(entry);
  const vaultNumber = entryIndex + 1;

  return <div className="vault-access-editor">
    <div className="fleet-owner-inputs">
      <Field label="Vault name" help="The label people recognize.">
        <Input aria-label={`Vault ${vaultNumber} label`} value={entry.label} placeholder="Shared vault" onChange={event => onEntryChange({ label: event.target.value })} />
      </Field>
      <Field label="Container file" help="The encrypted container file on this PC. A filename extension is not required.">
        <Input aria-label={`Vault ${vaultNumber} container path`} value={entry.container_path} placeholder="Encrypted container file" onChange={event => onEntryChange({ container_path: event.target.value })} />
        <small>Keep each managed Vault in its own dedicated parent folder.</small>
      </Field>
      <Field label="Container type" help="Standard containers can be private or shared. For an outer + hidden container, choose which volume to open when mounting.">
        <select aria-label={`Vault ${vaultNumber} container type`} value={entry.container_kind} onChange={event => onEntryChange({ container_kind: event.target.value as VaultContainerKind })}>
          <option value="standard">Standard container</option><option value="dual">Outer + hidden container</option>
        </select>
        {entry.container_kind === "dual" && <small className="vault-access-security-note">A writable outer mount requires the hidden protection password for that one request.</small>}
      </Field>
      <Field label="Primary owner" help="The Windows account responsible for this Vault.">
        <Input aria-label={`Vault ${vaultNumber} owner`} value={entry.owner_account} placeholder="PC\username" onChange={event => onOwnerChange(event.target.value)} />
        <small>Use PC-or-domain\username.</small>
      </Field>
      <Field label="Drive letter" help="The preferred letter in File Explorer. Leave blank for Windows to choose.">
        <Input aria-label={`Vault ${vaultNumber} preferred drive letter`} value={entry.mount.preferred_letter ?? ""} maxLength={1} placeholder="V" onChange={event => onEntryChange({ mount: { ...entry.mount, preferred_letter: event.target.value.toUpperCase() || undefined } })} />
      </Field>
    </div>

    <VaultAccessPatternPicker value={accessPreset} onChange={onPresetChange} />

    <div className="fleet-vault-grants">
      <strong>Who can access this vault</strong>
      {accessPreset === "private" ? <div className="fleet-vault-grant-row" role="group" aria-label="Owner permission">
        <div className="fleet-field"><span>User or group</span><strong className="vault-access-principal-name">{entry.owner_account || "Choose a primary owner"}</strong></div>
        <div className="fleet-field"><span>Policy access</span><output aria-label="Owner policy access">Read &amp; write</output></div>
        <span className="vault-access-row-note">Owner only</span>
      </div> : entry.grants.map((grant, grantIndex) => <div className="fleet-vault-grant-row" role="group" aria-label={`Permission ${grantIndex + 1}`} key={`${entry.id}-${grantIndex}`}>
        <div className="fleet-field">
          <span>User or group</span>
          <VaultPrincipalPicker ariaLabel={`Grant ${grantIndex + 1} principal`} value={grant.principal_name} directory={directory} onChange={value => onEntryChange({ grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, principal_name: value } : current) })} />
        </div>
        {accessPreset === "custom" ? <label className="fleet-field"><span>Policy access</span>
          <select aria-label={`Grant ${grantIndex + 1} access`} value={grant.access} onChange={event => onEntryChange({ grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, access: event.target.value as "read" | "write" } : current) })}>
            <option value="read">Read</option><option value="write">Read &amp; write</option>
          </select>
        </label> : <div className="fleet-field"><span>Policy access</span><output aria-label={`Grant ${grantIndex + 1} access`}>{grant.access === "write" ? "Read & write" : "Read"}</output></div>}
        <Button variant="outline" size="sm" aria-label={`Remove grant ${grantIndex + 1}`} onClick={() => onEntryChange({ grants: entry.grants.filter((_, index) => index !== grantIndex) })}>Remove</Button>
      </div>)}
      {accessPreset !== "private" && <>
        <Button className="fleet-vault-add-grant" variant="outline" size="sm" onClick={() => onEntryChange({ grants: [...entry.grants, { principal_name: "", access: entry.grants[0]?.access ?? "write" }] })}>Add person or group</Button>
        <p className="fleet-field-hint">Removing a grant takes effect only after saving. Other user or group grants may still allow access.</p>
      </>}
    </div>

    <details className="vault-access-details">
      <summary>Access details</summary>
      <dl>
        <dt>Policy, not a live access check</dt>
        <dd>These rows show policy grants. Unsaved edits do not change Windows access. The service checks the calling Windows account separately.</dd>
        <dt>No access</dt>
        <dd>Removing a row removes that grant, not every route to access. This editor does not add a deny rule.</dd>
        <dt>Mounting is separate</dt>
        <dd>Saved settings do not mean a Vault is mounted. Check its current status below.</dd>
      </dl>
    </details>
  </div>;
}
