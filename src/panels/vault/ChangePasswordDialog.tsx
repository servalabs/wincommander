import { useCallback, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Dialog, FormGroup, Icon, InputGroup } from "@/components/ui/bp";
import VaultOperationNotice from "@/components/shared/VaultOperationNotice";
import useBackend from "../../hooks/useBackend";
import useVaultNames from "../../hooks/useVaultNames";
import { useTheme } from "../../context/ThemeContext";
import { newDiagnosticOperationId, recordDiagnostic } from "../../lib/diagnostics";
import { showSuccess } from "../../utils/toast";
import { findVaultKind } from "@/lib/vaultRename";
import {
  EMPTY_PASSWORD_CHANGE_FORM,
  PASSWORD_CHANGE_BUSY,
  PASSWORD_CHANGE_WARNING,
  VAULT_KIND_HINT,
  passwordChangeFormProblem,
  vaultPasswordChangeErrorMessage,
  withoutPasswords,
  type HeaderCredentials,
  type VaultPasswordChangeForm,
  type VaultPasswordKind,
} from "@/lib/vaultPasswordChange";

type HeaderName = "outer" | "hidden";
type CredentialField = keyof HeaderCredentials;

interface ChoiceProps<T extends string> {
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; text: string }>;
  onChange: (value: T) => void;
}

function Choice<T extends string>({ label, value, options, onChange }: ChoiceProps<T>) {
  return (
    <div className="mount-type-selector mb-4" role="radiogroup" aria-label={label}>
      {options.map(option => (
        <button key={option.value} type="button" role="radio" aria-checked={value === option.value}
          className={`mount-type-btn${value === option.value ? " is-active" : ""}`}
          onClick={() => onChange(option.value)}>
          {value === option.value && <span className="vault-active-dot" aria-hidden />}
          <span>{option.text}</span>
        </button>
      ))}
    </div>
  );
}

const VAULT_KIND_OPTIONS = [
  { value: "standard", text: "Standard vault (one password)" },
  { value: "decoy", text: "Decoy vault (two passwords)" },
] as const;

interface ChangePasswordDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** Prefilled from the vault row that opened the dialog; the person can still pick another file. */
  initialPath?: string;
  initialKind?: VaultPasswordKind;
}

