// SPDX-License-Identifier: AGPL-3.0-or-later
import { useId, useState } from "react";
import { Button } from "@/components/ui/bp";
import { Input } from "@/components/ui/input";
import type { VaultCloudBackupConfiguration } from "@/types/generated/ipc";
import { validateCloudBackupDraft } from "@/lib/vaultCloudBackup";

interface Props {
  configuration: VaultCloudBackupConfiguration | null;
  disabled: boolean;
  canSave: boolean;
  onSave: (configuration: VaultCloudBackupConfiguration) => void;
}
export default function VaultCloudBackupForm({ configuration, disabled, canSave, onSave }: Props) {
  const id = useId();
  const [wholeVault, setWholeVault] = useState(configuration?.whole_vault ?? true);
  const [sources, setSources] = useState(configuration?.source_relative_paths.join("\n") ?? "");
  const [name, setName] = useState(configuration?.remote_folder_name ?? "My Vault");
  const [automatic, setAutomatic] = useState(configuration?.automatic ?? true);
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState("");
  const save = () => {
    const next: VaultCloudBackupConfiguration = {
      whole_vault: wholeVault, source_relative_paths: wholeVault ? [] : sources.split(/\r?\n/).map(value => value.trim()).filter(Boolean),
      remote_folder_name: name.trim(), automatic,
    };
    const failure = validateCloudBackupDraft(next);
    setError(failure ?? "");
    if (!failure && consent) onSave(next);
  };
  return <section className="vault-cloud-backup__section" aria-labelledby={`${id}-title`}>
    <h3 id={`${id}-title`}>Files and automatic backup</h3>
    <fieldset disabled={disabled} className="vault-cloud-backup__fields">
      <legend className="sr-only">Backup sources</legend>
      <label><input type="radio" name={`${id}-source`} checked={wholeVault} onChange={() => setWholeVault(true)} /> Whole Vault</label>
      <label><input type="radio" name={`${id}-source`} checked={!wholeVault} onChange={() => setWholeVault(false)} /> Choose folders</label>
      {!wholeVault && <div><label htmlFor={`${id}-sources`}>Paths inside this Vault — one per line</label>
        <textarea id={`${id}-sources`} value={sources} onChange={event => setSources(event.target.value)} maxLength={8000} rows={4} placeholder={"Documents\nPhotos"} aria-describedby={`${id}-help`} />
      </div>}
      <p id={`${id}-help`} className="vault-cloud-backup__muted">For example Documents or Work\Reports. Backup settings and Windows system folders are always excluded.</p>
      <label htmlFor={`${id}-name`}>Proton backup folder name</label>
      <Input id={`${id}-name`} value={name} maxLength={128} onChange={event => setName(event.target.value)} />
      <p className="vault-cloud-backup__muted">Saved in My files / WinCommander Vault Backups / {name || "your folder"}.</p>
      <label><input type="checkbox" checked={automatic} onChange={event => setAutomatic(event.target.checked)} /> Automatically back up changes while this Vault is open</label>
      <p className="vault-cloud-backup__muted">No schedule is needed. Backup catches up when you reopen the Vault and reuses your saved Proton session.</p>
      <label><input type="checkbox" checked={consent} onChange={event => setConsent(event.target.checked)} /> I agree to copy these files to my Proton Drive account.</label>
    </fieldset>
    {error && <p role="alert">{error}</p>}
    <Button intent="primary" disabled={disabled || !canSave || !consent} onClick={save}>Save backup setup</Button>
  </section>;
}
