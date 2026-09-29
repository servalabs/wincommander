import { useCallback, useEffect, useState, type ReactNode } from "react";
import useBackend from "@/hooks/useBackend";
import { selectableDriveLetters } from "@/lib/vaultOperationFeedback";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { FleetAccessDirectory } from "./accessControlTypes";
import { vaultCanonicalPathDisplay, type VaultAccessEntry, type VaultOwnerPrincipal } from "./vaultAccessTypes";
import { vaultAccessPreset, type VaultAccessPreset } from "./vaultAccessPresets";
import VaultAccessInfo from "./VaultAccessInfo";
import VaultAccessPatternPicker from "./VaultAccessPatternPicker";
import VaultPrincipalPicker from "./VaultPrincipalPicker";
import "./VaultAccessEditor.css";

interface VaultAccessEditorProps {
  entry: VaultAccessEntry;
  entryIndex: number;
  directory: FleetAccessDirectory;
  ownerPrincipals: readonly VaultOwnerPrincipal[];
  currentCallerSid: string | null;
  locked?: boolean;
  ownerDirectoryUnavailable?: boolean;
  otherReservedLetters?: string[];
  onEntryChange: (patch: Partial<VaultAccessEntry>) => void;
  onOwnerChange: (owner: VaultOwnerPrincipal) => void;
  onPresetChange: (preset: Exclude<VaultAccessPreset, "custom">) => void;
}

function Field({ label, help, children }: { label: string; help: string; children: ReactNode }) {
  return <div className="fleet-field">
    <div className="vault-access-field-heading"><span>{label}</span><VaultAccessInfo label={`About ${label.toLowerCase()}`}>{help}</VaultAccessInfo></div>
    {children}
  </div>;
}

/** The service supplies only approved candidates. The selected SID remains
 * durable policy data, but the picker presents ordinary account names. */
function ownerOptionLabel(principal: VaultOwnerPrincipal, currentCallerSid: string | null) {
  const name = principal.display_name.trim() || "Windows account";
  return `${name}${principal.sid === currentCallerSid ? " (Current user)" : ""}`;
}