export default function ChangePasswordDialog({ isOpen, onClose, initialPath = "", initialKind }: ChangePasswordDialogProps) {
  const { theme } = useTheme();
  const { changeVolumePassword } = useBackend();
  const { names } = useVaultNames();
  const [form, setForm] = useState<VaultPasswordChangeForm>(() => ({
    ...EMPTY_PASSWORD_CHANGE_FORM,
    volumePath: initialPath,
    vaultKind: initialKind ?? findVaultKind(names, initialPath) ?? "standard",
  }));
  const [showPasswords, setShowPasswords] = useState(false);
  const [showExtras, setShowExtras] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [changed, setChanged] = useState(false);
  const problem = passwordChangeFormProblem(form);
  const isDecoy = form.vaultKind === "decoy";
  const headers: HeaderName[] = isDecoy ? ["outer", "hidden"] : ["outer"];
  const word = (header: HeaderName) => (isDecoy ? (header === "outer" ? "decoy " : "real ") : "");

  const edit = useCallback((change: (current: VaultPasswordChangeForm) => VaultPasswordChangeForm) => {
    setForm(change);
    setFailure("");
    setChanged(false);
  }, []);

  const setPath = useCallback((volumePath: string) => {
    const hinted = findVaultKind(names, volumePath);
    edit(current => ({ ...current, volumePath, ...(hinted ? { vaultKind: hinted } : {}) }));
  }, [edit, names]);

  const updateHeader = useCallback((header: HeaderName, field: CredentialField, value: string) => {
    edit(current => ({ ...current, [header]: { ...current[header], [field]: value } }));
  }, [edit]);

  const close = useCallback(() => {
    if (busy) return;
    setForm(EMPTY_PASSWORD_CHANGE_FORM);
    setFailure("");
    setChanged(false);
    setShowExtras(false);
    onClose();
  }, [busy, onClose]);

  const browse = useCallback(async (target: "volume" | { header: HeaderName; field: "oldKeyfile" | "newKeyfile" }) => {
    try {
      const selected = await open(target === "volume"
        ? { multiple: false, filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }] }
        : { multiple: false });
      if (typeof selected !== "string") return;
      if (target === "volume") setPath(selected);
      else updateHeader(target.header, target.field, selected);
    } catch {
      setFailure("The file picker could not be opened. Type the path instead.");
    }
  }, [setPath, updateHeader]);

  const submit = useCallback(async () => {
    if (busy || problem) return;
    const operationId = newDiagnosticOperationId("vault");
    setBusy(true);
    setFailure("");
    const request = form;
    // Passwords leave component state as soon as the request is sent.
    setForm(withoutPasswords);
    try {
      const result = await changeVolumePassword(request);
      if (!result.success || result.data?.status !== "password_changed") throw new Error(result.error || "");
      setChanged(true);
      showSuccess(request.vaultKind === "decoy" ? "Vault passwords changed." : "Vault password changed.", undefined, { kind: "notification", operationId });
      recordDiagnostic({ operationId, feature: "vault", action: "change_password", stage: "service_receipt",
        lifecycle: "verified", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "local_sensitive" });
    } catch (error) {
      setFailure(vaultPasswordChangeErrorMessage(error));
      recordDiagnostic({ operationId, feature: "vault", action: "change_password", stage: "service_receipt",
        lifecycle: "verified", outcome: "failed", errorCode: "VLT.PASSWORD.CHANGE_FAILED", severity: "warn",
        retryability: "manual", suggestedNextAction: "review_status", privacyClass: "local_sensitive" });
    } finally {
      setBusy(false);
    }
  }, [busy, changeVolumePassword, form, problem]);

  const passwordField = (header: HeaderName, field: "oldPassword" | "newPassword" | "confirmPassword", label: string) => (
    <FormGroup key={`${header}-${field}`} label={label} labelFor={`change-${header}-${field}`}>
      <InputGroup id={`change-${header}-${field}`} type={showPasswords ? "text" : "password"} value={form[header][field]}
        autoComplete="off" onChange={event => updateHeader(header, field, event.target.value)}
        rightElement={header === "outer" && field === "oldPassword" ? (
          <Button icon={showPasswords ? "eye-off" : "eye-open"} minimal
            aria-label={showPasswords ? "Hide passwords" : "Show passwords"}
            onClick={() => setShowPasswords(!showPasswords)} />
        ) : undefined} />
    </FormGroup>
  );

  const keyfileField = (header: HeaderName, field: "oldKeyfile" | "newKeyfile", label: string) => (
    <FormGroup key={`${header}-${field}`} label={label} labelFor={`change-${header}-${field}`}>
      <InputGroup id={`change-${header}-${field}`} placeholder="Path to keyfile" value={form[header][field]} autoComplete="off"
        onChange={event => updateHeader(header, field, event.target.value)}
        rightElement={<Button icon="folder-open" minimal aria-label={`Browse for ${label.toLowerCase()}`} onClick={() => void browse({ header, field })} />} />
    </FormGroup>
  );

  const pimField = (header: HeaderName, field: "oldPim" | "newPim", label: string) => (
    <FormGroup key={`${header}-${field}`} label={label} labelFor={`change-${header}-${field}`} helperText="Leave blank unless the vault uses a custom PIM.">
      <InputGroup id={`change-${header}-${field}`} type="number" min={1} max={2147468} placeholder="Default" value={form[header][field]}
        autoComplete="off" onChange={event => updateHeader(header, field, event.target.value)} />
    </FormGroup>
  );

  return (
    <Dialog isOpen={isOpen} onClose={close} title="Change Vault Password"
      className={`mount-dialog vault-file-dialog ${theme === "light" ? "light" : ""}`}>
      <div className="wc-dialog-body">
        <Callout intent="warning" icon="warning-sign" className="mb-4">{PASSWORD_CHANGE_WARNING}</Callout>
        <VaultOperationNotice tone="info" message={busy ? PASSWORD_CHANGE_BUSY : ""} />
        <VaultOperationNotice message={failure} />
        <VaultOperationNotice tone="success" message={changed
          ? `The ${isDecoy ? "passwords were" : "password was"} changed. Use the new ${isDecoy ? "passwords" : "password"} the next time you mount this vault.`
          : ""} />
        <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}>
          <FormGroup label="Vault file" labelFor="change-volume-path" helperText="The vault must be dismounted.">
            <InputGroup id="change-volume-path" placeholder="/path/to/volume.hc" value={form.volumePath} autoComplete="off"
              onChange={event => setPath(event.target.value)}
              rightElement={<Button icon="folder-open" minimal aria-label="Browse for the vault file" onClick={() => void browse("volume")} />} />
          </FormGroup>

          <FormGroup label="Vault type" helperText={VAULT_KIND_HINT}>
            <Choice label="Vault type" value={form.vaultKind} options={VAULT_KIND_OPTIONS}
              onChange={value => edit(current => ({ ...current, vaultKind: value }))} />
          </FormGroup>

          {headers.map(header => [
            passwordField(header, "oldPassword", `Current ${word(header)}password`),
            passwordField(header, "newPassword", `New ${word(header)}password`),
            passwordField(header, "confirmPassword", `Type the new ${word(header)}password again`),
          ])}

          <Button minimal small icon={showExtras ? "chevron-down" : "chevron-right"}
            text="Advanced: PIM and keyfile" onClick={() => setShowExtras(!showExtras)} aria-expanded={showExtras} />
          {showExtras && (
            <div className="mount-options-grid" aria-label="PIM and keyfile options">
              {headers.map(header => [
                pimField(header, "oldPim", `Current ${word(header)}PIM (optional)`),
                pimField(header, "newPim", `New ${word(header)}PIM (optional)`),
                keyfileField(header, "oldKeyfile", `Current ${word(header)}keyfile (optional)`),
                keyfileField(header, "newKeyfile", `New ${word(header)}keyfile (optional)`),
              ])}
            </div>
          )}
          {problem && form.volumePath && <p className="vault-card-status-line" role="status"><Icon icon="info-sign" size={12} /> {problem}</p>}
        </fieldset>
      </div>
      <div className="mount-dialog-footer">
        <Button icon="cross" text={changed ? "DONE" : "CANCEL"} disabled={busy} onClick={close} minimal className="modal-cancel-btn" />
        <Button icon="key" text="CHANGE PASSWORD" onClick={() => void submit()} loading={busy}
          disabled={Boolean(problem) || busy} className="modal-primary-btn" />
      </div>
    </Dialog>
  );
}
