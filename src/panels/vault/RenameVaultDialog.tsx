import { useCallback, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Dialog, FormGroup, Icon, InputGroup } from "@/components/ui/bp";
import VaultOperationNotice from "@/components/shared/VaultOperationNotice";
import useVaultNames from "../../hooks/useVaultNames";
import { useTheme } from "../../context/ThemeContext";
import { showSuccess } from "../../utils/toast";
import { MAX_VAULT_NAME_BYTES, findVaultName, vaultNameProblem, vaultRenameErrorMessage } from "@/lib/vaultRename";

interface RenameVaultDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** Prefilled from the vault row that opened the dialog; the person can still pick another file. */
  initialPath?: string;
}

export default function RenameVaultDialog({ isOpen, onClose, initialPath = "" }: RenameVaultDialogProps) {
  const { theme } = useTheme();
  const { names, renameVault } = useVaultNames();
  const [volumePath, setVolumePath] = useState(initialPath);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [renamed, setRenamed] = useState("");
  const currentName = findVaultName(names, volumePath);
  const problem = !volumePath.trim() ? "Choose the vault file." : vaultNameProblem(name)
    ?? (name.trim() === currentName ? "This is already the vault's name." : null);

  const selectPath = useCallback((path: string) => {
    setVolumePath(path);
    setFailure("");
    setRenamed("");
  }, []);

  const close = useCallback(() => {
    if (busy) return;
    setVolumePath("");
    setName("");
    setFailure("");
    setRenamed("");
    onClose();
  }, [busy, onClose]);

  const browse = useCallback(async () => {
    try {
      const selected = await open({ multiple: false, filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }] });
      if (typeof selected === "string") selectPath(selected);
    } catch {
      setFailure("The file picker could not be opened. Type the path instead.");
    }
  }, [selectPath]);

  const submit = useCallback(async () => {
    if (busy || problem) return;
    setBusy(true);
    setFailure("");
    try {
      const result = await renameVault(volumePath.trim(), name);
      if (result?.status !== "renamed") throw new Error("");
      setRenamed(result.name);
      showSuccess(`Vault renamed to "${result.name}".`, undefined, { kind: "notification" });
    } catch (error) {
      setFailure(vaultRenameErrorMessage(error));
    } finally {
      setBusy(false);
    }
  }, [busy, name, problem, renameVault, volumePath]);

  return (
    <Dialog isOpen={isOpen} onClose={close} title="Rename Vault"
      className={`mount-dialog vault-file-dialog vault-file-dialog--fit ${theme === "light" ? "light" : ""}`}>
      <div className="wc-dialog-body">
        <p className="vault-card-status-line">
          This changes only the name WinCommander shows. The vault file keeps its file name and location.
        </p>
        <VaultOperationNotice message={failure} />
        <VaultOperationNotice tone="success" message={renamed ? `The vault is now called "${renamed}".` : ""} />
        <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}>
          <FormGroup label="Vault file" labelFor="rename-volume-path" helperText="The vault must be dismounted.">
            <InputGroup id="rename-volume-path" placeholder="/path/to/volume.hc" value={volumePath} autoComplete="off"
              onChange={event => selectPath(event.target.value)}
              rightElement={<Button icon="folder-open" minimal aria-label="Browse for the vault file" onClick={() => void browse()} />} />
          </FormGroup>
          {volumePath.trim() && (
            <p className="vault-card-status-line" role="status">
              Current name: {currentName ? <strong>{currentName}</strong> : "none yet"}
            </p>
          )}
          <FormGroup label="New name" labelFor="rename-new-name" helperText={`Up to ${MAX_VAULT_NAME_BYTES} characters.`}>
            <InputGroup id="rename-new-name" value={name} autoComplete="off" maxLength={MAX_VAULT_NAME_BYTES}
              onChange={event => { setName(event.target.value); setFailure(""); setRenamed(""); }}
              onKeyDown={event => { if (event.key === "Enter") void submit(); }} />
          </FormGroup>
          {problem && volumePath.trim() && name && (
            <p className="vault-card-status-line" role="status"><Icon icon="info-sign" size={12} /> {problem}</p>
          )}
        </fieldset>
      </div>
      <div className="mount-dialog-footer">
        <Button icon="cross" text={renamed ? "DONE" : "CANCEL"} disabled={busy} onClick={close} minimal className="modal-cancel-btn" />
        <Button icon="edit" text="RENAME" onClick={() => void submit()} loading={busy}
          disabled={Boolean(problem) || busy} className="modal-primary-btn" />
      </div>
    </Dialog>
  );
}