export default function VaultAccessEditor({ entry, entryIndex, directory, ownerPrincipals, currentCallerSid, locked = false, ownerDirectoryUnavailable = false, otherReservedLetters = [], onEntryChange, onOwnerChange, onPresetChange }: VaultAccessEditorProps) {
  const { getAvailableDriveLetters } = useBackend();
  const [availableLetters, setAvailableLetters] = useState<string[]>([]);
  const [lettersLoading, setLettersLoading] = useState(true);
  const [letterFailure, setLetterFailure] = useState("");
  const refreshLetters = useCallback(async () => {
    setLettersLoading(true);
    setLetterFailure("");
    try {
      const result = await getAvailableDriveLetters(entry.id);
      if (!result.success || !result.data) throw new Error();
      setAvailableLetters(selectableDriveLetters(result.data.letters));
    } catch {
      setAvailableLetters([]);
      setLetterFailure("Free drive letters could not be checked. Refresh before choosing a letter.");
    } finally { setLettersLoading(false); }
  }, [entry.id, getAvailableDriveLetters]);
  useEffect(() => { void refreshLetters(); }, [refreshLetters]);
  const letterChoices = selectableDriveLetters(availableLetters, otherReservedLetters);
  const selectedLetter = entry.mount.preferred_letter ?? "";
  const accessPreset = vaultAccessPreset(entry);
  const eligibleOwnerPrincipals = accessPreset === "private"
    ? ownerPrincipals.filter(principal => principal.is_local_administrator)
    : ownerPrincipals;
  const vaultNumber = entryIndex + 1;
  // Use the actually discovered signed-in account for guidance instead of
  // assuming every PC calls its administrator account "Administrator".
  const displayPath = vaultCanonicalPathDisplay(entry);
  const pathIsServiceUnavailable = entry.container_path_state === "unavailable"
    || (entry.container_path_state === "available" && displayPath === "Path unavailable");

  const browseContainerFile = async () => {
    try {
      // Do not filter by extension: an existing encrypted container is valid
      // without one. The service still validates the selected path only when
      // the administrator explicitly saves the policy.
      const selected = await openFileDialog({
        multiple: false,
        directory: false,
        title: "Select an existing encrypted Vault container",
      });
      if (typeof selected === "string") onEntryChange({ container_path: selected, container_path_state: undefined, canonical_container_path: undefined });
    } catch {
      // The path input remains available if Windows cannot open its picker.
    }
  };

  return <fieldset className="vault-access-editor" disabled={locked}>
    <div className="fleet-owner-inputs">
      <Field label="Vault name" help="The label people recognize.">
        <Input aria-label={`Vault ${vaultNumber} label`} value={entry.label} placeholder="Shared vault" onChange={event => onEntryChange({ label: event.target.value })} />
      </Field>
      <Field label="Container file" help="The encrypted container file on this PC. A filename extension is not required.">
        <div className="vault-access-container-path">
          <Input aria-label={`Vault ${vaultNumber} container path`} value={displayPath} placeholder="Encrypted container file" onChange={event => onEntryChange({ container_path: event.target.value, container_path_state: undefined, canonical_container_path: undefined })} />
          <Button variant="outline" size="sm" type="button" aria-label={`Browse for Vault ${vaultNumber} container file`} onClick={() => void browseContainerFile()}>Browse</Button>
        </div>
        <small>This permission applies only to this exact encrypted file. Sibling containers can use the same folder. If this file is replaced, select the replacement here and save, or remove the obsolete policy first.</small>
      </Field>
      <Field label="Primary owner" help={accessPreset === "private" ? "The Windows account responsible for this Vault. For a private Vault, only a service-approved local administrator can be selected. Changing it transfers ownership and is available only while the Vault is unmounted." : "The Windows account responsible for this Vault. For a shared Vault, the service validates the selected Windows account."}>
        <select
          aria-label={`Vault ${vaultNumber} primary owner`}
          value={entry.primary_owner_sid ?? ""}
          disabled={ownerDirectoryUnavailable}
          onChange={event => {
            const selected = eligibleOwnerPrincipals.find(principal => principal.sid === event.target.value);
            if (selected) onOwnerChange(selected);
          }}
        >
          <option value="" disabled>{eligibleOwnerPrincipals.length > 0 ? (accessPreset === "private" ? "Select an administrator…" : "Select a Windows user…") : (accessPreset === "private" ? "No eligible administrators found" : "No eligible Windows users found")}</option>
          {eligibleOwnerPrincipals.map(principal => <option key={principal.sid} value={principal.sid}>{ownerOptionLabel(principal, currentCallerSid)}</option>)}
        </select>
        <small>{ownerDirectoryUnavailable
          ? "Windows administrator accounts are unavailable right now, so owner selection and policy saving are disabled. Refresh this page after the local Vault service is ready."
          : accessPreset === "private"
            ? "Only service-approved local administrators appear here. Transfer ownership only while this Vault is unmounted."
            : "The service validates this Windows account."} WinCommander saves the selected Windows account securely, not merely by its displayed name.</small>
      </Field>
      <Field label="Drive letter" help="The preferred letter in File Explorer. Only letters that are free and not reserved by another Vault can be selected. Leave blank for Windows to choose.">
        <select aria-label={`Vault ${vaultNumber} preferred drive letter`} value={selectedLetter} disabled={lettersLoading} onChange={event => onEntryChange({ mount: { ...entry.mount, preferred_letter: event.target.value || undefined } })}>
          <option value="">{lettersLoading ? "Checking free letters…" : "Choose automatically"}</option>
          {selectedLetter && !letterChoices.includes(selectedLetter) && <option value={selectedLetter} disabled>{selectedLetter}: — {lettersLoading ? "checking" : "unavailable"}</option>}
          {letterChoices.map(letter => <option key={letter} value={letter}>{letter}:</option>)}
        </select>
        <Button type="button" variant="outline" size="sm" disabled={lettersLoading} onClick={() => void refreshLetters()}>Refresh free letters</Button>
        {letterFailure && <small role="alert">{letterFailure}</small>}
        {!lettersLoading && selectedLetter && !letterChoices.includes(selectedLetter) && !locked && <small role="alert">This letter is occupied or reserved. Choose another free letter before saving.</small>}
      </Field>
    </div>

    {entry.container_path_state && <p className="fleet-field-hint" role="status">
      {pathIsServiceUnavailable ? "Path unavailable" : "Saved container path verified by the Vault service."}
    </p>}

    <VaultAccessPatternPicker value={accessPreset} disabled={locked} onChange={onPresetChange} />

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
        <label className="fleet-field"><span>Policy access</span>
          <select aria-label={`Grant ${grantIndex + 1} access`} value={grant.access} onChange={event => onEntryChange({ grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, access: event.target.value as "read" | "write" } : current) })}>
            <option value="read">Read</option><option value="write">Read &amp; write</option>
          </select>
        </label>
        <Button variant="outline" size="sm" aria-label={`Remove grant ${grantIndex + 1}`} onClick={() => onEntryChange({ grants: entry.grants.filter((_, index) => index !== grantIndex) })}>Remove</Button>
      </div>)}
      {accessPreset !== "private" && <>
        <Button className="fleet-vault-add-grant" variant="outline" size="sm" onClick={() => onEntryChange({ grants: [...entry.grants, { principal_name: "", access: accessPreset === "shared-write" ? "write" : "read" }] })}>Add person or group</Button>
        <p className="fleet-field-hint">Shared, view only starts with the primary owner able to edit and everyone else able to read. Change only a selected person or group to Read &amp; write for an exception; the other rows stay unchanged. Removing a grant takes effect only after saving. Other user or group grants may still allow access.</p>
      </>}
    </div>

    {locked && <p className="fleet-vault-verification-warning" role="status">This Vault is mounted. Dismount it before changing its owner, access, container, or policy.</p>}

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
  </fieldset>;
}
