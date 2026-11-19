// SPDX-License-Identifier: AGPL-3.0-or-later
import { useId, useState } from "react";
import { Button } from "@/components/ui/bp";
import { Input } from "@/components/ui/input";
import { open } from "@tauri-apps/plugin-dialog";
import type { VaultCloudBackupConfiguration } from "@/types/generated/ipc";
import { validateCloudBackupDraft } from "@/lib/vaultCloudBackup";
import { addPickedVaultSources } from "@/lib/vaultCloudBackupPicker";

interface Props {
  configuration: VaultCloudBackupConfiguration | null;
  disabled: boolean;
  canSave: boolean;
  driveLabel: string;
  onSave: (configuration: VaultCloudBackupConfiguration) => void;
}
export default function VaultCloudBackupForm({ configuration, disabled, canSave, driveLabel, onSave }: Props) {
  const id = useId();
  const [wholeVault, setWholeVault] = useState(configuration?.whole_vault ?? true);
  const [sources, setSources] = useState(configuration?.source_relative_paths.join("\n") ?? "");
  const [name, setName] = useState(configuration?.remote_folder_name ?? "My Vault");
  const [automatic, setAutomatic] = useState(configuration?.automatic ?? true);
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState("");
  const [picking, setPicking] = useState(false);
  const selectedSources = sources.split(/\r?\n/).map(value => value.trim()).filter(Boolean);
  const pickSources = async (directory: boolean) => {
    setPicking(true);
    try {
      const chosen = await open({ directory, multiple: true, defaultPath: `${driveLabel.replace(/[\\/]+$/, "")}\\`, title: directory ? "Choose folders inside this Vault" : "Choose files inside this Vault" });
      if (!chosen) return;
      const next = addPickedVaultSources(driveLabel, sources, Array.isArray(chosen) ? chosen : [chosen]);
      if (next === null) { setError("Choose files or folders inside this Vault. Backup settings and system folders cannot be selected."); return; }
      setSources(next);
      setError("");
    } catch {
      setError("The file picker could not be opened. Please try again.");
    } finally { setPicking(false); }
  };
  const save = () => {
    const next: VaultCloudBackupConfiguration = {
      whole_vault: wholeVault, source_relative_paths: wholeVault ? [] : selectedSources,
      remote_folder_name: name.trim(), automatic,
    };
    const failure = validateCloudBackupDraft(next);
    setError(failure ?? "");
    if (!failure && consent) onSave(next);
  };
  return <section className="vault-cloud-backup__section" aria-labelledby={`${id}-title`}>
    <h3 id={`${id}-title`}>Backup settings</h3>
    <fieldset disabled={disabled || picking} className="vault-cloud-backup__fields">
      <legend className="sr-only">Backup sources</legend>
      <label><input type="radio" name={`${id}-source`} checked={wholeVault} onChange={() => setWholeVault(true)} /> Whole Vault</label>
      <label><input type="radio" name={`${id}-source`} checked={!wholeVault} onChange={() => setWholeVault(false)} /> Choose files or folders</label>
      {!wholeVault && <div><div className="vault-cloud-backup__actions">
        <Button icon="folder-open" aria-label="Choose folders" title="Choose folders" disabled={picking} onClick={() => void pickSources(true)} />
        <Button icon="document" aria-label="Choose files" title="Choose files" disabled={picking} onClick={() => void pickSources(false)} />
      </div><p id={`${id}-sources`}>Selected files and folders</p>
        {selectedSources.length ? <ul className="vault-cloud-backup__sources" aria-labelledby={`${id}-sources`}>
          {selectedSources.map((path, index) => <li key={`${path}-${index}`}>
            <span>{path}</span>
            <Button icon="cross" minimal aria-label={`Remove ${path}`} title={`Remove ${path}`} onClick={() => { setSources(selectedSources.filter((_, selectedIndex) => selectedIndex !== index).join("\n")); setError(""); }} />
          </li>)}
        </ul> : <p className="vault-cloud-backup__muted">No files or folders selected.</p>}
      </div>}
      <p className="vault-cloud-backup__muted">Choose files or folders inside this Vault. Backup settings and system folders are excluded.</p>
      <label htmlFor={`${id}-name`}>Proton backup folder name</label>
      <Input id={`${id}-name`} value={name} maxLength={128} onChange={event => setName(event.target.value)} />
      <label><input type="checkbox" checked={automatic} onChange={event => setAutomatic(event.target.checked)} /> Back up changes automatically while this Vault is open</label>
      <p className="vault-cloud-backup__muted">Resumes when you reopen the Vault.</p>
      <label><input type="checkbox" checked={consent} onChange={event => setConsent(event.target.checked)} /> I agree to copy these files to my Proton Drive account.</label>
    </fieldset>
    {error && <p role="alert">{error}</p>}
    <Button intent="primary" disabled={disabled || picking || !canSave || !consent} onClick={save}>Save backup setup</Button>
  </section>;
}
