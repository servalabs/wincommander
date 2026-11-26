import { useCallback, useReducer, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Dialog, FormGroup, Icon, InputGroup, Spinner } from "@/components/ui/bp";
import VaultOperationNotice from "@/components/shared/VaultOperationNotice";
import useBackend from "../../hooks/useBackend";
import { useTheme } from "../../context/ThemeContext";
import { showSuccess } from "../../utils/toast";
import {
  RENAME_BUSY,
  RENAME_PASSWORD_NOTE,
  buildRenameRequest,
  readRenameResult,
  renameProblem,
  renameReducer,
  startRename,
} from "@/lib/vaultFileRename";

interface RenameVaultDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** Prefilled from the vault row that opened the dialog. */
  initialPath?: string;
  /** A row's own vault cannot be swapped for another file here. */
  pathLocked?: boolean;
  onRenamed?: (path: string) => void;
}

export default function RenameVaultDialog({ isOpen, onClose, initialPath = "", pathLocked = false, onRenamed }: RenameVaultDialogProps) {
  const { theme } = useTheme();
  const { renameVaultFile } = useBackend();
  const [flow, dispatch] = useReducer(renameReducer, initialPath, startRename);
  const [showPassword, setShowPassword] = useState(false);
  const [showExtras, setShowExtras] = useState(false);
  const { busy, renamed } = flow;
  const problem = renameProblem(flow);

  const close = useCallback(() => {
    if (busy) return;
    dispatch({ type: "reset" });
    setShowExtras(false);
    onClose();
  }, [busy, onClose]);

  const browse = useCallback(async (target: "volume" | "keyfile") => {
    try {
      const selected = await open(target === "volume"
        ? { multiple: false, filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }] }
        : { multiple: false });
      if (typeof selected !== "string") return;
      dispatch(target === "volume" ? { type: "path", volumePath: selected } : { type: "edit", field: "oldKeyfile", value: selected });
    } catch {
      dispatch({ type: "notice", failure: "The file picker could not be opened. Type the path instead." });
    }
  }, []);

  const submit = useCallback(async () => {
    if (busy || problem || renamed) return;
    const request = buildRenameRequest(flow.volumePath, flow.newName, flow.credentials);
    dispatch({ type: "send" });
    setShowExtras(false);
    try {
      const result = readRenameResult(await renameVaultFile(request));
      if (!result) throw new Error("");
      dispatch({ type: "renamed", result });
      showSuccess(`Renamed to ${result.name}.`, undefined, { kind: "notification" });
      onRenamed?.(result.path);
    } catch (error) {
      dispatch({ type: "failed", error });
    }
  }, [busy, flow, onRenamed, problem, renameVaultFile, renamed]);

  return (
    <Dialog isOpen={isOpen} onClose={close} title="Rename Vault"
      className={`mount-dialog vault-file-dialog ${theme === "light" ? "light" : ""}`}>
      <div className="wc-dialog-body">
        <VaultOperationNotice tone="success" message={renamed ? `Renamed to ${renamed}.` : ""} />
        <VaultOperationNotice message={flow.failure} />
        {!renamed && (
          <fieldset disabled={busy} className="vault-file-dialog__fields">
            <FormGroup label="Vault file" labelFor="rename-volume-path" helperText="The vault must be dismounted.">
              <InputGroup id="rename-volume-path" placeholder="/path/to/volume.hc" value={flow.volumePath} autoComplete="off"
                disabled={pathLocked} onChange={event => dispatch({ type: "path", volumePath: event.target.value })}
                rightElement={pathLocked ? undefined
                  : <Button icon="folder-open" minimal aria-label="Browse for the vault file" onClick={() => void browse("volume")} />} />
            </FormGroup>
            <FormGroup label="New file name" labelFor="rename-new-name" helperText="Leave out the extension to keep the current one.">
              <InputGroup id="rename-new-name" value={flow.newName} autoComplete="off" maxLength={255}
                onChange={event => dispatch({ type: "name", newName: event.target.value })} />
            </FormGroup>
            <FormGroup label="Current password" labelFor="rename-password" helperText={RENAME_PASSWORD_NOTE}>
              <InputGroup id="rename-password" type={showPassword ? "text" : "password"} value={flow.credentials.oldPassword}
                autoComplete="off" onChange={event => dispatch({ type: "edit", field: "oldPassword", value: event.target.value })}
                onKeyDown={event => { if (event.key === "Enter") void submit(); }}
                rightElement={<Button icon={showPassword ? "eye-off" : "eye-open"} minimal
                  aria-label={showPassword ? "Hide password" : "Show password"} onClick={() => setShowPassword(!showPassword)} />} />
            </FormGroup>
            <Button minimal small icon={showExtras ? "chevron-down" : "chevron-right"} className="vault-file-dialog__advanced"
              text="Advanced: PIM and keyfile" onClick={() => setShowExtras(!showExtras)} aria-expanded={showExtras} />
            {showExtras && (
              <div className="mount-options-grid" aria-label="PIM and keyfile options">
                <FormGroup label="PIM (optional)" labelFor="rename-pim" helperText="Leave blank unless the vault uses a custom PIM.">
                  <InputGroup id="rename-pim" type="number" min={1} max={2147468} placeholder="Default" value={flow.credentials.oldPim}
                    autoComplete="off" onChange={event => dispatch({ type: "edit", field: "oldPim", value: event.target.value })} />
                </FormGroup>
                <FormGroup label="Keyfile (optional)" labelFor="rename-keyfile">
                  <InputGroup id="rename-keyfile" placeholder="Path to keyfile" value={flow.credentials.oldKeyfile} autoComplete="off"
                    onChange={event => dispatch({ type: "edit", field: "oldKeyfile", value: event.target.value })}
                    rightElement={<Button icon="folder-open" minimal aria-label="Browse for keyfile (optional)" onClick={() => void browse("keyfile")} />} />
                </FormGroup>
              </div>
            )}
            {problem && flow.volumePath && !busy && <p className="vault-file-dialog__note" role="status"><Icon icon="info-sign" size={12} /> {problem}</p>}
          </fieldset>
        )}
      </div>
      <div className="mount-dialog-footer">
        {busy && <span className="vault-file-dialog__progress" role="status"><Spinner size={14} /> {RENAME_BUSY}</span>}
        <Button icon="cross" text={renamed ? "DONE" : "CANCEL"} disabled={busy} onClick={close} minimal className="modal-cancel-btn" />
        {!renamed && <Button icon="edit" text="RENAME" onClick={() => void submit()} loading={busy}
          disabled={Boolean(problem) || busy} className="modal-primary-btn" />}
      </div>
    </Dialog>
  );
}
